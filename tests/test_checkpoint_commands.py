"""Content-bound checkpoint decisions, conditional UI, and single-file commits."""

import contextlib
import io
import json
import os
import select
import subprocess
import sys
import time
from unittest import mock

from runner import checkpoints, coven, storage
from test_coven import CovenTestCase, ROOT, snapshot


REAL_SEND = coven.send_to_agent_pane
WORKER = """
import contextlib, sys
from runner import coven, storage
coven.send_to_agent_pane = lambda *args: False
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


class CheckpointCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        sender = mock.patch.object(coven, "send_to_agent_pane", return_value=False)
        self.sender = sender.start()
        self.addCleanup(sender.stop)
        status = mock.patch.object(coven, "agent_runtime_status", return_value=("stopped", None))
        status.start()
        self.addCleanup(status.stop)

    def view(self, checkpoint="plan-approval"):
        return json.loads(self.invoke("checkpoint", "inspect", self.workspace, checkpoint, "--json"))

    def spawn(self, code, *args):
        process = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        return process

    def simultaneous(self, commands):
        with coven.workspace_mutation(self.workspace):
            children = [self.spawn(WORKER, *args) for args in commands]
            for child in children:
                self.assertTrue(select.select([child.stdout], [], [], 15)[0])
                self.assertEqual(child.stdout.readline().strip(), "ready")
        results = []
        for child in children:
            out, err = child.communicate(timeout=25)
            results.append((child.returncode, out, err))
        return results

    def test_exact_inspection_conditional_decision_and_all_readers_detect_edits(self):
        before = snapshot(self.workspace)
        inspected = self.view()
        self.assertEqual(snapshot(self.workspace), before)
        result = json.loads(self.invoke("approve", self.workspace, "plan-approval", "--expect-revision", inspected["revision"], "--json"))
        self.assertEqual(result["outcome"], "committed")
        self.assertEqual(result["selection"], "conditional")
        self.assertEqual(result["before_revision"], inspected["revision"])
        self.assertEqual(result["scope_revision"], inspected["scope_revision"])
        path = self.workspace / "checkpoints/plan-approval.md"
        path.write_bytes(path.read_bytes() + b"\nNew acceptance criterion.\n")
        edited = path.read_bytes()
        view = self.view()
        self.assertEqual(view["effective_status"], "approval_stale")
        self.assertNotEqual(view["scope_revision"], result["scope_revision"])
        self.assertIn("| `plan-approval` | approval_stale |", self.invoke("status", self.workspace))
        self.assertIn("`plan-approval` — approval_stale", self.invoke("standup", self.workspace))
        self.assertIn("plan-approval", coven.checkpoint_review_candidates(self.workspace))
        self.assertIn("approve checkpoint `plan-approval`", self.invoke("next", self.workspace))
        self.invoke("refresh", self.workspace)
        self.assertEqual(path.read_bytes(), edited)
        rebound = json.loads(self.invoke("approve", self.workspace, "plan-approval", "--expect-revision", view["revision"], "--json"))
        self.assertEqual(rebound["scope_revision"], view["scope_revision"])
        self.assertEqual(self.view()["effective_status"], "approved")

    def test_legacy_approved_is_unbound_without_rewriting_it(self):
        path = self.workspace / "checkpoints/plan-approval.md"
        path.write_bytes(path.read_bytes().replace(b"Status: pending", b"Status: approved"))
        original = path.read_bytes()
        self.assertEqual(self.view()["effective_status"], "approved_unbound")
        self.assertIn("approve checkpoint `plan-approval`", self.invoke("next", self.workspace))
        self.invoke("refresh", self.workspace)
        self.assertEqual(path.read_bytes(), original)
        self.invoke("approve", self.workspace, "plan-approval")
        self.assertEqual(self.view()["binding"], "bound")

    def test_snapshot_not_scope_controls_stale_decisions(self):
        inspected = self.view()
        self.invoke("approve", self.workspace, "plan-approval", "--expect-revision", inspected["revision"])
        current = self.view()
        self.assertEqual(current["scope_revision"], inspected["scope_revision"])
        before = snapshot(self.workspace)
        self.sender.reset_mock()
        self.invoke("suggest", self.workspace, "plan-approval", "Reconsider", "--expect-revision", inspected["revision"], error="Stale checkpoint revision")
        self.invoke("approve", self.workspace, "plan-approval", "--expect-revision", current["scope_revision"], error="invalid expected checkpoint revision")
        self.assertEqual(snapshot(self.workspace), before)
        self.sender.assert_not_called()

    def test_conditional_approve_suggest_have_one_winner(self):
        revision = self.view()["revision"]
        results = self.simultaneous([
            ("approve", self.workspace, "plan-approval", "--expect-revision", revision, "--json"),
            ("suggest", self.workspace, "plan-approval", "More QA", "--expect-revision", revision, "--json"),
        ])
        self.assertEqual(sorted(code for code, _, _ in results), [0, 1])
        winner = next(json.loads(out) for code, out, _ in results if code == 0)
        self.assertIn("Stale checkpoint revision", next(err for code, _, err in results if code == 1))
        current = self.view()
        self.assertEqual(current["decision"]["decision_id"], winner["decision_id"])
        self.assertEqual(len(current["history"]), 1)

    def test_unconditional_decisions_preserve_all_history_under_contention(self):
        original_scope = self.view()["scope_revision"]
        results = self.simultaneous([
            ("approve", self.workspace, "plan-approval", "--message", f"Review {index}", "--json") for index in range(3)
        ])
        for code, out, err in results:
            self.assertEqual(code, 0, err)
            self.assertEqual(json.loads(out)["selection"], "latest")
        current = self.view()
        self.assertEqual(current["scope_revision"], original_scope)
        self.assertEqual({record["notes"] for record in current["history"]}, {f"Review {index}" for index in range(3)})

    def test_missing_revision_cannot_overwrite_concurrent_creation(self):
        token = self.view("custom")["revision"]
        self.assertEqual(token, "checkpoint-missing-v1:custom")
        results = self.simultaneous([
            ("approve", self.workspace, "custom", "--expect-revision", token, "--json") for _ in range(2)
        ])
        self.assertEqual(sorted(code for code, _, _ in results), [0, 1])
        self.assertEqual(len(self.view("custom")["history"]), 1)

    def test_prepublication_error_preserves_authority_and_secondary_files(self):
        before = snapshot(self.workspace)
        with mock.patch.object(storage.os, "replace", side_effect=OSError("before checkpoint replace")):
            self.invoke("approve", self.workspace, "--json", error="snapshot was not replaced")
        self.assertEqual(snapshot(self.workspace), before)
        self.sender.assert_not_called()

    def test_uncertain_publication_skips_secondary_effects(self):
        preserved = {name: (self.workspace / name).read_bytes() for name in ("logs/events.jsonl", "logs/messages.jsonl", "dashboard.md")}
        output = io.StringIO()
        with mock.patch.object(storage, "sync_directory", side_effect=OSError("directory fsync failed")), contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(["approve", str(self.workspace), "plan-approval", "--json"]), 2)
        result = json.loads(output.getvalue())
        self.assertEqual(result["outcome"], "durability_uncertain")
        self.assertEqual(self.view()["decision"]["decision_id"], result["decision_id"])
        for name, data in preserved.items():
            self.assertEqual((self.workspace / name).read_bytes(), data)
        self.sender.assert_not_called()

    def test_secondary_errors_are_committed_warnings_and_corrupt_history_is_not_authority(self):
        for helper in ("append_event", "append_message", "refresh_dashboard"):
            with self.subTest(helper=helper), mock.patch.object(coven, helper, side_effect=storage.StorageError("secondary unavailable")):
                result = json.loads(self.invoke("approve", self.workspace, "--json"))
            self.assertEqual(result["outcome"], "committed")
            self.assertTrue(result["warnings"])
        for name in ("events", "messages"):
            (self.workspace / f"logs/{name}.jsonl").write_bytes(b"corrupt\n")
        result = json.loads(self.invoke("suggest", self.workspace, "plan-approval", "Changes required", "--json"))
        self.assertEqual(result["outcome"], "committed")
        self.assertTrue(result["warnings"])
        self.assertEqual(self.view()["effective_status"], "changes_requested")

    def test_notifications_run_outside_lock_and_observe_scoped_identity(self):
        acquired = []
        def observe(team, agent, message):
            with storage.workspace_lock(self.workspace, coven.managed_path(self.workspace, "state/.workspace.lock"), timeout=0):
                acquired.append(agent)
            self.assertIn("scope checkpoint-scope-v1:", message)
            self.assertIn("not execution authorization", message)
            self.assertNotIn("Continue your assigned task", message)
            return True
        self.sender.side_effect = observe
        result = json.loads(self.invoke("approve", self.workspace, "--json"))
        self.assertEqual(result["sent"], 5)
        self.assertEqual(len(acquired), 5)
        self.assertEqual(result["warnings"], [])

    def test_superseded_or_staled_notifications_are_suppressed_between_recipients(self):
        path = self.workspace / "checkpoints/plan-approval.md"
        def stale_after_first(*args):
            path.write_bytes(path.read_bytes() + b"\nChanged scope after the first notice\n")
            return True
        self.sender.side_effect = stale_after_first
        result = json.loads(self.invoke("approve", self.workspace, "--json"))
        self.assertEqual(result["sent"], 1)
        self.assertEqual(self.sender.call_count, 1)
        self.assertTrue(any("suppressed" in text for text in result["warnings"]))
        self.assertEqual(self.view()["effective_status"], "approval_stale")
        self.invoke("status", self.workspace)
        self.assertIn("approval_stale", (self.workspace / "dashboard.md").read_text())

    def test_failed_native_transport_does_not_report_delivery_or_rollback(self):
        for multiplexer in ("tmux", "herdr"):
            with self.subTest(multiplexer=multiplexer), mock.patch.dict(os.environ, {"COVEN_MULTIPLEXER": multiplexer}), \
                    mock.patch.object(coven, "send_to_agent_pane", new=REAL_SEND), \
                    mock.patch.object(coven, "running_agent_pane", return_value="pane-1"), \
                    mock.patch.object(coven.subprocess, "run", return_value=mock.Mock(returncode=1, stderr="rejected")):
                result = json.loads(self.invoke("approve", self.workspace, "--json"))
            self.assertEqual(result["outcome"], "committed")
            self.assertEqual(result["sent"], 0)
            self.assertTrue(any("delivery failed" in text for text in result["warnings"]))
            self.assertEqual(self.view()["effective_status"], "approved")

    def test_herdr_text_success_but_enter_failure_is_not_successful_delivery(self):
        def transport(command, **kwargs):
            failed = command[2] == "send-keys"
            return mock.Mock(returncode=1 if failed else 0, stderr="enter rejected" if failed else "")
        with mock.patch.dict(os.environ, {"COVEN_MULTIPLEXER": "herdr"}), \
                mock.patch.object(coven, "send_to_agent_pane", new=REAL_SEND), \
                mock.patch.object(coven, "running_agent_pane", return_value="pane-1"), \
                mock.patch.object(coven.subprocess, "run", side_effect=transport) as commands:
            result = json.loads(self.invoke("approve", self.workspace, "--json"))
        self.assertEqual(result["outcome"], "committed")
        self.assertEqual(result["sent"], 0)
        self.assertEqual(commands.call_count, 10)
        self.assertTrue(any("enter rejected" in text for text in result["warnings"]))
        events = storage.read_jsonl((self.workspace / "logs/events.jsonl").resolve())
        self.assertFalse(any(event["type"] == "checkpoint.notice.sent" for event in events))

    def test_actual_new_decision_suppresses_the_previous_notice_between_recipients(self):
        deliveries = []
        def replace_after_first(team, agent, message):
            deliveries.append(agent)
            with mock.patch.object(coven, "deliver_checkpoint_notice", return_value=None):
                coven.approve_checkpoint(self.workspace, coven.load_team(self.workspace), "plan-approval", "New approval of unchanged scope")
            return True
        self.sender.side_effect = replace_after_first
        result = json.loads(self.invoke("approve", self.workspace, "--json"))
        self.assertEqual(deliveries, ["clippy"])
        self.assertEqual(result["sent"], 1)
        self.assertEqual(self.view()["effective_status"], "approved")
        self.assertEqual(self.view()["scope_revision"], result["scope_revision"])
        self.assertNotEqual(self.view()["decision"]["decision_id"], result["decision_id"])
        self.assertTrue(any("superseded" in message for message in result["warnings"]))

    def test_review_uses_displayed_revision_even_when_multiline_input_changes_file(self):
        for suggestion in (False, True):
            with self.subTest(suggestion=suggestion):
                path = self.workspace / "checkpoints/plan-approval.md"
                before_events = (self.workspace / "logs/events.jsonl").read_bytes()
                def changed(*args):
                    path.write_bytes(path.read_bytes() + b"\nEdited while reviewing\n")
                    return "More QA" if suggestion else "a"
                inputs = ["s", changed, "."] if suggestion else [changed]
                def answer(*args):
                    value = inputs.pop(0)
                    return value() if callable(value) else value
                output, errors = io.StringIO(), io.StringIO()
                with mock.patch("builtins.input", side_effect=answer), contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                    self.assertEqual(coven.main(["review", str(self.workspace), "plan-approval"]), 1)
                self.assertIn("Stale checkpoint revision", errors.getvalue())
                self.assertEqual((self.workspace / "logs/events.jsonl").read_bytes(), before_events)
                self.assertIsNone(self.view()["decision"])
        self.sender.assert_not_called()

    def test_console_conflict_and_uncertainty_exit_statuses_survive_quit_and_nested_review(self):
        def edit_and_confirm(*args):
            path = self.workspace / "checkpoints/plan-approval.md"
            path.write_bytes(path.read_bytes() + b"\nEdited before console confirmation\n")
            return "y"
        inputs = ["approve plan-approval", edit_and_confirm, "quit"]
        def answer(*args):
            item = inputs.pop(0)
            return item() if callable(item) else item
        with mock.patch("builtins.input", side_effect=answer), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(coven.main(["console", str(self.workspace)]), 1)
        self.assertIsNone(self.view()["decision"])
        for inputs in (["approve plan-approval", "y", "quit"], ["suggest plan-approval More QA", "y", EOFError()],
                       ["review plan-approval", "a", "quit"]):
            with self.subTest(inputs=inputs), mock.patch("builtins.input", side_effect=inputs), \
                    mock.patch.object(storage, "sync_directory", side_effect=OSError("uncertain")), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(coven.main(["console", str(self.workspace)]), 2)

    def test_external_link_target_is_deliberately_not_checkpoint_scope(self):
        path = self.workspace / "checkpoints/plan-approval.md"
        path.write_bytes(path.read_bytes() + b"\nSee [plan](../spec.md) for details.\n")
        self.invoke("approve", self.workspace)
        revision = self.view()["scope_revision"]
        (self.workspace / "spec.md").write_text("External plan changed\n")
        self.assertEqual(self.view()["scope_revision"], revision)
        self.assertEqual(self.view()["effective_status"], "approved")

    def test_task_ledger_authority_does_not_absorb_checkpoint_decisions(self):
        token = json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]
        self.invoke("state", "import-tasks", self.workspace, "--expect-revision", token)
        before = json.loads(self.invoke("task", "list", self.workspace, "--json"))
        self.invoke("approve", self.workspace)
        self.invoke("suggest", self.workspace, "plan-approval", "Rework")
        after = json.loads(self.invoke("task", "list", self.workspace, "--json"))
        self.assertEqual(after["revision"], before["revision"])
        self.assertEqual(after["sequence"], 0)

    def test_atomic_checkpoint_readers_see_complete_old_or_new_files(self):
        code = """
import sys
from pathlib import Path
from runner import coven
w = Path(sys.argv[1])
coven.send_to_agent_pane = lambda *args: False
for index in range(8):
    coven.approve_checkpoint(w, coven.load_team(w), 'plan-approval', 'Review '+str(index))
"""
        child = self.spawn(code, self.workspace)
        count, deadline = 0, time.monotonic() + 25
        while child.poll() is None and time.monotonic() < deadline:
            checkpoints.inspect("plan-approval", (self.workspace / "checkpoints/plan-approval.md").read_bytes())
            count += 1
            time.sleep(0.001)
        _, error = child.communicate(timeout=10)
        self.assertEqual(child.returncode, 0, error)
        self.assertGreater(count, 0)
        self.assertEqual(len(self.view()["history"]), 8)

    def test_deterministic_errors_do_not_create_a_legacy_lock(self):
        (self.workspace / "state/.workspace.lock").unlink()  # Disposable inactive fixture.
        before = snapshot(self.workspace)
        self.invoke("approve", self.workspace, "--expect-revision", "bad", error="invalid expected")
        self.invoke("suggest", self.workspace, "plan-approval", "raw -->", error="reserved machinery")
        self.assertEqual(snapshot(self.workspace), before)
