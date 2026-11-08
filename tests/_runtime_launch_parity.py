"""Real native CLI, inert compiled transports, and the retained Python oracle.

All native workspaces are initialized by the Cargo binary. Oracle replays happen
in this temporary parent, at the SAME canonical root/inode, with native-generated
UUIDs pinned explicitly. No digest, input byte, namespace, command, or arbitrary
text is normalized. Only history event UUIDs and named clock fields are volatile.
The fixture never interprets shell programs and PATH contains no interpreter.
This is cooperating-process acceptance, not a power-loss/fault-injection claim.
"""
import argparse
from collections import Counter
from contextlib import contextmanager, redirect_stdout
from datetime import datetime, timezone
import fcntl
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import actions, checkpoints, coven, storage

ENV_KEYS = {"PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"}
UNICODE = "literal snow \u96ea; quotes ' and \"; not a shell $(touch forbidden)\nsecond line"


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode()


def equal(actual, expected, label):
    if canonical(actual) == canonical(expected):
        return

    def difference(a, b, path):
        if type(a) is not type(b):
            return path, a, b
        if isinstance(a, dict):
            if a.keys() != b.keys():
                return path + ".keys", sorted(a), sorted(b)
            for key in a:
                if canonical(a[key]) != canonical(b[key]):
                    return difference(a[key], b[key], path + "." + key)
        elif isinstance(a, list):
            if len(a) != len(b):
                return path + ".length", len(a), len(b)
            for index, (left, right) in enumerate(zip(a, b)):
                if canonical(left) != canonical(right):
                    return difference(left, right, f"{path}[{index}]")
        return path, a, b

    raise AssertionError((label, *difference(actual, expected, "$")))


def files(root):
    return {str(p.relative_to(root)): p.read_bytes()
            for p in root.rglob("*") if p.is_file()}


def restore(root, snapshot):
    for path in root.rglob("*"):
        if path.is_file() and str(path.relative_to(root)) not in snapshot:
            path.unlink()
    for name, data in snapshot.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists() or path.read_bytes() != data:
            path.write_bytes(data)


def history(raw):
    rows = []
    for line in raw.splitlines():
        row = json.loads(line)
        # Only these schema fields are volatile. Literal message bodies and
        # decision/claim IDs, revisions and digests remain completely untouched.
        if "event_id" in row:
            parsed = uuid.UUID(row["event_id"])
            assert parsed.version == 4 and str(parsed) == row["event_id"]
            row["event_id"] = "00000000-0000-4000-8000-000000000000"
        if "ts" in row:
            parsed = datetime.fromisoformat(row["ts"])
            assert parsed.tzinfo == timezone.utc
            row["ts"] = "2000-01-01T00:00:00+00:00"
        rows.append(row)
    return rows


@contextmanager
def inherited_descriptors():
    low = os.open(os.devnull, os.O_RDONLY)
    high = None
    try:
        high = fcntl.fcntl(low, fcntl.F_DUPFD, 2048)
        assert 3 <= low < 1024 and 2048 <= high < 4096, (low, high)
        yield low, high
    finally:
        if high is not None:
            os.close(high)
        os.close(low)


@contextmanager
def environment(env, cwd):
    before, directory = dict(os.environ), Path.cwd()
    try:
        os.environ.clear()
        os.environ.update(env)
        os.chdir(cwd)
        yield
    finally:
        os.chdir(directory)
        os.environ.clear()
        os.environ.update(before)


class Suite:
    def __init__(self, root, binary, fixture):
        self.root, self.binary = root, binary
        self.bin = root / "compiled-only-bin"
        self.bin.mkdir()
        for name in ("tmux", "herdr"):
            shutil.copyfile(fixture, self.bin / name)
            (self.bin / name).chmod(0o755)
        # Generated Coven references are pinned, not resolved from an installation.
        (self.bin / "coven").symlink_to(binary)
        assert not any((self.bin / name).exists() for name in ("python", "python3", "oc", "opencode"))
        self.calls, self.routes, self.cases = 0, Counter(), Counter()
        self.inherited_fd_cli_calls = 0
        self.attempted_cases = 0
        self.failures = []
        # Negative control: subprocess pass_fds really reaches the compiled
        # backend, and its fd scan detects BOTH low and high sentinel numbers.
        home = root / "fd-probe-home"
        home.mkdir()
        (home / "runtime-fixture-control").write_text(f"workspace={root}\n")
        with inherited_descriptors() as descriptors:
            probe = subprocess.run([str(self.bin / "tmux"), "list-panes"], cwd=root,
                env={"HOME": str(home)}, stdin=subprocess.DEVNULL, pass_fds=descriptors,
                capture_output=True, timeout=15)
            assert probe.returncode == 0, (probe.stdout, probe.stderr)
            record = json.loads((home / "runtime-fixture-calls.jsonl").read_bytes())
            assert record["inherited_fds"] == list(descriptors), record
            assert record["stdin_null"], record

    def case(self, name, function):
        self.attempted_cases += 1
        try:
            function()
            self.cases[name] += 1
        except Exception as error:
            import traceback
            self.failures.append((name, traceback.format_exc()))
            print(f"FAIL {name}: {error}", flush=True)

    def workspace(self, name, backend="tmux"):
        return Workspace(self, name, backend)


