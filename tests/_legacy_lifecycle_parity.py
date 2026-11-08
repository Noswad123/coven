"""Legacy CLI acceptance against a std-only, script-inert backend.

The parent is the development Python oracle, never a native child/provider.
Replays use the identical canonical workspace inode. Decision UUIDs/clocks are
pinned at their production sites, so checkpoint bytes, revisions and notices
compare exactly, not through a digest/field mask. Only history event_id/ts use
the existing schema-specific volatility validator. This does NOT exercise the
delayed shell senders: those outlive legacy admission by compatibility design.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from unittest.mock import patch

from _runtime_launch_parity import (
    Workspace as LaunchWorkspace, Suite as LaunchSuite, UNICODE,
    coven, checkpoints, actions, environment, equal, files, restore, history,
    inherited_descriptors,
)

REQUIRED = {"standup", "console", "lead", "bootstrap", "agent status", "agent start",
            "agent stop", "agent restart", "plan", "up", "start", "restart"}


class Suite(LaunchSuite):
    def __init__(self, root, binary, fixture):
        self.root, self.binary = root, binary
        self.bin = root / "native only 'quoted bin"
        self.bin.mkdir()
        for name in ("tmux", "herdr"):
            shutil.copyfile(fixture, self.bin / name)
            (self.bin / name).chmod(0o755)
        (self.bin / "coven").symlink_to(binary)
        assert sorted(p.name for p in self.bin.iterdir()) == ["coven", "herdr", "tmux"]
        self.calls, self.routes, self.cases = 0, Counter(), Counter()
        self.inherited_fd_cli_calls, self.attempted_cases = 0, 0
        self.failures = []
        self.oracle_calls = 0
        self.workspaces = []

    def workspace(self, name, backend="tmux"):
        workspace = Workspace(self, name, backend)
        self.workspaces.append(workspace)
        return workspace


class Workspace(LaunchWorkspace):
    def __init__(self, suite, name, backend):
        self.suite, self.backend = suite, backend
        self.parity_failures = []
        self.root, self.home = suite.root / name, suite.root / (name + "-home")
        self.root.mkdir()
        self.home.mkdir()
        self.caller = suite.root / (name + "-caller")
        self.caller.mkdir()
        self.control()
        self.env = dict(PATH=str(suite.bin), HOME=str(self.home), SHELL="/bin/sh",
                        LANG="C.UTF-8", LC_ALL="C.UTF-8", TERM="xterm-256color",
                        COVEN_COMMAND=str(suite.bin / "coven"), COVEN_MULTIPLEXER=backend,
                        COVEN_LEGACY_SENTINEL="all caller environment must survive")
        self.run("init", str(self.root), "--name", "Legacy inert", "--agents", "Clippy,Eric",
                 "--multiplexer", backend, "--opencode-command", "inert-provider 'literal ; $(touch forbidden)'", "--blank")
        assert self.records() == []
        (self.root / "prompts/clippy.start.txt").write_text(UNICODE)

    def control(self, **values):
        super().control(**values)
        (self.home / "legacy-failure-counter").unlink(missing_ok=True)

    def argv(self, route, *args):
        words = route.split()
        return (*words, str(self.root), *args)

    def contract(self, rows, *, null=True, tty=None):
        for row in rows:
            assert row["cwd"] == str(self.caller), row
            equal(row["env"], self.env, "ALL legacy environment")
            assert row["inherited_fds"] == [], row
            assert row["workspace_lock_present"] and not row["workspace_locked"], row
            if null:
                assert row["stdin_null"], row
            if tty is not None:
                equal(row["ttys"], tty, "actual backend TTY mask")

    def parity(self, route, *args, input=b"", code=0):
        # Collect independent comparison failures so a presentation regression
        # cannot hide later lifecycle routes. The enclosing case still FAILS.
        import traceback
        try:
            return self._parity(route, *args, input=input, code=code)
        except AssertionError:
            self.parity_failures.append((route, traceback.format_exc()))
            return self.last_output

    def assert_parity(self):
        assert not self.parity_failures, self.parity_failures

    def _parity(self, route, *args, input=b"", code=0):
        """Compare actual calls, topology, complete workspace, stdout and status."""
        before, backend_before = files(self.root), self.backend_files()
        start = len(self.records())
        argv = self.argv(route, *args)
        out, err = self.run(*argv, input=input if input else None, code=code)
        self.last_output = out
        native, backend_after = files(self.root), self.backend_files()
        native_rows = self.records()
        self.contract(native_rows[start:])
        new_events = [json.loads(line) for line in native.get("logs/events.jsonl", b"").splitlines()][
            len(before.get("logs/events.jsonl", b"").splitlines()):]
        events = iter(new_events)
        messages = iter([json.loads(line) for line in native.get("logs/messages.jsonl", b"").splitlines()][
            len(before.get("logs/messages.jsonl", b"").splitlines()):])
        decisions = {}
        for name, raw in native.items():
            if name.startswith("checkpoints/") and before.get(name) != raw:
                view = checkpoints.inspect(Path(name).stem, raw)
                if view.decision:
                    decisions[Path(name).stem] = view.decision
        lead = json.loads(native["state/lead.json"]) if "state/lead.json" in native else None
        # A console can set, show and clear in ONE invocation. Its final lead is
        # null, but the exact prior metadata is retained in lead.cleared history.
        if lead is None:
            lead = next((row["previous"] for row in new_events
                         if row["type"] == "lead.cleared" and row.get("previous")), None)
        original_event, original_message = coven.append_event, coven.append_message
        original_decide = coven.decide_checkpoint

        def event(workspace, kind, **kw):
            row = next(events)
            assert row["type"] == kind, (kind, row)
            with patch.object(coven, "now", return_value=row["ts"]):
                return original_event(workspace, kind, **kw)

        def message(*a, **kw):
            row = next(messages)
            with patch.object(coven, "now", return_value=row["ts"]):
                return original_message(*a, **kw)

        def decide(workspace, team, checkpoint, *a, **kw):
            d = decisions[checkpoint]
            assert uuid.UUID(hex=d["decision_id"]).version == 4
            with patch.object(coven.uuid, "uuid4", return_value=uuid.UUID(hex=d["decision_id"])), \
                    patch.object(coven, "now", return_value=d["decided_at"]):
                return original_decide(workspace, team, checkpoint, *a, **kw)

        restore(self.root, before)
        self.restore_backend(backend_before)
        captured, errors = io.StringIO(), io.StringIO()
        try:
            with ExitStack() as stack:
                stack.enter_context(environment(self.env, self.caller))
                stack.enter_context(redirect_stdout(captured))
                stack.enter_context(redirect_stderr(errors))
                stack.enter_context(patch.object(sys, "stdin", io.StringIO(input.decode())))
                stack.enter_context(patch.object(coven, "append_event", side_effect=event))
                stack.enter_context(patch.object(coven, "append_message", side_effect=message))
                stack.enter_context(patch.object(coven, "decide_checkpoint", side_effect=decide))
                if lead and any(row["type"] == "lead.set" for row in new_events):
                    stack.enter_context(patch.object(coven, "now", return_value=lead["set_at"]))
                status = coven.main(list(argv))
            self.suite.oracle_calls += 1
            comparisons = []

            def compare(actual, expected, label):
                try:
                    if isinstance(actual, bytes):
                        assert actual == expected, (label, actual[:500], expected[:500])
                    elif isinstance(actual, str) and isinstance(expected, str):
                        if actual != expected:
                            index = next((i for i, (a, b) in enumerate(zip(actual, expected))
                                          if a != b), min(len(actual), len(expected)))
                            raise AssertionError((label, "first differing character", index,
                                "oracle", actual[max(0, index-50):index+100],
                                "native", expected[max(0, index-50):index+100],
                                "lengths", len(actual), len(expected)))
                    else:
                        equal(actual, expected, label)
                except AssertionError as error:
                    comparisons.append(str(error))

            compare(status, code, "oracle exit status")
            compare(captured.getvalue(), out.decode(), "exact human output " + route)
            compare(errors.getvalue(), err.decode(), "exact stderr " + route)
            compare(self.records(), native_rows, "ordered actual transport " + route)
            expected = files(self.root)
            compare(sorted(expected), sorted(native), "workspace inventory " + route)
            for name in native:
                if name in ("logs/events.jsonl", "logs/messages.jsonl"):
                    compare(history(native[name]), history(expected.get(name, b"")), "schema history " + route)
                else:
                    compare(native[name], expected.get(name), "exact workspace bytes " + name)
            actual_backend = self.backend_files()
            assert actual_backend == backend_after, ("complete backend state/handles", route,
                [name for name in actual_backend.keys() | backend_after.keys()
                 if actual_backend.get(name) != backend_after.get(name)])
            assert next(events, None) is None and next(messages, None) is None
            assert not comparisons, comparisons
        finally:
            restore(self.root, native)
            self.restore_backend(backend_after)
        return out

    def reject(self, route, *args):
        before = self.records()
        child = self.start(*self.argv(route, *args))
        out, err = child.communicate(timeout=15)
        assert child.returncode != 0, (route, out, err)
        assert self.records() == before, (route, "effect before rejection")
        return out, err

    def enable_native(self):
        return self.run("action", "enable-runtime-launch", str(self.root), "--json", json_output=True)


def flow(suite, backend, command):
    w = suite.workspace("flow-" + backend + "-" + command, backend)
    if command == "canonical":
        # Python's cli_command defaults to an absolute Python entrypoint; pin
        # BOTH sides explicitly instead of rewriting any captured script text.
        w.env["COVEN_COMMAND"] = "coven"
    w.parity("plan")
    w.parity("up")
    created = w.records()
    scripts = [a for r in created for a in r["argv"] if "while true" in a or "sleep 5" in a]
    assert len(scripts) >= 5 and any("sleep 5" in s for s in scripts), scripts
    assert any("$(touch forbidden)" in s for s in scripts), scripts
    assert not (w.root / "forbidden").exists() and not (w.caller / "forbidden").exists()
    start = len(created)
    w.parity("start")
    assert all(r["argv"][0] == ("has-session" if backend == "tmux" else "workspace")
               for r in w.records()[start:]), "reuse unexpectedly creates topology"
    for route in ("agent stop", "agent start", "agent restart"):
        w.parity(route, "clippy")
    (w.root / "prompts/eric.start.txt").unlink()
    w.parity("bootstrap")
    assert (w.root / "prompts/eric.start.txt").exists()
    w.parity("agent status")
    w.parity("standup")
    w.parity("lead", "set", "clippy", "--message", UNICODE)
    w.parity("lead", "show")
    w.parity("lead", "clear")
    w.parity("console")  # Actual EOF, no pre-scripted quit.
    w.parity("console", input=b"help\nagents\nstandup\nlead clippy\nlead show\nlead clear\nclippy literal shortcut\nsend eric literal send\napprove plan-approval\n\nquit\n")
    # One write per checkpoint permits precise UUID/time pinning, including the
    # revisions printed by nested review and the bytes of literal notices.
    w.parity("console", input=b"approve plan-approval\ny\nsuggest final-review literal notes\nyes\nreview console-review\na\nquit\n")
    w.parity("restart")
    w.parity("up", "--replace")
    w.parity("down")
    assert not (w.home / "legacy-topology").read_bytes()
    w.assert_parity()


def bootstrap_failure(suite, backend, operation):
    w = suite.workspace("bootstrap-fail-" + backend + "-" + operation.replace(" ", "-"), backend)
    w.parity("up")
    w.control(fail=operation)
    start = len(w.records())
    out = w.parity("bootstrap")
    assert b"Bootstrap prompts sent: 2" in out, out
    rows = w.records()[start:]
    deliveries = [r for r in rows if (r["argv"][0] in ("load-buffer", "paste-buffer", "send-keys")
                  if backend == "tmux" else r["argv"][:2] in (["pane", "send-text"], ["pane", "send-keys"]))]
    assert len(deliveries) == (6 if backend == "tmux" else 4), deliveries
    w.assert_parity()
    # This is an attempted-delivery count, NEVER backend acceptance evidence.


def creation_failure(suite, backend, operation, nth=1):
    w = suite.workspace(f"create-fail-{backend}-{operation.replace(' ', '-')}-{nth}", backend)
    w.control(fail=operation, fail_nth=nth)
    child = w.start(*w.argv("up"))
    out, err = child.communicate(timeout=20)
    assert child.returncode != 0, (out, err)
    rows = w.records()
    w.contract(rows)
    assert operation in " ".join(rows[-1]["argv"]), rows
    assert sum(" ".join(r["argv"]).startswith(operation) for r in rows) == nth
    assert not any(r["argv"][0] == "kill-session" or r["argv"][:2] == ["workspace", "close"] for r in rows)
    state = (w.home / "legacy-topology").read_bytes() if (w.home / "legacy-topology").exists() else b""
    if "split" in operation:
        assert state, "partial topology was rolled back"


def unchecked_run(suite):
    w = suite.workspace("unchecked-herdr-run", "herdr")
    w.control(fail="pane run")
    w.parity("up")
    assert sum(r["argv"][:2] == ["pane", "run"] for r in w.records()) == 6
    w.parity("agent restart", "clippy")
    w.parity("agent status")
    w.assert_parity()


def agent_start_failure(suite, backend, nth):
    w = suite.workspace(f"agent-start-fail-{backend}-{nth}", backend)
    w.parity("up")
    w.parity("agent stop", "clippy", "eric")
    operation = "new-window" if backend == "tmux" else "tab create"
    w.control(fail=operation, fail_nth=nth)
    start = len(w.records())
    child = w.start(*w.argv("agent start", "clippy", "eric"))
    out, err = child.communicate(timeout=15)
    assert child.returncode != 0, (out, err)
    rows = w.records()[start:]
    w.contract(rows)
    assert " ".join(rows[-1]["argv"]).startswith(operation), rows
    assert sum(" ".join(r["argv"]).startswith(operation) for r in rows) == nth
    assert not any(r["argv"][0] == "kill-window" or r["argv"][:2] == ["tab", "close"] for r in rows)
    w.control()
    status = w.parity("agent status")
    assert b"Eric (eric): " + backend.encode() + b"=stopped" in status, status
    assert (b"Clippy (clippy): " + backend.encode() + b"=running" in status) == (nth == 2), status
    w.assert_parity()


def invalid_batches(suite, backend):
    w = suite.workspace("invalid-batches-" + backend, backend)
    w.parity("plan")
    for route in ("agent start", "agent stop", "agent restart", "agent status"):
        snapshot = files(w.root)
        w.reject(route, "clippy", "missing-late-agent")
        assert files(w.root) == snapshot, route
    start = len(w.records())
    child = w.start(*w.argv("agent start", "clippy"))
    out, err = child.communicate(timeout=15)
    assert child.returncode != 0, (out, err)
    rows = w.records()[start:]
    w.contract(rows)
    assert all(r["argv"][0] in ("list-panes", "has-session") if backend == "tmux"
               else r["argv"][:2] == ["workspace", "list"] for r in rows), rows
    missing = w.root / "prompts/clippy.start.txt"
    missing.unlink()
    (w.root / "prompts/eric.start.txt").write_bytes(b"invalid late UTF-8 \xff")
    w.reject("bootstrap")
    assert not missing.exists(), "earlier missing prompt created before validating the batch"
    assert not (w.home / "legacy-topology").read_bytes(), "missing parent got created"
    w.assert_parity()


def partial_reuse(suite, backend):
    w = suite.workspace("partial-reuse-" + backend, backend)
    w.parity("up")
    w.parity("agent stop", "eric")
    before = (w.home / "legacy-topology").read_bytes()
    w.parity("up")
    assert (w.home / "legacy-topology").read_bytes() == before
    out = w.parity("bootstrap")
    assert b"No running agent pane found for: eric" in out and b"Bootstrap prompts sent: 1" in out
    w.parity("up", "--replace")
    assert b"eric" in w.parity("agent status")
    w.assert_parity()


def authority(suite, backend, malformed):
    w = suite.workspace(f"authority-{backend}-{malformed}", backend)
    w.parity("up")
    w.enable_native()
    if malformed:
        (w.root / actions.CONTROL).write_bytes(b'{"invalid":true}\n')
    before = w.records()
    for route, args in (("plan", ()), ("up", ()), ("restart", ()), ("bootstrap", ()),
                        ("agent start", ("clippy",)), ("agent stop", ("clippy",)),
                        ("agent restart", ("clippy",))):
        w.reject(route, *args)
    if not malformed:
        assert b"unverified" in w.parity("agent status")
        assert b"unverified" in w.parity("standup")
        # Delivery diagnostics intentionally differ by native error vocabulary;
        # metadata must still commit without a backend call.
        w.run(*w.argv("lead", "set", "clippy", "--message", "metadata only"))
        assert json.loads((w.root / "state/lead.json").read_bytes())["agent"] == "clippy"
        w.parity("lead", "show")
        w.parity("lead", "clear")
    else:
        for route in ("agent status", "standup"):
            w.reject(route)
    assert w.records() == before, "authority blocked effects reached backend"
    w.assert_parity()


def drains(suite, backend, route):
    w = suite.workspace("drain-" + backend + "-" + route.replace(" ", "-"), backend)
    w.parity("up")
    if route == "agent start":
        w.parity("agent stop", "clippy")
    elif route == "up":
        w.parity("down")
    barrier = ("load-buffer" if backend == "tmux" else "pane send-text") if route == "bootstrap" else (
        "new-window" if backend == "tmux" and route == "agent start" else
        "new-session" if backend == "tmux" else "tab create" if route == "agent start" else "workspace create")
    w.control(barrier=barrier)
    child = w.start(*w.argv(route, *(["clippy"] if route == "agent start" else [])))
    activation = None
    try:
        w.wait_barrier()
        activation = w.start("action", "enable-runtime-launch", str(w.root), "--json")
        time.sleep(.2)
        assert activation.poll() is None and not (w.root / actions.CONTROL).exists()
        w.run("goal", str(w.root), "workspace writer while activation drains")
        assert child.poll() is None
    finally:
        w.release()
        out, err = child.communicate(timeout=20)
        if activation is not None:
            enabled, errors = activation.communicate(timeout=20)
    assert child.returncode == 0, (out, err)
    assert activation is not None and activation.returncode == 0, (enabled, errors)
    assert json.loads(enabled)["outcome"] == "enabled"
    w.contract(w.records())
    before = w.records()
    w.reject(route, *(["clippy"] if route == "agent start" else []))
    assert w.records() == before
    w.assert_parity()


def descriptor_negative_control(suite):
    w = suite.workspace("fixture-negative-control")
    with inherited_descriptors() as descriptors:
        subprocess.run([str(suite.bin / "tmux"), "has-session", "-t", "absent"],
                       cwd=w.caller, env=w.env, stdin=subprocess.DEVNULL,
                       pass_fds=descriptors, capture_output=True, timeout=10)
    row = w.records()[0]
    assert row["inherited_fds"] == list(descriptors), row
    assert row["stdio_null"] == [True, False, False], row


def console_cas(suite, backend):
    w = suite.workspace("console-cas-" + backend, backend)
    w.parity("up")
    target = w.root / "checkpoints/plan-approval.md"
    before = target.read_bytes()
    revision = checkpoints.inspect("plan-approval", before).revision
    records = w.records()
    child = w.start(*w.argv("console"), input=b"approve plan-approval\n")
    try:
        child.stdin.write(b"approve plan-approval\n")
        child.stdin.flush()
        displayed = b""
        deadline = time.monotonic() + 10
        while b"[y/N] > " not in displayed:
            assert time.monotonic() < deadline, displayed
            if select.select([child.stdout], [], [], .1)[0]:
                chunk = os.read(child.stdout.fileno(), 65536)
                assert chunk, ("console exited before confirmation", displayed)
                displayed += chunk
        assert revision.encode() in displayed, displayed
        # Simulate a concurrent editor AFTER display and BEFORE confirmation.
        changed = before + b"\nExternal editor changed the reviewed scope.\n"
        target.write_bytes(changed)
        out, err = child.communicate(b"yes\nquit\n", timeout=15)
        assert child.returncode == 1, (displayed, out, err)
        assert b"stale" in err.lower() and b"no retry" in err.lower(), err
        assert target.read_bytes() == changed
        assert checkpoints.inspect("plan-approval", changed).revision != revision
        assert w.records() == records, "stale decision notified a runtime"
        w.assert_parity()
    finally:
        if child.poll() is None:
            child.kill()
            child.communicate()


def tty_attach(suite, in_tmux, mask):
    """Real CLI PTY, inert foreground image. No real tmux session is contacted."""
    import pty
    w = suite.workspace(f"tty-{in_tmux}-{mask}")
    w.parity("up")
    if in_tmux:
        w.env["TMUX"] = "fixture-not-a-real-session"
    operation = "switch-client" if in_tmux else "attach-session"
    w.control(barrier=operation)
    master, slave = pty.openpty()
    start = len(w.records())
    activation = None
    try:
        with inherited_descriptors() as descriptors:
            child = subprocess.Popen([suite.binary, *w.argv("up")], cwd=w.caller, env=w.env,
                stdin=slave if mask & 1 else subprocess.DEVNULL,
                stdout=slave if mask & 2 else subprocess.PIPE,
                stderr=slave if mask & 4 else subprocess.PIPE, pass_fds=descriptors)
        suite.calls += 1
        suite.inherited_fd_cli_calls += 1
        suite.routes["up"] += 1
        os.close(slave)
        slave = None
        if mask & 3 == 3:
            w.wait_barrier()
            activation = w.start("action", "enable-runtime-launch", str(w.root), "--json")
            time.sleep(.2)
            assert activation.poll() is None and not (w.root / actions.CONTROL).exists()
            w.run("goal", str(w.root), "writer during foreground attach")
        w.release()
        out, err = child.communicate(timeout=15)
        assert child.returncode == 0, (out, err)
        if activation is not None:
            out, err = activation.communicate(timeout=15)
            assert activation.returncode == 0, (out, err)
        rows = w.records()[start:]
        attaches = [r for r in rows if r["argv"][0] in ("attach-session", "switch-client")]
        assert len(attaches) == (1 if mask & 3 == 3 else 0), rows
        w.contract(rows, null=False)
        if attaches:
            equal(attaches[0]["ttys"], [bool(mask & (1 << fd)) for fd in range(3)], "foreground inherited TTYs")
            assert attaches[0]["argv"][0] == operation
        w.assert_parity()
    finally:
        w.release()
        if "child" in locals() and child.poll() is None:
            child.kill()
            child.communicate()
        if activation is not None and activation.poll() is None:
            activation.kill()
            activation.communicate()
        if slave is not None:
            os.close(slave)
        os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--fixture", required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="coven-legacy-lifecycle-") as temporary:
        suite = Suite(Path(temporary).resolve(), str(Path(args.binary).resolve()), args.fixture)
        suite.case("descriptor-negative-control", lambda: descriptor_negative_control(suite))
        for backend in ("tmux", "herdr"):
            for command in ("absolute", "canonical"):
                suite.case(f"flow-{backend}-{command}", lambda b=backend,c=command: flow(suite,b,c))
            operations = ("load-buffer", "paste-buffer", "send-keys") if backend == "tmux" else ("pane send-text", "pane send-keys")
            for operation in operations:
                suite.case(f"bootstrap-nonzero-{backend}-{operation}", lambda b=backend,o=operation: bootstrap_failure(suite,b,o))
            first = "new-session" if backend == "tmux" else "workspace create"
            split = "split-window" if backend == "tmux" else "pane split"
            for operation, nth in ((first,1), (split,1), (split,3)):
                suite.case(f"required-failure-{backend}-{operation}-{nth}", lambda b=backend,o=operation,n=nth: creation_failure(suite,b,o,n))
            for nth in (1, 2):
                suite.case(f"agent-start-failure-{backend}-{nth}", lambda b=backend,n=nth: agent_start_failure(suite,b,n))
            suite.case("invalid-batches-" + backend, lambda b=backend: invalid_batches(suite,b))
            suite.case("partial-reuse-" + backend, lambda b=backend: partial_reuse(suite,b))
            suite.case("console-display-cas-" + backend, lambda b=backend: console_cas(suite,b))
            for malformed in (False, True):
                suite.case(f"authority-{backend}-{malformed}", lambda b=backend,m=malformed: authority(suite,b,m))
            for route in ("agent start", "bootstrap", "up"):
                suite.case(f"drain-{backend}-{route}", lambda b=backend,r=route: drains(suite,b,r))
        suite.case("unchecked-herdr-run", lambda: unchecked_run(suite))
        for in_tmux in (False, True):
            for mask in range(8):
                suite.case(f"tty-{in_tmux}-{mask}", lambda t=in_tmux,m=mask: tty_attach(suite,t,m))
        missing = REQUIRED - suite.routes.keys()
        print(json.dumps(dict(case_count=sum(suite.cases.values()), attempted_cases=suite.attempted_cases,
            cases=dict(suite.cases),
            failed_cases=len(suite.failures), native_cli_calls=suite.calls,
            backend_calls=sum(len(w.records()) for w in suite.workspaces),
            inherited_fd_cli_calls=suite.inherited_fd_cli_calls, oracle_calls=suite.oracle_calls,
            batch_routes=len(REQUIRED - missing), required_batch_routes=len(REQUIRED),
            missing_batch_routes=sorted(missing), routes=dict(sorted(suite.routes.items())),
            limitations=["Scripts are DATA; delayed senders after creation are NOT fenced by legacy admission",
                         "60s timeout/no-retry and >61s foreground lifetime belong to platform unit tests",
                         "TTY probes are inert, not a claim of real tmux/Herdr/provider operation"])), flush=True)
        if missing:
            suite.failures.append(("missing-routes", repr(sorted(missing))))
        for name, trace in suite.failures:
            print(name + "\n" + trace, file=sys.stderr)
        if suite.failures:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
