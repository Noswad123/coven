"""Independent-process task races and honest snapshot/secondary failure outcomes."""

import contextlib
import io
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time
from unittest import mock

from runner import coven, records, storage
from test_coven import CovenTestCase, ROOT, snapshot


WORKER = """
import contextlib, sys
from runner import coven, storage
original = storage.workspace_lock
@contextlib.contextmanager
def announced(*args, **kwargs):
    if kwargs.get('held') is None:
        print('ready', flush=True)
    with original(*args, **kwargs) as held:
        yield held
storage.workspace_lock = announced
raise SystemExit(coven.main(sys.argv[1:]))
"""


class TaskMutationTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)

    def spawn(self, code, *args):
        env = {key: value for key, value in os.environ.items()
               if key not in {"COVEN_HOME", "COVEN_COMMAND", "COVEN_MULTIPLEXER"}}
        process = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT, env=env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        return process

    def wait_line(self, process, expected):
        self.assertTrue(select.select([process.stdout], [], [], 15)[0], "child did not signal readiness")
        actual = process.stdout.readline().strip()
        if actual != expected:
            self.fail(f"expected {expected!r}, got {actual!r}; child={process.communicate(timeout=10)}")

    def snapshot_json(self):
        return json.loads(self.invoke("task", "list", self.workspace, "--json"))

    def simultaneous(self, commands):
        with coven.workspace_mutation(self.workspace):
            processes = [self.spawn(WORKER, *command) for command in commands]
            for process in processes:
                self.wait_line(process, "ready")
        results = []
        for process in processes:
            stdout, stderr = process.communicate(timeout=25)
            results.append((process.returncode, stdout, stderr))
        return results

    def test_concurrent_adds_keep_every_task_and_allocate_ids_under_lock(self):
        original_count = len(self.read_json("state/tasks.json"))
        commands = [("task", "add", self.workspace, "Parallel work", "--json") for _ in range(6)]
        results = self.simultaneous(commands)
        ids = []
        for code, stdout, stderr in results:
            self.assertEqual(code, 0, stderr)
            result = json.loads(stdout)
            self.assertEqual(result["outcome"], "committed")
            ids.append(result["task"]["id"])
            self.assertNotEqual(result["before_revision"], result["after_revision"])
        self.assertEqual(len(set(ids)), 6)
        self.assertEqual(len(self.read_json("state/tasks.json")), original_count + 6)
        events = storage.read_jsonl((self.workspace / "logs/events.jsonl").resolve())
        mutations = [event for event in events if event.get("mutation_id")]
        self.assertEqual(len(mutations), 6)
        for previous, current in zip(mutations, mutations[1:]):
            self.assertEqual(previous["after_revision"], current["before_revision"])
        self.assertEqual(len({event["event_id"] for event in events}), len(events))
        dashboard = (self.workspace / "dashboard.md").read_text()
        self.assertTrue(all(f"`{task_id}`" in dashboard for task_id in ids))

    def test_add_and_assign_do_not_erase_each_others_updates(self):
        commands = [("task", "assign", self.workspace, "core-chatbot", "eric", "--json")]
        commands += [("task", "add", self.workspace, f"Independent {index}", "--json") for index in range(4)]
        for code, _, stderr in self.simultaneous(commands):
            self.assertEqual(code, 0, stderr)
        tasks = self.read_json("state/tasks.json")
        self.assertEqual(next(task for task in tasks if task["id"] == "core-chatbot")["owner"], "eric")
        self.assertEqual(sum(task["title"].startswith("Independent") for task in tasks), 4)

    def test_event_and_message_appenders_share_task_lock_without_interleaving(self):
        commands = [("message", self.workspace, "user", "coven", f"Concurrent note {index} " + "x" * 16000) for index in range(3)]
        commands += [("task", "add", self.workspace, f"Concurrent task {index}", "--json") for index in range(3)]
        for code, _, stderr in self.simultaneous(commands):
            self.assertEqual(code, 0, stderr)
        messages = storage.read_jsonl((self.workspace / "logs/messages.jsonl").resolve())
        self.assertEqual(len(messages), 3)
        self.assertTrue(all(len(message["body"]) > 16000 for message in messages))
        events = storage.read_jsonl((self.workspace / "logs/events.jsonl").resolve())
        self.assertEqual(sum(event["type"] == "message.sent" for event in events), 3)
        self.assertEqual(sum(bool(event.get("mutation_id")) for event in events), 3)

    def test_two_conditional_assignments_have_one_winner_not_two_claims(self):
        revision = self.snapshot_json()["revision"]
        commands = [("task", "assign", self.workspace, "core-chatbot", owner,
                     "--expect-revision", revision, "--json") for owner in ("srikanth", "eric")]
        results = self.simultaneous(commands)
        self.assertEqual(sorted(code for code, _, _ in results), [0, 1])
        winner = next(json.loads(output) for code, output, _ in results if code == 0)
        self.assertEqual(next(task for task in self.read_json("state/tasks.json") if task["id"] == "core-chatbot")["owner"], winner["task"]["owner"])
        self.assertIn("Stale tasks revision", next(error for code, _, error in results if code == 1))
        self.assertEqual(self.snapshot_json()["revision"], winner["after_revision"])

    def test_unconditional_assign_is_serial_reassignment_not_exclusive_claim(self):
        first = coven.assign_task(self.workspace, "core-chatbot", "srikanth")
        second = coven.assign_task(self.workspace, "core-chatbot", "eric")
        self.assertEqual(second["before_revision"], first["after_revision"])
        self.assertEqual(second["task"]["owner"], "eric")

    def test_revision_is_exact_bytes_and_stale_requests_have_no_side_effects(self):
        observed = self.snapshot_json()["revision"]
        tasks = self.read_json("state/tasks.json")
        path = self.workspace / "state/tasks.json"
        path.write_text(json.dumps(tasks))  # Formatting-only external edit invalidates a token.
        current = self.snapshot_json()["revision"]
        self.assertNotEqual(observed, current)
        before = snapshot(self.workspace)
        self.invoke("task", "add", self.workspace, "Stale proposal", "--expect-revision", observed,
                    error=f"current {current}")
        self.invoke("task", "assign", self.workspace, "core-chatbot", "eric", "--expect-revision", observed,
                    error="Stale tasks revision")
        self.assertEqual(snapshot(self.workspace), before)

    def test_reads_and_invalid_requests_do_not_create_legacy_lock(self):
        lock = self.workspace / "state/.workspace.lock"
        lock.unlink()  # Disposable, inactive fixture models a pre-lock legacy workspace.
        before = snapshot(self.workspace)
        self.snapshot_json()
        self.invoke("task", "add", self.workspace, "!!!", error="Invalid task title")
        self.invoke("task", "add", self.workspace, "Work", "--expect-revision", "bad", error="invalid tasks revision")
        self.invoke("task", "assign", self.workspace, "missing", "eric", error="Task not found")
        self.assertEqual(snapshot(self.workspace), before)
        self.assertFalse(lock.exists())

    def test_missing_snapshot_token_is_distinct_and_no_envelope_is_introduced(self):
        (self.workspace / "state/tasks.json").unlink()
        self.assertEqual(self.snapshot_json(), {"revision": "tasks-missing:v1", "tasks": []})
        result = json.loads(self.invoke("task", "add", self.workspace, "First task", "--expect-revision", "tasks-missing:v1", "--json"))
        self.assertEqual(result["outcome"], "committed")
        self.assertIsInstance(self.read_json("state/tasks.json"), list)
        self.assertEqual(self.snapshot_json()["revision"], result["after_revision"])

    def test_corrupt_history_blocks_task_commit_and_strict_read_commands(self):
        for filename in ("events", "messages"):
            path = self.workspace / f"logs/{filename}.jsonl"
            original = path.read_bytes()
            for bad in (b'{"ok":1}\nnot-json\n', b'{"ok":1}', b'[]\n', b'\xff\n'):
                with self.subTest(filename=filename, bad=bad):
                    path.write_bytes(bad)
                    before = snapshot(self.workspace)
                    self.invoke("task", "add", self.workspace, "Blocked by history", error="inspect/repair")
                    self.invoke(filename, self.workspace, error="line")
                    self.assertEqual(snapshot(self.workspace), before)
            path.write_bytes(original)

    def test_failure_before_replace_preserves_every_existing_file(self):
        before = snapshot(self.workspace)
        with mock.patch.object(storage.os, "replace", side_effect=OSError("injected before replace")):
            self.invoke("task", "add", self.workspace, "Not committed", error="snapshot was not replaced")
        self.assertEqual(snapshot(self.workspace), before)

    def test_committed_secondary_failures_are_success_with_warnings(self):
        for helper, warning in (("append_event", "history incomplete"), ("refresh_dashboard", "dashboard stale")):
            with self.subTest(helper=helper), mock.patch.object(coven, helper, side_effect=OSError("secondary failed")):
                result = json.loads(self.invoke("task", "add", self.workspace, f"Committed {helper}", "--json"))
            self.assertEqual(result["outcome"], "committed")
            self.assertTrue(any(warning in text for text in result["warnings"]))
            self.assertIn(result["task"]["id"], {task["id"] for task in self.read_json("state/tasks.json")})

    def test_actual_event_fsync_failure_after_replace_does_not_report_task_failure(self):
        event_inode = (self.workspace / "logs/events.jsonl").stat().st_ino
        original_fsync = storage.os.fsync
        def fail_history_sync(fd):
            if os.fstat(fd).st_ino == event_inode:
                raise OSError("injected history fsync failure")
            original_fsync(fd)
        with mock.patch.object(storage.os, "fsync", side_effect=fail_history_sync):
            result = json.loads(self.invoke("task", "add", self.workspace, "Committed despite history fsync", "--json"))
        self.assertEqual(result["outcome"], "committed")
        self.assertTrue(any("history incomplete" in text for text in result["warnings"]))
        self.assertEqual(self.snapshot_json()["revision"], result["after_revision"])
        self.assertIn(f"`{result['task']['id']}`", (self.workspace / "dashboard.md").read_text())

    def test_directory_sync_failure_reports_replaced_but_uncertain_exit_two(self):
        events = (self.workspace / "logs/events.jsonl").read_bytes()
        dashboard = (self.workspace / "dashboard.md").read_bytes()
        output = io.StringIO()
        with mock.patch.object(storage, "sync_directory", side_effect=OSError("fsync failed")), contextlib.redirect_stdout(output):
            code = coven.main(["task", "add", str(self.workspace), "Uncertain task", "--json"])
        self.assertEqual(code, 2)
        result = json.loads(output.getvalue())
        self.assertEqual(result["outcome"], "durability_uncertain")
        self.assertIn("inspect before retry", result["warnings"][0])
        self.assertEqual(self.snapshot_json()["revision"], result["after_revision"])
        self.assertEqual((self.workspace / "logs/events.jsonl").read_bytes(), events)
        self.assertEqual((self.workspace / "dashboard.md").read_bytes(), dashboard)

    def test_task_readers_see_only_complete_snapshots_during_replacements(self):
        code = """
import sys
from runner import coven
from pathlib import Path
for index in range(15):
    coven.add_task(Path(sys.argv[1]), 'Reader race '+str(index))
"""
        process = self.spawn(code, self.workspace)
        reads = 0
        deadline = time.monotonic() + 25
        while process.poll() is None and time.monotonic() < deadline:
            records.tasks(json.loads((self.workspace / "state/tasks.json").read_bytes()))
            reads += 1
            time.sleep(0.001)
        _, errors = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, errors)
        self.assertGreater(reads, 0)
        self.assertEqual(sum(task["title"].startswith("Reader race") for task in self.read_json("state/tasks.json")), 15)

    def test_dashboard_render_holds_lock_until_publication(self):
        code = """
import sys
from pathlib import Path
from runner import coven
original = coven.render_dashboard
calls = 0
def paused(workspace):
    global calls
    content = original(workspace)
    calls += 1
    # Refresh discards a read-only draft before acquiring its lock.
    if calls == (2 if sys.argv[2] == 'refresh' else 1):
        print('rendered', flush=True)
        sys.stdin.readline()
    return content
coven.render_dashboard = paused
if sys.argv[2] == 'refresh':
    coven.main(['refresh', sys.argv[1]])
else:
    coven.refresh_dashboard(Path(sys.argv[1]))
"""
        for entrypoint in ("dashboard", "refresh"):
            with self.subTest(entrypoint=entrypoint):
                process = self.spawn(code, self.workspace, entrypoint)
                self.wait_line(process, "rendered")
                path = (self.workspace / "state/.workspace.lock").resolve()
                with self.assertRaises(storage.LockBusy), storage.workspace_lock(self.workspace, path, timeout=0):
                    pass
                worker = self.spawn(WORKER, "task", "add", self.workspace, "Newer dashboard task", "--json")
                self.wait_line(worker, "ready")
                process.stdin.write("publish\n")
                process.stdin.flush()
                _, error = process.communicate(timeout=15)
                self.assertEqual(process.returncode, 0, error)
                output, error = worker.communicate(timeout=20)
                self.assertEqual(worker.returncode, 0, error)
                task_id = json.loads(output)["task"]["id"]
                self.assertIn(f"`{task_id}`", (self.workspace / "dashboard.md").read_text())

    def test_bad_refresh_preparation_does_not_create_a_legacy_lock(self):
        lock = self.workspace / "state/.workspace.lock"
        lock.unlink()  # Inactive disposable fixture, not a force-unlock procedure.
        goal = self.workspace / "goal.md"
        original = goal.read_bytes()
        goal.write_bytes(b"\xff")
        before = snapshot(self.workspace)
        self.invoke("refresh", self.workspace, error="Cannot prepare refresh")
        self.invoke("status", self.workspace, error="Cannot read managed file")
        self.assertEqual(snapshot(self.workspace), before)
        goal.write_bytes(original)
        metadata = self.workspace / "coven.json"
        team = self.read_json("coven.json")
        team["example"] = "missing-example"
        metadata.write_text(json.dumps(team))
        before = snapshot(self.workspace)
        self.invoke("refresh", self.workspace, error="Unknown coven example")
        self.assertEqual(snapshot(self.workspace), before)
        self.assertFalse(lock.exists())

    def test_process_death_before_and_after_replace_never_promotes_or_rolls_back(self):
        for phase in ("before", "after"):
            with self.subTest(phase=phase):
                before_revision = self.snapshot_json()["revision"]
                code = """
import sys
from pathlib import Path
from runner import coven, storage
phase = sys.argv[2]
if phase == 'before':
    original = storage.os.replace
    def paused(*args):
        print('paused', flush=True)
        sys.stdin.readline()
        return original(*args)
    storage.os.replace = paused
else:
    original = storage.sync_directory
    def paused(*args):
        print('paused', flush=True)
        sys.stdin.readline()
        return original(*args)
    storage.sync_directory = paused
coven.add_task(Path(sys.argv[1]), 'Killed '+phase)
"""
                process = self.spawn(code, self.workspace, phase)
                self.wait_line(process, "paused")
                process.kill()
                process.communicate(timeout=10)
                current = self.snapshot_json()
                titles = {task["title"] for task in current["tasks"]}
                if phase == "before":
                    self.assertEqual(current["revision"], before_revision)
                    self.assertNotIn("Killed before", titles)
                    self.assertTrue(list((self.workspace / "state").glob(".tasks.json.*.tmp")))
                else:
                    self.assertNotEqual(current["revision"], before_revision)
                    self.assertIn("Killed after", titles)
                    self.invoke("task", "add", self.workspace, "Do not blindly retry", "--expect-revision", before_revision,
                                error="Stale tasks revision")
                # A killed process released its lock. Old temp files are not read.
                self.invoke("task", "add", self.workspace, f"After crash {phase}")

    def test_new_prompts_and_authority_are_truthful_without_rewriting_custom_roles(self):
        self.assertEqual(self.read_json("coven.json")["source_of_truth"], "workspace-files")
        prompt = (self.workspace / "prompts/jamal.md").read_text()
        self.assertIn("--expect-revision", prompt)
        self.assertIn("Do not edit shared task JSON", prompt)
        (self.workspace / "prompts/jamal.md").write_text("Custom legacy role prompt\n")
        self.invoke("refresh", self.workspace)
        self.assertEqual((self.workspace / "prompts/jamal.md").read_text(), "Custom legacy role prompt\n")
        self.assertIn("Shared task JSON, logs, and dashboard files are not editing surfaces", (self.workspace / "prompts/jamal.start.txt").read_text())