class Workspace:
    def __init__(self, suite, name, backend):
        self.suite, self.backend = suite, backend
        self.root, self.home = suite.root / name, suite.root / (name + "-home")
        self.root.mkdir()
        self.home.mkdir()
        self.caller = self.root
        self.control()
        # No ambient session/provider/socket configuration enters native children.
        self.env = dict(PATH=str(suite.bin), HOME=str(self.home), SHELL="/bin/sh",
                        LANG="C.UTF-8", LC_ALL="C.UTF-8", TERM="xterm-256color",
                        COVEN_COMMAND=str(suite.bin / "coven"), COVEN_MULTIPLEXER=backend)
        self.run("init", str(self.root), "--name", "Inert acceptance", "--agents", "Clippy",
                 "--multiplexer", backend, "--opencode-command", "oc --literal 'not executed'", "--blank")
        assert self.records() == []
        # A non-ASCII literal must survive the captured inventory, plan, argv,
        # persisted claim, inspection, and both JSON/human renderers unchanged.
        (self.root / "prompts/clippy.start.txt").write_text(UNICODE)

    def start(self, *args, input=None):
        self.suite.calls += 1
        route = " ".join(args[:2]) if args[0] in ("action", "agent", "checkpoint") else args[0]
        self.suite.routes[route] += 1
        with inherited_descriptors() as descriptors:
            child = subprocess.Popen([self.suite.binary, *args], cwd=self.caller, env=self.env,
                                     stdin=subprocess.PIPE if input is not None else subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, pass_fds=descriptors)
        self.suite.inherited_fd_cli_calls += 1
        return child

    def run(self, *args, code=0, input=None, json_output=False):
        child = self.start(*args, input=input)
        try:
            out, err = child.communicate(input, timeout=85)
        except BaseException:
            child.kill()
            child.communicate()
            raise
        if child.returncode != code:
            diagnostic = out
            try:
                receipt = json.loads(out)
                if isinstance(receipt, dict) and "claim" in receipt:
                    diagnostic = {"outcome": receipt.get("outcome"), "replayed": receipt.get("replayed"),
                                  "report": receipt["claim"].get("report"), "warnings": receipt.get("warnings")}
            except (ValueError, AttributeError):
                pass
            raise AssertionError((args, child.returncode, diagnostic, err))
        if code == 0:
            assert not err, (args, err)
        if json_output:
            assert out.endswith(b"\n") and not out.endswith(b"\n\n"), (args, out)
            return json.loads(out)
        return out, err

    def action(self, name, *args, code=0, pretty=False):
        workspace = [str(self.root)] if self.caller != self.root else []
        return self.run("action", name, *workspace, *args, *([] if pretty else ["--json"]),
                        code=code, json_output=True)

    def control(self, **values):
        values = {"workspace": str(self.root), **values}
        (self.home / "runtime-fixture-control").write_text("".join(f"{k}={v}\n" for k, v in values.items()))
        for name in ("runtime-fixture-entered", "runtime-fixture-release"):
            (self.home / name).unlink(missing_ok=True)

    def records(self):
        path = self.home / "runtime-fixture-calls.jsonl"
        return [json.loads(line) for line in path.read_bytes().splitlines()] if path.exists() else []

    def backend_files(self):
        return {p.name: p.read_bytes() for p in self.home.iterdir() if p.is_file()}

    def restore_backend(self, snapshot):
        for p in self.home.iterdir():
            if p.is_file() and p.name not in snapshot:
                p.unlink()
        for name, data in snapshot.items():
            (self.home / name).write_bytes(data)

    def transport_contract(self, records, frozen=True, null=True):
        for row in records:
            assert row["cwd"] == str(self.root if frozen else self.caller), row
            assert row["workspace_lock_present"], row
            assert not row["workspace_locked"], row
            assert row["inherited_fds"] == [], row
            if null:
                assert row["stdin_null"], row
            if frozen:
                equal(row["env"], {k: self.env[k] for k in ENV_KEYS}, "six frozen env keys")

    def oracle_replay(self, before, callback, *, identities=(), transport_before=None):
        """Replay at the identical inode; preserve native results after comparison.

        Only explicitly supplied UUID production sites are pinned. Events remain
        independently generated and are compared separately by their payloads.
        """
        native, backend_after = files(self.root), self.backend_files()
        native_calls = self.records()
        restore(self.root, before)
        if transport_before is not None:
            self.restore_backend(transport_before)
        identity_iter = iter(identities)
        original_uuid = uuid.uuid4

        def identity():
            try:
                return uuid.UUID(hex=next(identity_iter))
            except StopIteration:
                return original_uuid()

        try:
            with environment(self.env, self.caller), patch.object(uuid, "uuid4", side_effect=identity):
                value = callback()
            oracle = files(self.root)
            if transport_before is not None:
                # Fixture timestamps are an explicitly named observation clock,
                # not arbitrary output. Every actual argv/cwd/env/fd/lock fact
                # still compares, including legacy transport and operator stop.
                def observations(rows):
                    return [{k: v for k, v in row.items() if k != "at_ns"} for row in rows]
                equal(observations(native_calls), observations(self.records()), "actual transport transcript parity")
            for name in set(native) | set(oracle):
                if name == actions.CONTROL or name.startswith("state/actions/"):
                    assert native.get(name) == oracle.get(name), ("authority bytes", name)
                if name.startswith("checkpoints/runtime-launch-"):
                    assert native.get(name) == oracle.get(name), ("review bytes", name)
                if name in ("logs/events.jsonl", "logs/messages.jsonl"):
                    equal(history(native.get(name, b"")), history(oracle.get(name, b"")), "history " + name)
            return value
        finally:
            restore(self.root, native)
            self.restore_backend(backend_after)

    def inspect(self, action_id, pretty=False):
        before = files(self.root)
        native = self.action("inspect", action_id, pretty=pretty)
        with environment(self.env, self.root):
            expected = actions.ActionStore(self.root).inspect(action_id)
            expected["checkpoint"] = coven.checkpoint_view(self.root, expected["checkpoint_id"]).to_dict()
            expected["outcome"] = "inspected"
        equal(native, expected, "full action inspection and every digest")
        assert files(self.root) == before, "inspection wrote state"
        return native

    def enable(self):
        before = files(self.root)
        result = self.action("enable-runtime-launch")
        control = result["control"]

        def oracle():
            store = actions.ActionStore(self.root)
            with storage.transport_gate(self.root, self.root / "state/.runtime-transport.lock", exclusive=True) as gate:
                with storage.workspace_lock(self.root, self.root / "state/.workspace.lock") as held:
                    return store.enable(held, gate=gate)

        expected = self.oracle_replay(before, oracle, identities=(control["workspace_id"], control["epoch_id"]))
        equal(control, expected, "enable control")
        assert result["outcome"] == "enabled"
        again = self.action("enable-runtime-launch")
        equal(again, result, "activation idempotent")
        return result

    def prepare(self):
        before = files(self.root)
        result = self.action("prepare-runtime-launch")
        expected = self.oracle_replay(before, lambda: coven.prepare_runtime_launch(self.root),
                                      identities=(result["action_id"],))
        equal(result, expected, "prepare output/checkpoint/manifest digests")
        inspected = self.inspect(result["action_id"])
        equal(inspected["bundle"]["inputs"], self.inputs(), "complete closed inventory")
        equal(inspected["bundle"]["plan"]["agents"][0]["starter"], UNICODE, "literal starter")
        return result

    def inputs(self):
        with environment(self.env, self.root):
            return coven.runtime_launch_inputs(self.root)

    def decision(self, cp, action="approve", *, notice=False, review=False, failed=False):
        snapshot, transport_before = files(self.root), self.backend_files()
        path = self.root / "checkpoints" / (cp + ".md")
        raw = path.read_bytes() if path.exists() else None
        before = checkpoints.inspect(cp, raw)
        notes = "Reviewed literal \u96ea" if action == "approve" else "Change literal \u96ea"
        workspace = [str(self.root)] if self.caller != self.root else []
        if review:
            out, _ = self.run("review", *workspace, cp, "--message", notes, input=b"a\n")
            assert b"Snapshot: " + before.revision.encode() in out
            result = None
        elif action == "approve":
            result = self.run("approve", *workspace, cp, "--message", notes, "--expect-revision", before.revision,
                              "--json", json_output=True)
        else:
            result = self.run("suggest", *workspace, cp, notes, "--expect-revision", before.revision,
                              "--json", json_output=True)
        view = checkpoints.inspect(cp, path.read_bytes())
        d = view.decision
        assert uuid.UUID(hex=d["decision_id"]).version == 4
        assert datetime.fromisoformat(d["decided_at"]).tzinfo == timezone.utc
        expected_raw, expected = checkpoints.prepare(cp, raw, "approved" if action == "approve" else "changes_requested",
            notes, decision_id=d["decision_id"], decided_at=d["decided_at"], source="review" if review else "cli",
            initial_content=coven.checkpoint_file(cp, cp.replace("-", " ").title(), "User approval checkpoint.").encode())
        assert path.read_bytes() == expected_raw, ("decision raw bytes including scope/digest", cp)
        equal(view.to_dict(), expected.to_dict(), "checkpoint authority")
        observed = self.run("checkpoint", "inspect", *workspace, cp, "--json", json_output=True)
        equal(observed, expected.to_dict(), "checkpoint native/oracle inspection")
        if result is not None:
            for key, value in dict(outcome="committed", checkpoint=cp, action=view.effective_status,
                    decision_id=d["decision_id"], selection="conditional", before_revision=before.revision,
                    after_revision=view.revision, scope_revision=view.scope_revision).items():
                equal(result[key], value, "decision result " + key)
            assert result["sent"] == (1 if notice and not failed else 0), result
            if failed or not notice:
                assert result["warnings"], result
            else:
                assert result["warnings"] == [] and result["missing"] == [], result
        if notice and not failed:
            def oracle():
                with patch.object(coven, "now", return_value=d["decided_at"]):
                    return coven.decide_checkpoint(self.root, coven.load_team(self.root), cp,
                        view.effective_status, notes, expected_revision=before.revision,
                        source="review" if review else "cli")

            expected_result = self.oracle_replay(snapshot, oracle, identities=(d["decision_id"],),
                                                 transport_before=transport_before)
            if result is not None:
                equal(result, expected_result, "full positive decision receipt/notice parity")
        return view

    def approved(self):
        self.enable()
        prepared = self.prepare()
        view = self.decision(prepared["checkpoint"])
        return prepared, view

    def submit_args(self, prepared, view):
        return (prepared["action_id"], "--expect-manifest", prepared["manifest_digest"],
                "--expect-checkpoint", view.revision, "--expect-decision", view.decision["decision_id"])

    def submit(self, prepared, view, code=0, parity=True):
        before, transport_before = files(self.root), self.backend_files()
        result = self.action("submit", *self.submit_args(prepared, view), code=code)
        if parity:
            expected = self.oracle_replay(before, lambda: coven.submit_runtime_launch(self.root,
                prepared["action_id"], prepared["manifest_digest"], view.revision, view.decision["decision_id"]),
                identities=(result["claim"]["claim_id"],), transport_before=transport_before)
            equal(result, expected, "submit full receipt and observed report")
        return result

    def wait_barrier(self):
        deadline = time.monotonic() + 8
        while not (self.home / "runtime-fixture-entered").exists():
            assert time.monotonic() < deadline, ("backend never entered barrier", self.records())
            time.sleep(.01)

    def release(self):
        (self.home / "runtime-fixture-release").touch()


