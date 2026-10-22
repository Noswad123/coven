"""Reviewed runtime attempts, activation fencing, and frozen transport boundaries.

Every runtime executor/transport is fake. Subprocesses only exercise Coven and
local POSIX coordination; they do not start tmux, Herdr, or provider sessions.
"""

import contextlib
import io
import json
import os
from pathlib import Path
import select
import subprocess
import sys
from unittest import mock

from runner import actions, coven, launch, storage
from test_coven import CovenTestCase, ROOT, snapshot
from test_launch import FakeRunner


REAL_EXECUTE = launch.execute
REAL_SEND = coven.send_to_agent_pane


def completed():
    return {"operation": "runtime_launch", "status": "transport_completed", "completed_steps": 1,
            "handles": {}, "diagnostics": [], "outcome_unknown": False}


class ActionCommandTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "architect,implementer", "--multiplexer", "tmux")
        sender = mock.patch.object(coven, "send_to_agent_pane", return_value=False)
        self.sender = sender.start()
        self.addCleanup(sender.stop)
        guard = mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("Unexpected native runtime"))
        self.native = guard.start()
        self.addCleanup(guard.stop)

    def enable(self, workspace=None):
        return json.loads(self.invoke("action", "enable-runtime-launch", workspace or self.workspace, "--json"))

    def reviewed(self):
        prepared = json.loads(self.invoke("action", "prepare-runtime-launch", self.workspace, "--json"))
        approval = json.loads(self.invoke("approve", self.workspace, prepared["checkpoint"], "--expect-revision", prepared["checkpoint_revision"], "--json"))
        prepared.update(expect_checkpoint=approval["after_revision"], expect_decision=approval["decision_id"])
        return prepared

    def submit_args(self, prepared):
        return ["action", "submit", str(self.workspace), prepared["action_id"],
                "--expect-manifest", prepared["manifest_digest"], "--expect-checkpoint", prepared["expect_checkpoint"],
                "--expect-decision", prepared["expect_decision"], "--json"]

    def submit(self, prepared, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(self.submit_args(prepared)), expected)
        return json.loads(output.getvalue())

    def spawn(self, code, *args):
        process = subprocess.Popen([sys.executable, "-c", code, *map(str, args)], cwd=ROOT,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        def cleanup():
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=10)
        self.addCleanup(cleanup)
        return process

    def wait(self, child, line):
        self.assertTrue(select.select([child.stdout], [], [], 15)[0], "child did not signal")
        observed = child.stdout.readline().strip()
        if observed != line:
            self.fail(f"expected {line!r}, got {observed!r}; child={child.communicate(timeout=10)}")

    def test_opt_in_workflow_both_frozen_executors_and_consumed_replay(self):
        for backend in ("tmux", "herdr"):
            with self.subTest(backend=backend), mock.patch.dict(os.environ, {"COVEN_MULTIPLEXER": backend}):
                control = self.enable()["control"]
                prepared = self.reviewed()
                before = snapshot(self.workspace)
                inspected = json.loads(self.invoke("action", "inspect", self.workspace, prepared["action_id"], "--json"))
                self.assertEqual(snapshot(self.workspace), before)
                self.assertEqual(inspected["manifest_digest"], prepared["manifest_digest"])
                self.assertIn("unrestricted COLD runtime launch", inspected["checkpoint"]["content"])
                fake = FakeRunner(backend)
                with mock.patch.object(launch, "execute", side_effect=lambda plan: REAL_EXECUTE(plan, fake, lambda _: None)) as execute:
                    first = self.submit(prepared)
                    second = self.submit(prepared)
                self.assertEqual(first["outcome"], "transport_completed")
                self.assertTrue(second["replayed"])
                self.assertEqual(first["claim"]["claim_id"], second["claim"]["claim_id"])
                self.assertEqual(execute.call_count, 1)
                self.assertIn(control["workspace_id"], first["claim"]["plan"]["namespace"])
        self.native.assert_not_called()

    def test_active_legacy_aliases_and_shared_helpers_fail_before_native_effects(self):
        cached = coven.load_team(self.workspace)
        self.enable()
        before = snapshot(self.workspace)
        for args in (("up", self.workspace), ("start", self.workspace), ("up", self.workspace, "--replace"),
                     ("restart", self.workspace), ("agent", "start", self.workspace, "architect"),
                     ("agent", "restart", self.workspace, "architect"), ("agent", "stop", self.workspace, "architect"),
                     ("bootstrap", self.workspace), ("send", self.workspace, "architect", "Hello")):
            self.invoke(*args, error="Approved runtime-launch mode")
            self.assertEqual(snapshot(self.workspace), before)
        helpers = (
            lambda: coven.start_agent_window(self.workspace, cached, "architect"),
            lambda: coven.create_coven_tmux_session(self.workspace, cached),
            lambda: coven.create_coven_herdr_workspace(self.workspace, cached),
            lambda: coven.run_coven_tmux(self.workspace, cached, "restart"),
            lambda: coven.run_coven_herdr(self.workspace, cached, "restart"),
            lambda: REAL_SEND(cached, "architect", "Cached configuration is not authority"),
        )
        for helper in helpers:
            with self.assertRaises(actions.ActionError):
                helper()
        self.native.assert_not_called()

    def test_changed_frozen_inputs_never_reach_executor(self):
        self.enable()
        prepared = self.reviewed()
        paths = ("goal.md", "coven.json", "coven.toml", "state/agents.json", "agents/architect.md", "prompts/architect.md", "prompts/architect.start.txt")
        for relative in paths:
            path = self.workspace / relative
            original = path.read_bytes()
            if relative == "coven.json":
                value = json.loads(original)
                value["agents"].reverse()
                changed = json.dumps(value).encode()
            elif relative == "state/agents.json":
                value = json.loads(original)
                value[0]["role"] = "Different role"
                changed = json.dumps(value).encode()
            elif relative == "coven.toml":
                changed = original.replace(b'opencode_command = "oc"', b'opencode_command = "another-command"')
            else:
                changed = original + b"\nChanged input\n"
            with self.subTest(relative=relative), mock.patch.object(launch, "execute") as execute:
                path.write_bytes(changed)
                self.invoke(*self.submit_args(prepared), error="inputs")
                execute.assert_not_called()
                self.assertIsNone(actions.ActionStore(self.workspace).get_claim(prepared["action_id"]))
            path.write_bytes(original)
        for change in ({"COVEN_MULTIPLEXER": "herdr"}, {"PATH": os.environ.get("PATH", "") + ":/changed-input"}):
            with self.subTest(environment=change), mock.patch.dict(os.environ, change), mock.patch.object(launch, "execute") as execute:
                self.invoke(*self.submit_args(prepared), error="inputs")
                execute.assert_not_called()
        self.invoke("task", "add", self.workspace, "Changed task input")
        with mock.patch.object(launch, "execute") as execute:
            self.invoke(*self.submit_args(prepared), error="inputs")
            execute.assert_not_called()

    def test_revoked_or_wrong_exact_approval_and_manifest_refs_are_not_authorization(self):
        self.enable()
        prepared = self.reviewed()
        self.invoke("suggest", self.workspace, prepared["checkpoint"], "Do not launch")
        with mock.patch.object(launch, "execute") as execute:
            self.invoke(*self.submit_args(prepared), error="exact bound approval")
            execute.assert_not_called()
        view = coven.checkpoint_view(self.workspace, prepared["checkpoint"])
        approved = json.loads(self.invoke("approve", self.workspace, prepared["checkpoint"], "--expect-revision", view.revision, "--json"))
        prepared.update(expect_checkpoint=approved["after_revision"], expect_decision=approved["decision_id"])
        args = self.submit_args(prepared)
        args[args.index("--expect-manifest") + 1] = "runtime-launch-manifest-v1:" + "0" * 64
        with mock.patch.object(launch, "execute") as execute:
            self.invoke(*args, error="manifest digest mismatch")
            execute.assert_not_called()

    def test_after_claim_changes_and_revocation_do_not_reconstruct_frozen_plan(self):
        self.enable()
        prepared = self.reviewed()
        reviewed = actions.ActionStore(self.workspace).read_bundle(prepared["action_id"])["plan"]
        calls = FakeRunner("tmux")
        def execute(plan):
            with storage.workspace_lock(self.workspace, coven.managed_path(self.workspace, "state/.workspace.lock"), timeout=0):
                pass
            (self.workspace / "prompts/architect.start.txt").write_text("UNREVIEWED STARTER")
            os.environ["COVEN_MULTIPLEXER"] = "herdr"
            coven.suggest_checkpoint_changes(self.workspace, coven.load_team(self.workspace), prepared["checkpoint"], "Revoked after claim")
            self.assertEqual(plan, reviewed)
            return REAL_EXECUTE(plan, calls, lambda _: None)
        with mock.patch.dict(os.environ, {}), mock.patch.object(launch, "execute", side_effect=execute):
            result = self.submit(prepared)
        self.assertEqual(result["outcome"], "transport_completed")
        self.assertTrue(all(argv[0] == "tmux" for argv, _ in calls.calls))
        self.assertFalse(any("UNREVIEWED STARTER" in argv for argv, _ in calls.calls))
        with mock.patch.object(launch, "execute") as again:
            self.assertTrue(self.submit(prepared)["replayed"])
            again.assert_not_called()

    def test_two_submitters_consume_once_and_only_one_executor_runs(self):
        self.enable()
        prepared = self.reviewed()
        counter = self.root / "executor-calls"
        code = r"""
import json, sys
from pathlib import Path
from runner import coven, launch, storage
counter = Path(sys.argv[1])
def execute(plan):
    root = Path(plan['root'])
    with storage.workspace_lock(root, root/'state/.workspace.lock', timeout=0):
        with counter.open('a') as stream: stream.write('attempt\n')
    return {'operation':'runtime_launch','status':'transport_completed','completed_steps':1,'handles':{},'diagnostics':[],'outcome_unknown':False}
launch.execute=execute
print('ready',flush=True)
raise SystemExit(coven.main(sys.argv[2:]))
"""
        with coven.workspace_mutation(self.workspace):
            children = [self.spawn(code, counter, *self.submit_args(prepared)) for _ in range(2)]
            for child in children:
                self.wait(child, "ready")
        values = []
        for child in children:
            out, err = child.communicate(timeout=20)
            self.assertIn(child.returncode, (0, 2), err)
            values.append(json.loads(out))
        self.assertEqual(counter.read_text(), "attempt\n")
        self.assertEqual(len({value["claim"]["claim_id"] for value in values}), 1)
        self.assertEqual(sorted(value["replayed"] for value in values), [False, True])

    def test_uncertain_claim_never_executes_even_on_repeat(self):
        self.enable()
        prepared = self.reviewed()
        control_path = (self.workspace / actions.CONTROL).resolve()
        original = storage.sync_directory
        def fail_claim_sync(path):
            if path == control_path.parent and json.loads(control_path.read_bytes())["claims"]:
                raise OSError("claim directory sync failed")
            original(path)
        with mock.patch.object(storage, "sync_directory", side_effect=fail_claim_sync), mock.patch.object(launch, "execute") as execute:
            result = self.submit(prepared, expected=2)
            self.assertEqual(result["outcome"], "durability_uncertain")
            execute.assert_not_called()
        with mock.patch.object(launch, "execute") as execute:
            replay = self.submit(prepared, expected=2)
            self.assertTrue(replay["replayed"])
            self.assertEqual(replay["outcome"], "claimed")
            execute.assert_not_called()

    def test_failure_before_claim_publication_does_not_consume_or_execute(self):
        self.enable()
        prepared = self.reviewed()
        before = snapshot(self.workspace)
        with mock.patch.object(storage.os, "replace", side_effect=OSError("before claim replace")), mock.patch.object(launch, "execute") as execute:
            self.invoke(*self.submit_args(prepared), error="snapshot was not replaced")
            execute.assert_not_called()
        self.assertIsNone(actions.ActionStore(self.workspace).get_claim(prepared["action_id"]))
        self.assertEqual(snapshot(self.workspace), before)
        with mock.patch.object(launch, "execute", return_value=completed()) as execute:
            self.assertEqual(self.submit(prepared)["outcome"], "transport_completed")
            self.assertEqual(execute.call_count, 1)

    def test_failed_outcome_record_and_crashed_claim_are_never_relaunched(self):
        self.enable()
        prepared = self.reviewed()
        with mock.patch.object(launch, "execute", return_value=completed()) as execute, \
                mock.patch.object(actions.ActionStore, "record_outcome", side_effect=storage.StorageError("report disk failed")):
            value = self.submit(prepared, expected=2)
            self.assertEqual(value["outcome"], "outcome_unknown")
            self.assertEqual(execute.call_count, 1)
        with mock.patch.object(launch, "execute") as execute:
            repeat = self.submit(prepared, expected=2)
            execute.assert_not_called()
        self.invoke("action", "prepare-runtime-launch", self.workspace, error="unresolved")
        ack = json.loads(self.invoke("action", "acknowledge-unknown", self.workspace, prepared["action_id"],
                                     "--expect-claim", repeat["claim"]["claim_id"], "--json"))
        self.assertTrue(ack["claim"]["operator_acknowledged"])
        fresh = self.reviewed()
        self.assertNotEqual(fresh["action_id"], prepared["action_id"])

    def test_process_death_after_claim_leaves_consumed_unknown_attempt(self):
        self.enable()
        prepared = self.reviewed()
        code = """
import sys
from runner import coven, launch
def paused(plan):
    print('claimed',flush=True)
    sys.stdin.readline()
launch.execute=paused
raise SystemExit(coven.main(sys.argv[1:]))
"""
        child = self.spawn(code, *self.submit_args(prepared))
        self.wait(child, "claimed")
        child.kill()
        child.communicate(timeout=10)
        with mock.patch.object(launch, "execute") as execute:
            self.assertEqual(self.submit(prepared, expected=2)["outcome"], "claimed")
            execute.assert_not_called()

    def test_stop_uses_frozen_claim_not_current_config_and_does_not_reset(self):
        self.enable()
        prepared = self.reviewed()
        with mock.patch.object(launch, "execute", return_value=completed()):
            first = self.submit(prepared)
        (self.workspace / "coven.json").write_text("broken current config")
        report = {**completed(), "operation": "runtime_stop", "stop_command_attempted": True, "stop_command_succeeded": True}
        with mock.patch.object(launch, "stop", return_value=report) as stop:
            self.invoke("down", self.workspace)
        self.assertEqual(stop.call_args[0][0], first["claim"]["plan"])
        self.assertEqual(actions.ActionStore(self.workspace).get_claim(prepared["action_id"])["claim_id"], first["claim"]["claim_id"])

    def test_goal_lead_writers_recheck_inputs_under_the_same_claim_lock(self):
        self.enable()
        prepared = self.reviewed()
        self.invoke("goal", self.workspace, "Updated reviewed goal")
        self.invoke("lead", self.workspace, "architect")
        with mock.patch.object(launch, "execute") as execute:
            self.invoke(*self.submit_args(prepared), error="inputs")
            execute.assert_not_called()
        self.native.assert_not_called()

    def test_activation_fences_inflight_legacy_agent_start_startup_bootstrap_and_send(self):
        legacy = r"""
import sys
from pathlib import Path
from types import SimpleNamespace
from runner import coven, actions
w, operation, evidence = Path(sys.argv[1]),sys.argv[2],Path(sys.argv[3])
team=coven.load_team(w)
first=True
def run(argv,**kwargs):
    global first
    if first:
        first=False
        print('admitted',flush=True)
        sys.stdin.readline()
    with evidence.open('a') as stream: stream.write(str((w/actions.CONTROL).exists())+'\n')
    code=1 if operation=='up' and argv[:2]==['tmux','has-session'] else 0
    return SimpleNamespace(returncode=code,stdout='%1\n',stderr='')
coven.subprocess.run=run
if operation=='agent': coven.start_agent_window(w,team,'architect')
elif operation=='up': coven.run_coven_tmux(w,team,'up')
elif operation=='bootstrap': coven.main(['bootstrap',str(w)])
else: coven.send_to_agent_pane(team,'architect','Frozen cached-team delivery')
"""
        activation = """
import contextlib,sys
from runner import coven,storage
original=storage.transport_gate
@contextlib.contextmanager
def announced(*args,**kwargs):
    if kwargs.get('exclusive') and kwargs.get('held') is None: print('enabling',flush=True)
    with original(*args,**kwargs) as gate: yield gate
storage.transport_gate=announced
raise SystemExit(coven.main(['action','enable-runtime-launch',sys.argv[1],'--json']))
"""
        for operation in ("agent", "up", "bootstrap", "send"):
            with self.subTest(operation=operation):
                workspace = self.root / f"race-{operation}"
                self.invoke("init", workspace, "--blank", "--agents", "architect", "--multiplexer", "tmux")
                cached = coven.load_team(workspace)
                evidence = self.root / f"evidence-{operation}"
                old = self.spawn(legacy, workspace, operation, evidence)
                self.wait(old, "admitted")
                enable = self.spawn(activation, workspace)
                self.wait(enable, "enabling")
                self.assertIsNone(actions.ActionStore(workspace).read_control())
                with self.assertRaises(storage.LockBusy), storage.transport_gate(workspace, coven.managed_path(workspace, "state/.runtime-transport.lock"), exclusive=True, timeout=0):
                    pass
                old.stdin.write("continue\n")
                old.stdin.flush()
                _, error = old.communicate(timeout=15)
                self.assertEqual(old.returncode, 0, error)
                _, error = enable.communicate(timeout=15)
                self.assertEqual(enable.returncode, 0, error)
                self.assertTrue(actions.ActionStore(workspace).read_control())
                self.assertTrue(evidence.read_text().splitlines())
                self.assertEqual(set(evidence.read_text().splitlines()), {"False"})
                with self.assertRaises(actions.ActionError):
                    REAL_SEND(cached, "architect", "Cached-before-enable notice cannot bypass gate")
        self.native.assert_not_called()
