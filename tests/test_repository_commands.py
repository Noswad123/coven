"""Repository CLI, compatibility, and process-death publication boundaries."""

import contextlib
import io
import json
import os
import select
import shutil
import subprocess
import sys
from unittest import mock

from runner import coven, launch, repositories, storage, task_attempts, workers
from test_coven import CovenTestCase, ROOT, snapshot


class RepositoryCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        self.repo = self.root / "source checkout"
        self.repo.mkdir()
        self.git("init", "--quiet")
        (self.repo / "source.txt").write_text("committed base\n")
        self.git("add", "source.txt")
        self.git("commit", "--quiet", "-m", "disposable fixture")
        self.base = self.git("rev-parse", "HEAD").decode().strip()
        for target, name in ((launch, "execute"), (workers, "inspect_readiness"), (coven, "send_to_agent_pane")):
            guard = mock.patch.object(target, name, side_effect=AssertionError("Repository facts never start/probe workers or send prompts"))
            observed = guard.start()
            self.addCleanup(guard.stop)
            self.addCleanup(observed.assert_not_called)

    def git(self, *args):
        env = repositories._environment() | {"GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
            "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        return subprocess.run([shutil.which("git"), "-c", "commit.gpgsign=false", *args], cwd=self.repo,
            env=env, check=True, capture_output=True, timeout=10).stdout

    def register_args(self, workspace=None, target="refs/heads/main"):
        return ["repository", "register", str(workspace or self.workspace), "--path", str(self.repo), "--base", self.base,
            "--delivery-target", target, "--expect-repository-revision", repositories.UNREGISTERED, "--json"]

    def call(self, *args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        return json.loads(output.getvalue())

    def metadata(self):
        return {str(p.relative_to(self.repo)): (p.read_bytes(), p.stat().st_mtime_ns, p.stat().st_ctime_ns)
            for p in self.repo.rglob("*") if p.is_file()}

    def test_cli_lifecycle_optional_workspace_and_read_only_observations(self):
        before = snapshot(self.workspace)
        absent = self.call("repository", "inspect", self.workspace, "--verify", "--json")
        self.assertEqual(absent["revision"], repositories.UNREGISTERED)
        self.assertEqual(snapshot(self.workspace), before)
        (self.repo / "source.txt").write_text("dirty content is excluded\n")
        (self.repo / "untracked.txt").write_text("never copied\n")
        source_before = self.metadata()
        result = self.call(*self.register_args())
        self.assertEqual(result["registration"]["base_commit"], self.base)
        self.assertTrue(result["observations"]["dirty"])
        self.assertEqual(result["observations"]["complete_checkout_availability"], "unestablished")
        after = snapshot(self.workspace)
        self.assertEqual(set(after) - set(before), {"state/repository.json"} | ({"state/.workspace.lock"} - set(before)))
        self.assertEqual({key: after[key] for key in before}, before)
        with contextlib.chdir(self.workspace), mock.patch.object(repositories, "_observe", side_effect=AssertionError("Unexpected Git")), \
                mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("Unexpected lock")):
            inspected = self.call("repository", "inspect", "--json")
        self.assertEqual(inspected["registration"], result["registration"])
        verified = self.call("repository", "inspect", self.workspace, "--verify", "--json")
        self.assertEqual(verified["revision"], result["revision"])
        self.assertTrue(verified["observations"]["available"])
        self.assertEqual(self.metadata(), source_before)
        self.assertEqual(snapshot(self.workspace), after)
        self.assertEqual(result["authorization"], "none")
        self.assertFalse(result["dispatchable"])

    def test_replay_survives_deleted_source_and_corrupt_mutable_workspace(self):
        result = self.call(*self.register_args())
        shutil.rmtree(self.repo)
        for name in ("coven.json", "state/tasks.json", "state/agents.json"):
            (self.workspace / name).write_bytes(b"corrupt\xff")
        before = snapshot(self.workspace)
        replay = self.call(*self.register_args())
        self.assertTrue(replay["replayed"])
        self.assertEqual(replay["registration"], result["registration"])
        self.assertEqual(replay["observations"], {"status": "not_assessed"})
        self.assertEqual(self.call("repository", "inspect", self.workspace, "--json")["revision"], result["revision"])
        unavailable = self.call("repository", "inspect", self.workspace, "--verify", "--json", expected=1)
        self.assertEqual(unavailable["registration"], result["registration"])
        self.assertEqual(unavailable["revision"], result["revision"])
        self.assertEqual(snapshot(self.workspace), before)

    def test_unavailable_registration_does_not_gate_task_operations_or_repair_itself(self):
        authority = self.workspace / "state/repository.json"
        authority.write_bytes(b"null")
        before = snapshot(self.workspace)
        invalid = self.call("repository", "inspect", self.workspace, "--json", expected=1)
        self.assertIsNone(invalid["revision"])
        self.assertEqual(invalid["status"], "unavailable")
        self.assertNotIn("registration", invalid)
        self.invoke("task", "list", self.workspace, "--json")
        self.assertEqual(snapshot(self.workspace), before)
        self.invoke(*self.register_args(), error="cannot read repository registration authority")
        self.assertEqual(snapshot(self.workspace), before)
        self.invoke("task", "add", self.workspace, "Unrelated ordinary task")
        self.assertEqual(authority.read_bytes(), b"null")

    def test_uncertain_publication_and_exact_retry_exit_codes(self):
        original = storage.atomic_replace_bytes
        def uncertain(path, data):
            original(path, data)
            raise storage.CommitUncertain(path)
        with mock.patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            published = self.call(*self.register_args(), expected=2)
        with mock.patch.object(storage, "workspace_lock", side_effect=storage.LockBusy("injected busy")):
            uncertain_retry = self.call(*self.register_args(), expected=2)
        self.assertEqual(uncertain_retry["registration"], published["registration"])
        self.assertTrue(uncertain_retry["replayed"])
        confirmed = self.call(*self.register_args())
        self.assertEqual(confirmed["revision"], published["revision"])
        self.assertEqual(confirmed["delivery"]["outcome"], "durably_committed")
        self.invoke(*self.register_args(target="refs/heads/other"), error="immutable")

    def test_deterministic_rejection_does_not_create_lock_or_registration(self):
        before = snapshot(self.workspace)
        bad = self.register_args()
        bad[bad.index("--base") + 1] = "HEAD"
        self.invoke(*bad, error="full lowercase commit")
        stale = self.register_args()
        stale[stale.index("--expect-repository-revision") + 1] = "repository-sha256:" + "0" * 64
        with mock.patch.object(repositories, "_observe", side_effect=AssertionError("Stale request probes Git")):
            self.invoke(*stale, error="revision conflict")
        self.assertEqual(snapshot(self.workspace), before)

    def test_existing_authorities_preparations_and_artifacts_are_preserved(self):
        # Each stage independently registers in its own workspace. No schema
        # upgrade, task import, or preparation binding is implied by registration.
        for mode in ("snapshot", "ledger", "runtime", "execution-v1", "execution-v2"):
            with self.subTest(mode=mode):
                workspace = self.root / mode
                self.invoke("init", workspace)
                task_revision = lambda: json.loads(self.invoke("task", "list", workspace, "--json"))["revision"]
                if mode == "ledger":
                    self.invoke("state", "import-tasks", workspace, "--expect-revision", task_revision())
                if mode == "runtime":
                    self.invoke("action", "enable-runtime-launch", workspace)
                    self.invoke("action", "prepare-runtime-launch", workspace)
                if mode.startswith("execution"):
                    self.invoke("execution", "enable", workspace, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable")
                    revision = lambda: task_attempts.Store(workspace).read().revision
                    self.invoke("execution", "enroll", workspace, "core-chatbot", "--expect-execution-revision", revision(),
                        "--expect-tasks-revision", task_revision(), "--request-id", "enroll")
                    if mode.endswith("v2"):
                        self.invoke("execution", "upgrade", workspace, "--expect-execution-revision", revision(), "--request-id", "upgrade")
                        document = self.root / "graph.json"
                        document.write_text(json.dumps({"schema_version": 1, "kind": "task-graph", "nodes": [{
                            "task_id": "core-chatbot", "owner": "jamal", "depends_on": [],
                            "acceptance_criteria": [{"id": "criteria", "text": "Reviewed source"}]}]}))
                        graph = json.loads(self.invoke("task", "graph", "propose", workspace, "--document", document,
                            "--expect-execution-revision", revision(), "--expect-tasks-revision", task_revision(), "--request-id", "propose", "--json"))
                        self.invoke("task", "graph", "decide", workspace, graph["result"]["graph_revision"], "--decision", "approve",
                            "--reason", "Fixture review", "--expect-graph-digest", graph["result"]["graph_digest"],
                            "--expect-execution-revision", revision(), "--expect-tasks-revision", task_revision(), "--request-id", "review")
                        selection = ["--graph-revision", graph["result"]["graph_revision"]]
                    else:
                        intent = self.root / "intent.json"
                        intent.write_text(json.dumps({"schema_version": 1, "kind": "task-intent", "task_id": "core-chatbot", "owner": "jamal",
                            "acceptance_criteria": [{"id": "criteria", "text": "Reviewed source"}]}))
                        selection = ["--intent", intent]
                    self.invoke("task", "attempt", "prepare", workspace, "core-chatbot", *selection,
                        "--expect-execution-revision", revision(), "--expect-tasks-revision", task_revision(), "--request-id", "prepare")
                (workspace / "work/user.txt").write_text("do not copy or change\n")
                before = snapshot(workspace)
                self.call(*self.register_args(workspace))
                after = snapshot(workspace)
                self.assertEqual({key: after[key] for key in before}, before)
                self.assertEqual(set(after) - set(before), {"state/repository.json"} | ({"state/.workspace.lock"} - set(before)))
                if mode.startswith("execution"):
                    inspected = coven.execution_inspection(workspace)
                    self.assertFalse(inspected["dispatchable"])
                    self.assertEqual(inspected["authorization"], "none")
                    attempt = next(iter(task_attempts.Store(workspace).read().attempts.values()))
                    self.assertNotIn("repository", attempt["prepared"])

    def test_corrupt_registration_does_not_gate_cancellation_or_old_request_replay(self):
        self.invoke("execution", "enable", self.workspace, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable")
        enable = ["execution", "enable", str(self.workspace), "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable", "--json"]
        original = json.loads(self.invoke(*enable))["result"]
        revision = lambda: task_attempts.Store(self.workspace).read().revision
        task_revision = json.loads(self.invoke("task", "list", self.workspace, "--json"))["revision"]
        self.invoke("execution", "enroll", self.workspace, "core-chatbot", "--expect-execution-revision", revision(),
            "--expect-tasks-revision", task_revision, "--request-id", "enroll")
        intent = self.root / "cancel-intent.json"
        intent.write_text(json.dumps({"schema_version": 1, "kind": "task-intent", "task_id": "core-chatbot", "owner": "jamal",
            "acceptance_criteria": [{"id": "criteria", "text": "Reviewed source"}]}))
        prepare_args = ["task", "attempt", "prepare", str(self.workspace), "core-chatbot", "--intent", str(intent),
            "--expect-execution-revision", revision(), "--expect-tasks-revision", task_revision, "--request-id", "prepare", "--json"]
        prepared = json.loads(self.invoke(*prepare_args))
        (self.workspace / "state/repository.json").write_bytes(b"false")
        self.assertEqual(json.loads(self.invoke(*enable))["result"], original)
        self.assertEqual(json.loads(self.invoke(*prepare_args))["result"], prepared["result"])
        self.invoke("task", "attempt", "cancel", self.workspace, prepared["result"]["attempt_id"], "--reason", "Explicit local cancellation",
            "--expect-execution-revision", revision(), "--request-id", "cancel")
        attempt = task_attempts.Store(self.workspace).read().attempts[prepared["result"]["attempt_id"]]
        self.assertEqual(attempt["state"], "cancelled_before_submission")
        self.assertEqual(self.call("repository", "inspect", self.workspace, "--json", expected=1)["status"], "unavailable")
        self.assertEqual((self.workspace / "state/repository.json").read_bytes(), b"false")

    def test_real_wrapper_register_and_inspect_are_standalone_compatible(self):
        env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}
        source_before = self.metadata()
        registered = subprocess.run([str(ROOT / "bin/coven"), *self.register_args()], cwd=ROOT,
            env=env, text=True, capture_output=True, timeout=45)
        self.assertEqual(registered.returncode, 0, registered.stderr)
        result = json.loads(registered.stdout)
        inspected = subprocess.run([str(ROOT / "bin/coven"), "repository", "inspect", "--json"], cwd=self.workspace,
            env=env, text=True, capture_output=True, timeout=10)
        self.assertEqual(inspected.returncode, 0, inspected.stderr)
        self.assertEqual(json.loads(inspected.stdout)["revision"], result["revision"])
        self.assertEqual(self.metadata(), source_before)

    def test_independent_process_registrations_have_one_fact(self):
        worker = "from runner import coven; import sys; raise SystemExit(coven.main(sys.argv[1:]))"
        for identical in (False, True):
            with self.subTest(identical=identical):
                workspace = self.root / ("identical" if identical else "conflicting")
                self.invoke("init", workspace)
                children = [subprocess.Popen([sys.executable, "-c", worker, *self.register_args(workspace, target=target)],
                    cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for target in
                    ("refs/heads/one", "refs/heads/one" if identical else "refs/heads/two")]
                try:
                    responses = [child.communicate(timeout=45) for child in children]
                    self.assertEqual(sorted(child.returncode for child in children), [0, 0] if identical else [0, 1], responses)
                    results = [json.loads(response[0]) for child, response in zip(children, responses) if child.returncode == 0]
                    observed = self.call("repository", "inspect", workspace, "--json")
                    self.assertTrue(all(item["revision"] == observed["revision"] for item in results))
                    if identical:
                        self.assertEqual(sorted(item["replayed"] for item in results), [False, True])
                finally:
                    for child in children:
                        if child.poll() is None:
                            child.kill()
                        child.communicate(timeout=10)

    def test_process_death_before_and_after_publication_never_promotes_temp(self):
        worker = r'''
import sys
from runner import coven,storage
phase=sys.argv[1]
original=storage.os.replace
def pause(source,target):
    if str(target).endswith('/repository.json'):
        if phase=='before':
            print('paused',flush=True)
            sys.stdin.readline()
        original(source,target)
        if phase=='after':
            print('paused',flush=True)
            sys.stdin.readline()
    else: original(source,target)
storage.os.replace=pause
raise SystemExit(coven.main(sys.argv[2:]))
'''
        for phase in ("before", "after"):
            with self.subTest(phase=phase):
                workspace = self.root / ("crash-" + phase)
                self.invoke("init", workspace)
                child = subprocess.Popen([sys.executable, "-c", worker, phase, *self.register_args(workspace)],
                    cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    self.assertTrue(select.select([child.stdout], [], [], 40)[0])
                    self.assertEqual(child.stdout.readline().strip(), "paused")
                    child.kill()
                    child.communicate(timeout=10)
                    inspected = self.call("repository", "inspect", workspace, "--json")
                    self.assertEqual(inspected["status"], "unregistered" if phase == "before" else "registered")
                    if phase == "before":
                        self.assertTrue(list((workspace / "state").glob(".repository.json.*.tmp")))
                    result = self.call(*self.register_args(workspace))
                    self.assertEqual(result["replayed"], phase == "after")
                    if phase == "after":
                        self.assertEqual(result["revision"], inspected["revision"])
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.communicate(timeout=10)