def success(suite, backend):
    w = suite.workspace("fresh-" + backend, backend)
    prepared, view = w.approved()
    result = w.submit(prepared, view)
    assert result["outcome"] == "transport_completed" and not result["replayed"]
    rows = w.records()
    w.transport_contract(rows)
    plan = result["claim"]["plan"]
    runs = [step for step in plan["steps"] if step["operation"] != "sleep"]
    assert len(rows) == len(runs), (rows, runs)
    handles = result["claim"]["report"]["handles"]
    for row, step in zip(rows, runs):
        argv = [handles[a["handle"]] if isinstance(a, dict) else a for a in step["argv"]]
        equal([row["backend"], *row["argv"]], argv, "actual ordered argv")
    launch_at = next(row["at_ns"] for row in rows if row["argv"][0] == ("new-window" if backend == "tmux" else "pane")
                     and (backend == "tmux" or row["argv"][1:3] == ["run", "fixture_agent"]))
    send_at = next(row["at_ns"] for row in rows if (row["argv"][0] == "send-keys" if backend == "tmux" else row["argv"][:2] == ["pane", "send-text"]))
    assert send_at - launch_at >= 4_900_000_000, "real five-second startup delay was skipped"
    inspected = w.inspect(prepared["action_id"])
    equal(inspected["claim"], result["claim"], "durable outcome")
    for pretty in (False, True):
        out, err = w.run("action", "submit", *w.submit_args(prepared, view), *([] if pretty else ["--json"]))
        assert not err and UNICODE.encode().split(b";")[0] in out, out
        assert out.endswith(b"\n") and not out.endswith(b"\n\n")
        assert (b"\n  \"" in out) == pretty, out
        rendered = json.loads(out)
        equal(rendered, {**result, "replayed": True}, "JSON and human pretty receipt")
    assert w.records() == rows
    # Replay must not re-read mutable startup inputs, approval, or manifest.
    (w.root / "prompts/clippy.start.txt").write_text("changed after durable claim")
    (w.root / "goal.md").write_text("changed goal")
    (w.root / "checkpoints" / (prepared["checkpoint"] + ".md")).write_text("revoked after claim")
    (w.root / "state/actions" / (prepared["action_id"] + ".json")).write_text("tampered after claim")
    replay = w.submit(prepared, view)
    assert replay["replayed"] and replay["claim"] == result["claim"]
    assert w.records() == rows, "consumed replay executed again"
    # Backend/cwd/env selection changes do not retarget historical operator stop.
    config_path = w.root / "coven.json"
    config = json.loads(config_path.read_bytes())
    config["multiplexer"] = "herdr" if backend == "tmux" else "tmux"
    config_path.write_bytes(canonical(config))
    w.env["COVEN_MULTIPLEXER"] = config["multiplexer"]
    before, transport_before = files(w.root), w.backend_files()
    stopped = w.action("stop", prepared["action_id"])
    expected = w.oracle_replay(before, lambda: coven.stop_runtime_action(w.root, prepared["action_id"]),
                              transport_before=transport_before)
    equal(stopped, expected, "frozen operator stop full report")
    report = stopped["transport_report"]
    assert report["stop_command_attempted"] and report["stop_command_succeeded"], stopped
    assert w.records()[-1]["backend"] == backend
    for alias in ("down", "stop"):
        # Recreate presence only in the inert fixture, without admitting another
        # Coven startup. Both aliases must issue the frozen scoped stop command.
        (w.home / f"runtime-fixture-{backend}-state").write_text(plan["namespace"])
        before, transport_before = files(w.root), w.backend_files()
        result = w.run(alias, json_output=True)
        expected = w.oracle_replay(before, lambda: coven.stop_runtime_action(w.root, prepared["action_id"]),
                                  transport_before=transport_before)
        equal(result, expected, "alias frozen stop report")
        assert result["transport_report"]["status"] == "transport_completed"
        assert result["transport_report"]["stop_command_attempted"], result
        assert result["transport_report"]["stop_command_succeeded"], result
        assert w.records()[-1]["backend"] == backend
    absent = w.run("down", json_output=True)
    assert not absent["transport_report"]["stop_command_attempted"], absent
    stop_rows = w.records()[len(rows):]
    assert all(row["backend"] == backend for row in stop_rows), stop_rows
    if backend == "tmux":
        assert all(row["argv"][-1] == "=" + plan["namespace"] for row in stop_rows), stop_rows
    else:
        assert all(row["argv"] == ["workspace", "list"] or row["argv"] == ["workspace", "close", "fixture_workspace"] for row in stop_rows), stop_rows
    w.transport_contract(stop_rows)
    equal(actions.ActionStore(w.root).get_claim(prepared["action_id"]), replay["claim"], "stops preserve consumption")


def tampered(suite):
    w = suite.workspace("tampered")
    prepared, view = w.approved()
    original = (w.root / "goal.md").read_bytes()
    # Semantically unchanged JSON is nevertheless a raw captured-input change.
    target = w.root / "coven.json"
    raw = target.read_bytes()
    target.write_bytes(raw + b" \n")
    before = files(w.root)
    out, err = w.run("action", "submit", *w.submit_args(prepared, view), "--json", code=1)
    assert out == b"" and b"action_captured_inputs_changed" in err, (out, err)
    assert files(w.root) == before and w.records() == []
    target.write_bytes(raw)
    (w.root / "goal.md").write_bytes(original + b"tampered raw input\n")
    w.run("action", "submit", *w.submit_args(prepared, view), "--json", code=1)
    assert w.records() == [] and actions.ActionStore(w.root).get_claim(prepared["action_id"]) is None


def failed_transport(suite, kind):
    w = suite.workspace("failure-" + kind)
    prepared, view = w.approved()
    if kind == "query":
        w.control(query_error="true")
    elif kind == "present":
        (w.home / "runtime-fixture-tmux-state").write_text("already-present")
    else:
        w.control(fail="new-window")
    result = w.submit(prepared, view, code=1)
    assert result["outcome"] == "transport_failed_or_partial"
    assert result["claim"]["report"]["diagnostics"], result
    rows = w.records()
    assert len(rows) == (3 if kind == "partial" else 1), rows
    if kind == "query":
        assert "presence query failed" in result["claim"]["report"]["diagnostics"][0]["message"]
    w.control()
    again = w.submit(prepared, view, code=1)
    assert again["replayed"] and again["claim"] == result["claim"] and w.records() == rows
    w.inspect(prepared["action_id"])
    w.transport_contract(rows)
    if kind == "partial":
        # A failed presence query is not absence; a failed required stop is
        # uncertain, not silently successful. Neither may reset consumption.
        for controls, attempted in (({"query_error": "true"}, False), ({"fail": "kill-session"}, True)):
            w.control(**controls)
            before, transport_before = files(w.root), w.backend_files()
            stopped = w.action("stop", prepared["action_id"], code=1)
            expected = w.oracle_replay(before, lambda: coven.stop_runtime_action(w.root, prepared["action_id"]),
                                      transport_before=transport_before)
            equal(stopped, expected, "failed stop exact oracle report")
            report = stopped["transport_report"]
            assert report["stop_command_attempted"] == attempted and not report["stop_command_succeeded"]
            assert report["outcome_unknown"] == attempted
        equal(actions.ActionStore(w.root).get_claim(prepared["action_id"]), result["claim"], "failed stops preserve claim")


def unknown(suite):
    w = suite.workspace("unknown")
    prepared, view = w.approved()
    # Explicit test setup only: publish a genuine durable unresolved claim using
    # the oracle. No executor crash/power-loss claim is inferred from this setup.
    with environment(w.env, w.root), storage.workspace_lock(w.root, w.root / "state/.workspace.lock") as held:
        store = actions.ActionStore(w.root)
        claim, replayed = store.claim(prepared["action_id"], prepared["manifest_digest"], view.revision,
            view.decision["decision_id"], coven.runtime_launch_inputs(w.root), held)
        assert not replayed
    result = w.submit(prepared, view, code=2)
    assert result["outcome"] == "claimed" and result["replayed"] and not w.records()
    w.run("action", "prepare-runtime-launch", "--json", code=1)
    before = files(w.root)
    w.run("action", "acknowledge-unknown", prepared["action_id"], "--expect-claim", "a" * 32, "--json", code=1)
    assert files(w.root) == before and not w.records()
    acknowledged = w.action("acknowledge-unknown", prepared["action_id"], "--expect-claim", claim["claim_id"])

    def oracle():
        with coven.action_control_mutation(w.root) as (store, held):
            return store.acknowledge_unknown(prepared["action_id"], claim["claim_id"], held)

    expected = w.oracle_replay(before, oracle)
    equal(acknowledged["claim"], expected, "acknowledged claim")
    assert acknowledged["outcome"] == "acknowledged" and expected["operator_acknowledged"]
    replay = w.submit(prepared, view, code=2)
    assert replay["outcome"] == "outcome_unknown" and replay["replayed"]
    next_action = w.prepare()
    assert next_action["action_id"] != prepared["action_id"] and not w.records()


def conditional_race(suite):
    w = suite.workspace("race")
    prepared, view = w.approved()
    w.control(barrier="new-session")
    child = w.start("action", "submit", *w.submit_args(prepared, view), "--json")
    try:
        w.wait_barrier()
        replay = w.submit(prepared, view, code=2)
        assert replay["outcome"] == "claimed" and replay["replayed"]
        conflicting = list(w.submit_args(prepared, view))
        conflicting[-1] = "a" * 32
        before = files(w.root)
        w.run("action", "submit", *conflicting, "--json", code=1)
        assert files(w.root) == before
        # The admitted runtime is blocked in I/O, but unrelated workspace writes
        # must still acquire the lock and complete before releasing that barrier.
        w.run("goal", "changed while transport is in flight")
        assert child.poll() is None
    finally:
        w.release()
        out, err = child.communicate(timeout=30)
    assert child.returncode == 0, (out, err)
    result = json.loads(out)
    assert result["claim"]["claim_id"] == replay["claim"]["claim_id"]
    assert result["claim"]["plan"]["agents"][0]["starter"] == UNICODE
    rows = w.records()
    assert sum(row["argv"][0] == "new-session" for row in rows) == 1, rows
    assert any(UNICODE in row["argv"] for row in rows), rows
    w.transport_contract(rows)


def legacy(suite, backend):
    w = suite.workspace("legacy-" + backend, backend)
    with environment(w.env, w.root):
        team = coven.load_team(w.root)
        label = coven.herdr_workspace_label(team) if backend == "herdr" else coven.team_session_name(team)
    (w.home / f"runtime-fixture-{backend}-state").write_text(label)
    w.control(delay_ms=15)
    for route in (("send",), ("agent", "send")):
        start = len(w.records())
        before, transport_before = files(w.root), w.backend_files()
        out, _ = w.run(*route, "clippy", UNICODE)

        def oracle():
            captured = io.StringIO()
            with redirect_stdout(captured):
                status = coven.main([*route, "clippy", UNICODE])
            assert status == 0
            return captured.getvalue().encode()

        expected = w.oracle_replay(before, oracle, transport_before=transport_before)
        assert out == expected, ("legacy human stdout", out, expected)
        records = w.records()[start:]
        assert any(UNICODE in row["argv"] for row in records), records
        w.transport_contract(records, frozen=False)
    for action in ("approve", "suggest"):
        start = len(w.records())
        view = w.decision("plan-approval", action, notice=True)
        rows = w.records()[start:]
        assert any(any(view.decision["decision_id"] in arg for arg in row["argv"]) for row in rows), rows
        w.transport_contract(rows, frozen=False)
    w.decision("final-review", notice=True, review=True)
    # Actual text transport succeeds, but required Enter fails. The decision
    # remains bound and committed, with warning rather than rollback or success.
    if backend == "herdr":
        w.control(fail="pane send-keys")
        start = len(w.records())
        view = w.decision("plan-approval", notice=True, failed=True)
        rows = w.records()[start:]
        assert [row["argv"][1] for row in rows if row["argv"][0] == "pane"][-2:] == ["send-text", "send-keys"], rows
        assert view.effective_status == "approved"
        w.control()
    w.enable()
    before = len(w.records())
    for route in (("send",), ("agent", "send")):
        w.run(*route, "clippy", "must not deliver", code=1)
    w.decision("plan-approval", "suggest")
    assert len(w.records()) == before, "mode blocked legacy notices must never reach backend"
    w.transport_contract(w.records(), frozen=False)


def activation_drains(suite):
    w = suite.workspace("activation-drain")
    (w.home / "runtime-fixture-tmux-state").write_text("legacy")
    w.control(barrier="send-keys")
    delivery = w.start("send", "clippy", "admitted-before-activation")
    activation = None
    try:
        w.wait_barrier()
        activation = w.start("action", "enable-runtime-launch", "--json")
        # Observe a real admitted I/O barrier rather than race an unobserved send.
        time.sleep(.25)
        assert activation.poll() is None, "activation returned before admitted effect drained"
        assert not (w.root / actions.CONTROL).exists(), "activation published while admitted backend could still act"
        w.run("goal", "writer not blocked by waiting activation")
        assert delivery.poll() is None
    finally:
        w.release()
        out, err = delivery.communicate(timeout=15)
        if activation is not None:
            activated, activation_error = activation.communicate(timeout=15)
    assert delivery.returncode == 0, (out, err)
    assert activation.returncode == 0, (activated, activation_error)
    assert json.loads(activated)["outcome"] == "enabled"
    before = w.records()
    w.run("send", "clippy", "after-activation", code=1)
    assert w.records() == before
    w.transport_contract(before, frozen=False)


def path_resolution(suite, backend, frozen):
    name = ("frozen-path-" if frozen else "legacy-caller-path-") + backend
    w = suite.workspace(name, backend)
    caller = suite.root / (name + "-caller")
    caller.mkdir()
    # Both roots have viable images, so a wrong relative-PATH base cannot pass
    # merely because no executable happened to exist at the incorrect location.
    for base in (w.root, caller):
        for directory in ("bad-interpreter", "selected", "unused"):
            target = base / directory
            target.mkdir()
            executable = target / backend
            if directory == "bad-interpreter":
                executable.write_text(f"#!{base / 'interpreter-does-not-exist'}\n")
                executable.chmod(0o755)
            else:
                shutil.copyfile(suite.bin / backend, executable)
                executable.chmod(0o755)
    w.env["PATH"] = "bad-interpreter:selected:unused"
    selected = (w.root if frozen else caller) / "selected" / backend

    def check(rows):
        assert rows, "no compiled backend image ran"
        w.transport_contract(rows, frozen=frozen)
        assert all(row["image"] == str(selected) for row in rows), rows

    if frozen:
        prepared, view = w.approved()
        w.caller = caller
        result = w.submit(prepared, view)
        assert result["outcome"] == "transport_completed" and not result["replayed"]
        check(w.records())
        equal(w.inspect(prepared["action_id"])["claim"], result["claim"], "relative PATH durable claim")
        # A real image exiting 127 is NOT a pre-image failure and must not
        # advance to the next valid candidate, even for a frozen operator stop.
        w.control(fail="has-session" if backend == "tmux" else "workspace list", fail_exit=127)
        start = len(w.records())
        before, transport_before = files(w.root), w.backend_files()
        stopped = w.action("stop", prepared["action_id"], code=1)
        expected = w.oracle_replay(before, lambda: coven.stop_runtime_action(w.root, prepared["action_id"]),
                                  transport_before=transport_before)
        equal(stopped, expected, "exited 127 frozen stop does not fall back")
        rows = w.records()[start:]
        assert len(rows) == 1 and not stopped["transport_report"]["stop_command_attempted"], rows
        check(rows)
        equal(actions.ActionStore(w.root).get_claim(prepared["action_id"]), result["claim"], "failed stop preserves claim")
    else:
        w.caller = caller
        with environment(w.env, caller):
            team = coven.load_team(w.root)
            label = coven.herdr_workspace_label(team) if backend == "herdr" else coven.team_session_name(team)
        (w.home / f"runtime-fixture-{backend}-state").write_text(label)
        for route, rejected in ((("send",), False), (("agent", "send"), True)):
            if rejected:
                w.control(fail="list-panes" if backend == "tmux" else "workspace list", fail_exit=127)
            start = len(w.records())
            before, transport_before = files(w.root), w.backend_files()
            out, _ = w.run(*route, str(w.root), "clippy", UNICODE)

            def oracle():
                captured = io.StringIO()
                with redirect_stdout(captured):
                    status = coven.main([*route, str(w.root), "clippy", UNICODE])
                assert status == 0
                return captured.getvalue().encode()

            expected = w.oracle_replay(before, oracle, transport_before=transport_before)
            assert out == expected, (out, expected)
            rows = w.records()[start:]
            check(rows)
            if rejected:
                assert len(rows) == 1 and b"Sent message to 0 agent(s)" in out, rows
                assert (w.root / "logs/messages.jsonl").read_bytes() == before["logs/messages.jsonl"]
            else:
                assert any(UNICODE in row["argv"] for row in rows), rows
        w.control()
        start = len(w.records())
        w.decision("plan-approval", notice=True)
        check(w.records()[start:])
        start = len(w.records())
        w.run("down", str(w.root))
        rows = w.records()[start:]
        check(rows)
        expected_argv = (["has-session", "-t", label], ["kill-session", "-t", label]) if backend == "tmux" else (
            ["workspace", "list"], ["workspace", "close", "fixture_workspace"])
        equal([row["argv"] for row in rows], list(expected_argv), "legacy down caller cwd and exact scope")
        assert not (w.home / f"runtime-fixture-{backend}-state").exists()


def invalid_arguments(suite):
    w = suite.workspace("invalid")
    before = files(w.root)
    commands = [
        ("action", "submit", "not-a-uuid", "--json"),
        ("action", "acknowledge-unknown", "a" * 32, "--expect-claim", "invalid"),
        ("action", "stop", "not-a-uuid", "--json"),
        ("approve", "--agent", "missing", "--json"),
        ("suggest", "plan-approval", " ", "--json"),
        ("send", "clippy"),
        ("agent", "send", "clippy"),
        ("action", "enable-runtime-launch", "--unknown-option"),
    ]
    for args in commands:
        child = w.start(*args)
        out, err = child.communicate(timeout=15)
        assert child.returncode != 0 and err, (args, out, err)
        assert files(w.root) == before and not w.records(), ("invalid args had effects", args)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--fixture", required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="coven-native-runtime-") as temporary:
        suite = Suite(Path(temporary).resolve(), str(Path(args.binary).resolve()), str(Path(args.fixture).resolve()))
        for backend in ("tmux", "herdr"):
            suite.case("fresh-" + backend, lambda b=backend: success(suite, b))
            suite.case("legacy-" + backend, lambda b=backend: legacy(suite, b))
            suite.case("legacy-caller-path-" + backend, lambda b=backend: path_resolution(suite, b, False))
            suite.case("frozen-path-" + backend, lambda b=backend: path_resolution(suite, b, True))
        suite.case("raw-tampered-inputs", lambda: tampered(suite))
        for kind in ("query", "present", "partial"):
            suite.case("failure-" + kind, lambda k=kind: failed_transport(suite, k))
        suite.case("unknown-acknowledgement", lambda: unknown(suite))
        suite.case("conditional-submit-race", lambda: conditional_race(suite))
        suite.case("activation-drains-admitted-legacy", lambda: activation_drains(suite))
        suite.case("invalid-arguments-no-effects", lambda: invalid_arguments(suite))
        required_routes = {"action enable-runtime-launch", "action prepare-runtime-launch", "action submit",
                           "action acknowledge-unknown", "action stop", "approve", "suggest", "review",
                           "send", "agent send", "down", "stop"}
        missing_routes = required_routes - suite.routes.keys()
        failed_cases = len(suite.failures)
        if missing_routes:
            suite.failures.append(("batch-route-coverage", repr(sorted(missing_routes))))
        print(json.dumps({"cases": dict(suite.cases), "case_count": sum(suite.cases.values()),
                          "attempted_cases": suite.attempted_cases, "failed_cases": failed_cases,
                          "batch_routes": len(required_routes & suite.routes.keys()),
                          "required_batch_routes": len(required_routes), "missing_batch_routes": sorted(missing_routes),
                          "native_cli_calls": suite.calls,
                          "inherited_fd_cli_calls": suite.inherited_fd_cli_calls,
                          "fd_probe_self_checks": 1,
                          "routes": dict(sorted(suite.routes.items()))}, sort_keys=True), flush=True)
        if suite.failures:
            for name, trace in suite.failures:
                print(name + "\n" + trace, file=sys.stderr)
            raise SystemExit(1)


if __name__ == "__main__":
    main()
