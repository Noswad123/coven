"""TEMPORARY development oracle, never a native child or a runtime launcher.

Acceptance uses the actual Cargo binary, fresh native init, private std-only Rust
AF_UNIX peers, and v4-pinned private heartbeat files. --oracle-only validates test
construction; it is explicitly NOT native acceptance. No installed Kitsune/Djinn,
provider, or default socket is accessed. No compile failure is replaced by a stub.

Authority bytes, IDs, revisions, provenance, hashes and payloads compare exactly.
Only receipt timestamps/durations and receipt-relative remaining lifetimes use
bounded comparison. Emitted absolute deadlines are checked, never normalized.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, chdir, contextmanager, redirect_stderr, redirect_stdout
from copy import deepcopy
from itertools import permutations
import datetime
import io
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import uuid
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import coven, groups, group_publisher, group_live, launch, workers
from test_group_publisher import inventory
from test_djinn_live_observation import native_fixture, RUNTIME, INSTANCE, SESSION

REQUIRED = {"group " + name for name in (
    "create", "add", "remove", "list", "attach-live", "detach-live", "status", "publish")}
REQUIRED |= {"observe kitsune", "observe djinn-live"}
OWNER_GATES = (
    "src/groups/tests.rs: deterministic same-lock declaration CAS, replacement/fsync uncertainty and confirmed no-rewrite",
    "src/group_publisher/tests.rs: reservation phase gaps, post-burn cancellation and all-target counter uncertainty",
    "src/group_publisher/live_tests.rs: monotonic horizon, replay/contradiction, clock rollback/suspend and capability loss",
    "src/group_publisher/native_tests.rs: real native authority/status/reservation boundary",
    "src/platform/unix/observation_socket/: selected/canonical alias-reparent fences, descriptor identity and interrupt gaps",
)


def exact(a, b):
    if isinstance(a, bytes) and isinstance(b, bytes):
        if a != b:
            at = next((at for at, pair in enumerate(zip(a, b)) if pair[0] != pair[1]), min(len(a), len(b)))
            raise AssertionError({"byte_offset": at, "lengths": [len(a), len(b)],
                "actual_bytes": a[max(0, at - 40):at + 160],
                "expected_bytes": b[max(0, at - 40):at + 160]})
        return
    def encode(value):
        if isinstance(value, bytes):
            return {"exact_bytes_hex": value.hex()}
        raise TypeError(type(value))
    assert json.dumps(a, sort_keys=True, ensure_ascii=True, allow_nan=False, default=encode) == json.dumps(
        b, sort_keys=True, ensure_ascii=True, allow_nan=False, default=encode), (a, b)


def write(path, value):
    path.write_bytes(value if isinstance(value, bytes) else json.dumps(value).encode())
    path.chmod(0o600)


def tree(root):
    result = {}
    for path in sorted(root.rglob("*")):
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode):
            result[str(path.relative_to(root))] = (
                info.st_mode, info.st_ino, info.st_mtime_ns,
                path.read_bytes() if stat.S_ISREG(info.st_mode) else os.readlink(path))
    return result


def comparable(actual, expected, low, high, path=()):
    """Validate only documented receipt fields; don't strip arbitrary numbers."""
    if path and path[-1] in {"started_at", "observed_at", "sample_received_at"}:
        # Stored binding evidence is authority and must NOT use this exception.
        if "binding" not in path:
            for value in (actual, expected):
                stamp = datetime.datetime.fromisoformat(value).timestamp()
                assert low - .1 <= stamp <= high + .1, (path, value, low, high)
            return
    if path and path[-1] == "duration_ms":
        for value in (actual, expected):
            assert type(value) in (int, float) and 0 <= value <= (high - low) * 1000 + 100
        return
    if path and path[-1] == "expires_in_ms":
        for value in (actual, expected):
            assert type(value) is int and 0 <= value <= 3000, (path, value)
        assert abs(actual - expected) <= (high - low) * 1000 + 100
        return
    if isinstance(actual, dict) and isinstance(expected, dict):
        exact(sorted(actual), sorted(expected))
        for key in actual:
            comparable(actual[key], expected[key], low, high, (*path, key))
    elif isinstance(actual, list) and isinstance(expected, list):
        assert len(actual) == len(expected)
        for at, (a, b) in enumerate(zip(actual, expected)):
            comparable(a, b, low, high, (*path, at))
    else:
        try:
            exact(actual, expected)
        except AssertionError as exc:
            raise AssertionError((path, actual, expected)) from exc


def forbidden(*args, **kwargs):
    raise AssertionError(("forbidden native operation in parent oracle", args, kwargs))


def python_cli(args, cwd, env, ids=(), stamp=None):
    out, err = io.StringIO(), io.StringIO()
    identities = iter(ids)
    with ExitStack() as stack:
        stack.enter_context(chdir(cwd))
        stack.enter_context(redirect_stdout(out))
        stack.enter_context(redirect_stderr(err))
        stack.enter_context(patch.dict(os.environ, env, clear=True))
        for module, name in ((launch, "execute"), (launch, "stop"),
                             (workers, "inspect_readiness"), (coven, "send_to_agent_pane"),
                             (subprocess, "Popen"), (subprocess, "run")):
            stack.enter_context(patch.object(module, name, forbidden))
        if ids:
            # Do not patch shared uuid.uuid4: transport correlation has its own entropy.
            stack.enter_context(patch.object(groups, "uuid", SimpleNamespace(
                UUID=uuid.UUID, RFC_4122=uuid.RFC_4122,
                uuid4=lambda: uuid.UUID(next(identities)))))
        if stamp:
            class Clock(datetime.datetime):
                @classmethod
                def now(cls, tz=None):
                    return cls.fromisoformat(stamp)
            stack.enter_context(patch.object(datetime, "datetime", Clock))
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    assert next(identities, None) is None, "unconsumed native identity"
    return code, out.getvalue().encode(), err.getvalue().encode()


def identities(raw):
    if raw is None:
        return []
    value = json.loads(raw)
    return [value["workspace_id"], *[ident for group in value["groups"]
            for ident in [group["group_id"], *[m["participant_id"] for m in group["members"]]]]]


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.routes, self.cases, self.calls = Counter(), [], Counter()
        self.failures, self.attempted_cases, self.failed_routes = [], [], set()
        self.cas_reserve_race = None
        self.bin = root / "bin"
        self.bin.mkdir()
        (self.bin / "coven").symlink_to(binary)
        self.fixture = self.bin / "group-peer"
        build = subprocess.run(["rustup", "run", "1.96.1", "rustc", "--edition=2021",
            str(ROOT / "tests/support/group_socket_fixture.rs"), "-o", str(self.fixture)],
            capture_output=True, timeout=60)
        assert build.returncode == 0, build.stderr.decode()
        exact(sorted(p.name for p in self.bin.iterdir()), ["coven", "group-peer"])

    @contextmanager
    def case(self, name):
        failures = len(self.failures)
        yield
        self.attempted_cases.append(name)
        if len(self.failures) == failures:
            self.cases.append(name)

    def compare(self, label, function):
        try:
            function()
        except AssertionError as exc:
            self.failures.append((label, str(exc)[:2400]))
            self.failed_routes.update(route for route in REQUIRED if label.startswith(route))

    def invoke(self, args, cwd, env):
        if self.oracle_only:
            self.calls["development_oracle_cli"] += 1
            return python_cli(args, cwd, env)
        self.calls["native_cli"] += 1
        result = subprocess.run([str(self.binary), *map(str, args)], cwd=cwd, env=env,
                                capture_output=True, timeout=12)
        return result.returncode, result.stdout, result.stderr

    def workspace(self, name):
        workspace = Workspace.__new__(Workspace)
        try:
            workspace.__init__(self, name)
        except BaseException:
            if hasattr(workspace, "server"):
                workspace.close()
            raise
        return workspace


class Workspace:
    def __init__(self, suite, name):
        self.suite = suite
        self.root = suite.root / name
        self.home = suite.root / (name + "-h")
        self.root.mkdir()
        self.home.mkdir(mode=0o700)
        self.peer = self.home / "peer"
        self.peer.mkdir(mode=0o700)
        self.socket = str(self.peer / "s")
        self.env = dict(PATH=str(suite.bin), HOME=str(self.home), SHELL="/bin/sh",
                        LANG="C.UTF-8", LC_ALL="C.UTF-8", TERM="dumb",
                        KITSUNE_SOCKET_PATH="/must-not-select", KITSUNE_CLIENT_SOCKET_PATH="/must-not-select")
        self.source_dir = self.home / "private"
        self.source_dir.mkdir(mode=0o700)
        self.source = self.source_dir / (RUNTIME + ".json")
        self.heartbeat()
        self.value = inventory(2)
        self.value["annotations"]["valid_until_unix_ms"] = True
        self.configure()
        self.server = subprocess.Popen([str(suite.fixture)], env=self.env,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.wait(lambda: (self.peer / "ready").exists())
        info = Path(self.socket).lstat()
        assert stat.S_ISSOCK(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600
        assert info.st_uid == os.getuid()
        result = suite.invoke(["init", self.root, "--name", "Group fixture", "--blank"], self.root, self.env)
        assert result[0] == 0, result
        # Existence marker only: group routes must not parse any of these files.
        for relative in ("coven.json", "coven.toml", "state/tasks.json", "state/agents.json",
                         "state/execution.json", "state/runtime.json"):
            write(self.root / relative, b"unparsed independent authority\xff")
        self.authority = self.root / "state/groups.json"

    def close(self):
        self.server.terminate()
        out, err = self.server.communicate(timeout=3)
        assert not out and not err, (out, err)

    def wait(self, predicate):
        end = time.monotonic() + 4
        while not predicate():
            assert self.server.poll() is None, self.server.communicate()
            assert time.monotonic() < end, "fixture barrier timed out"
            time.sleep(.005)

    def configure(self, **modes):
        write(self.peer / "snapshot.json", self.value)
        write(self.peer / "process.json", {"type": "pane_process_info", "process_info": {
            "pane_id": "w1:p1", "foreground_processes": [
                {"pid": 10, "name": "untrusted", "argv": ["/private/djinn", "PRIVATE_ARGUMENT"]}]}})
        for name, mode in modes.items():
            write(self.peer / (name + "-mode"), mode.encode())

    def heartbeat(self, mode="busy"):
        value = native_fixture()
        value["sampled_at_ms"] = int(time.time() * 1000) - 50
        if mode == "unknown":
            value["instances"][0]["sessions"][0]["activity"] = "unknown"
        elif mode == "stale":
            value["sampled_at_ms"] -= 4000
        elif mode == "future":
            value["sampled_at_ms"] += 6000
        elif mode == "unselected-schema":
            value["instances"].append({"instance_id": "malformed", "sessions": []})
        elif mode == "tampered":
            value["runtime_id"] = INSTANCE
        write(self.source, value)
        if mode == "unsafe":
            self.source.chmod(0o644)
        elif mode == "absent":
            self.source.unlink()
        return value

    def records(self):
        path = self.peer / "calls.jsonl"
        return [json.loads(line) for line in path.read_bytes().splitlines()] if path.exists() else []

    def raw(self):
        return self.authority.read_bytes() if self.authority.exists() else None

    def state(self):
        return json.loads(self.raw())

    def compare_request(self, actual, expected):
        exact(actual["method"], expected["method"])
        a, b = actual["params"], expected["params"]
        exact(sorted(a), sorted(b))
        for key in a:
            if actual["method"] == "pane.annotate" and key == "valid_until_unix_ms":
                # Independently floored receipt clocks may differ by one ms.
                # Validate both emitted scalars, never replace/drop the deadline.
                sample = json.loads(self.source.read_bytes())["sampled_at_ms"]
                for deadline in (a[key], b[key]):
                    assert type(deadline) is int and sample < deadline <= sample + 3000
                assert abs(a[key] - b[key]) <= 1, (a[key], b[key], sample)
            else:
                exact(a[key], b[key])

    def run(self, route, *args, code=0, methods=(), unchanged=False, differential=True):
        argv = [*route.split(), *map(str, args), "--json"]
        before, raw, start = tree(self.root), self.raw(), len(self.records())
        low = time.time()
        actual = self.suite.invoke(argv, self.root, self.env)
        assert actual[0] == code, (argv, actual)
        assert b"PRIVATE_" not in actual[1] + actual[2], actual
        value = json.loads(actual[1]) if actual[1].strip() else None
        after, committed = tree(self.root), self.raw()
        native_records = self.records()[start:]
        exact([r["method"] for r in native_records], list(methods))
        self.suite.calls["acceptance_socket_requests"] += len(native_records)
        for row in native_records:
            exact(sorted(row), ["id", "method", "params"])
            assert isinstance(row["id"], str) and row["id"]
        assert len({r["id"] for r in native_records}) == len(native_records)
        protected = lambda items: {k: v for k, v in items.items() if k not in {
            "state/groups.json", "state/.workspace.lock", "state/.group-publisher.lock"}}
        exact(protected(before), protected(after))
        if unchanged:
            exact(before, after)
        if committed is not None:
            assert stat.S_IMODE(self.authority.stat().st_mode) == 0o600
        if differential and not self.suite.oracle_only:
            ids = [ident for ident in identities(committed) if ident not in identities(raw)]
            member = (value or {}).get("member") or {}
            stamp = None
            if route == "group add" and member:
                stamp = member["binding"]["evidence"]["observed_at"]
            if route == "group attach-live" and member.get("live_attachment"):
                stamp = member["live_attachment"]["attached_at"]
            self.restore(raw)
            expected_start = len(self.records())
            expected = python_cli(argv, self.root, self.env, ids, stamp)
            self.suite.calls["differential_oracle_cli"] += 1
            assert expected[0] == actual[0], (argv, actual, expected)
            self.suite.compare(route + " exact authority bytes", lambda: exact(self.raw(), committed))
            if value is not None:
                self.suite.compare(route + " complete report", lambda:
                    comparable(value, json.loads(expected[1]), low, time.time()))
            oracle_records = self.records()[expected_start:]
            exact([r["method"] for r in oracle_records], list(methods))
            for a, b in zip(native_records, oracle_records):
                # Only transport correlation IDs are non-authority randomness.
                self.suite.compare(route + " exact request payload", lambda:
                    self.compare_request(a, b))
            self.suite.calls["oracle_socket_requests"] += len(oracle_records)
            self.restore(committed)
            if committed is not None:
                info = self.authority.stat()
                os.utime(self.authority, ns=(info.st_atime_ns, after["state/groups.json"][2]))
        self.suite.routes[route] += 1
        if route == "group publish" and value:
            assert sum(value[k] for k in ("acknowledged", "rejected", "unsent", "uncertain")) == value["reservation_attempted_targets"]
            assert value["reserved_targets"] + value["reservation_uncertain"] == value["reservation_attempted_targets"]
        return value, native_records

    def restore(self, raw):
        if raw is None:
            self.authority.unlink(missing_ok=True)
        else:
            write(self.authority, raw)

    def create(self, key="team"):
        return self.run("group create", self.root, key, "--name", "Review Team")[0]

    def add(self, member="one", pane="w1:p1", process=False, rebind=False, code=0):
        methods = ["session.snapshot", "pane.process_info", "session.snapshot"] if process else ["session.snapshot"]
        return self.run("group add", self.root, "team", "--member", member, "--role", "Reviewer",
                        "--socket", self.socket, "--pane", pane,
                        *(["--process-evidence"] if process else []),
                        *(["--rebind"] if rebind else []), code=code, methods=methods)[0]

    def attach(self, code=0):
        return self.run("group attach-live", self.root, "team", "--member", "one",
                        *self.pins(), code=code)[0]

    def pins(self):
        return ["--file", self.source, "--runtime", RUNTIME, "--instance", INSTANCE, "--session", SESSION]

    def publish(self, live=False, code=0, methods=None, source="coven", duration=1, differential=True):
        if methods is None:
            methods = ["session.snapshot", "pane.annotate"]
        return self.run("group publish", self.root, "team", "--socket", self.socket,
            "--source", source, "--duration", duration, *(["--live"] if live else []),
            code=code, methods=methods, differential=differential)


def workflow(w):
    s = w.suite
    with s.case("fresh-native-init-uninitialized-read"):
        report, _ = w.run("group list", w.root, unchanged=True)
        assert report["initialized"] is False
    with s.case("create-confirmed-noop-no-rewrite"):
        first = w.create()
        report, _ = w.run("group create", w.root, "team", "--name", "Review Team", unchanged=True)
        assert report["outcome"] == "unchanged"
        exact(first["group"], report["group"])
    with s.case("snapshot-add-schema-one-and-unchanged"):
        annotation_capabilities = w.value.pop("annotations")
        w.configure()
        first = w.add()
        assert w.state()["schema_version"] == 1
        before = tree(w.root)
        again = w.add()
        exact(first["member"], again["member"])
        exact(before, tree(w.root))
        w.value["annotations"] = annotation_capabilities
        w.configure()
    with s.case("static-two-target-payload-and-counter-replay"):
        w.add("two", "w1:p2")
        revision = groups.read_groups(w.root)["revision"]
        for number in (1, 2):
            report, rows = w.publish(methods=["session.snapshot", "pane.annotate", "pane.annotate"])
            assert report["acknowledged"] == report["reserved_targets"] == 2
            exact([r["params"] for r in rows[1:]], [dict(pane_id=f"w1:p{i}", expected_terminal_id=f"terminal-{i}",
                annotation_epoch="epoch-1", source="coven", revision=number,
                text="Review Team \u00b7 Reviewer", ttl_ms=6000) for i in (1, 2)])
            exact(w.state()["publication_counters"][0]["watermarks"], {"terminal-1": number, "terminal-2": number})
            assert groups.read_groups(w.root)["revision"] == revision
    with s.case("process-requires-rebind-promotes-schema-two"):
        raw = w.raw()
        w.add(process=True, code=1)
        exact(w.raw(), raw)
        rebound = w.add(process=True, rebind=True)
        assert rebound["member"]["participant_id"] == first["member"]["participant_id"]
        assert w.state()["schema_version"] == 2
        exact(rebound["member"]["binding"]["evidence"]["process_evidence"]["matches"], [
            {"pid": 10, "executable_family": "djinn", "rule": "exact_posix_argv0_basename"}])
    with s.case("mixed-strict-publication-process-bracket"):
        w.publish(methods=["session.snapshot", "session.snapshot", "pane.process_info", "session.snapshot",
                           "pane.annotate", "pane.annotate"])
    with s.case("mixed-strict-rejects-malformed-legacy-before-process"):
        raw = w.raw()
        w.value["agents"] = "malformed legacy inventory"
        w.configure()
        report, _ = w.publish(code=1, methods=["session.snapshot"])
        assert report["diagnostics"] == ["schema_error"]
        exact(w.raw(), raw)
        w.value["agents"] = deepcopy(w.value["panes"])
        w.configure()
    with s.case("remove-preserves-counter-no-label-clear"):
        counters = deepcopy(w.state()["publication_counters"])
        w.run("group remove", w.root, "team", "--member", "two")
        exact(w.state()["publication_counters"], counters)
        w.run("group remove", w.root, "team", "--member", "two", unchanged=True)
    with s.case("process-only-generic-malformed-legacy"):
        w.value["agents"] = "ignored by generic pane contract"
        w.configure()
        w.publish(methods=["session.snapshot", "session.snapshot", "pane.process_info", "session.snapshot", "pane.annotate"])
    with s.case("expected-terminal-missing-before-process-query"):
        raw = w.raw()
        w.value["panes"][0]["terminal_id"] = "replacement"
        w.configure()
        report, _ = w.publish(code=1, methods=["session.snapshot"])
        assert report["diagnostics"] == ["bound_terminal_missing"]
        exact(w.raw(), raw)
        w.value["panes"][0]["terminal_id"] = "terminal-1"
        w.value["agents"] = deepcopy(w.value["panes"])
        w.configure()
    with s.case("frozen-endpoint-mismatch-before-process-query"):
        raw = w.raw()
        changed = w.state()
        changed["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"]["inode"] += 1
        write(w.authority, changed)
        before = w.raw()
        report, _ = w.publish(code=1, methods=["session.snapshot"])
        assert report["diagnostics"] == ["endpoint_replaced"]
        exact(w.raw(), before)
        w.restore(raw)
    with s.case("aggregate-one-MiB-process-read-budget"):
        w.value["padding"] = ["s" * 4000] * 100
        w.configure()
        process = json.loads((w.peer / "process.json").read_bytes())
        process["padding"] = ["p" * 4000] * 100
        write(w.peer / "process.json", process)
        raw = w.raw()
        report, _ = w.publish(code=1, methods=["session.snapshot", "session.snapshot", "pane.process_info"])
        assert report["diagnostics"] == ["response_limit"]
        exact(w.raw(), raw)
        report, _ = w.run("observe kitsune", "--socket", w.socket, "--process-pane", "w1:p1", code=1,
            methods=["session.snapshot", "pane.process_info", "session.snapshot"], unchanged=True)
        assert report["diagnostics"] == ["response_limit"]
        w.run("observe kitsune", "--socket", w.socket, "--process-pane", "w1:p1", "--max-response-bytes", 2 * 1024 * 1024,
            methods=["session.snapshot", "pane.process_info", "session.snapshot"], unchanged=True)
        del w.value["padding"]
        w.configure()
    with s.case("unsafe-attach-preserves-schema-two"):
        w.heartbeat("unsafe")
        raw = w.raw()
        w.attach(code=1)
        exact(w.raw(), raw)
        assert w.state()["schema_version"] == 2
    with s.case("busy-attach-promotes-schema-three-and-noop"):
        w.heartbeat()
        w.attach()
        assert w.state()["schema_version"] == 3
        before = tree(w.root)
        assert w.attach()["outcome"] == "unchanged"
        exact(before, tree(w.root))
    with s.case("status-busy-and-live-actual-annotation-deadline"):
        sample = w.heartbeat()["sampled_at_ms"]
        report, _ = w.run("group status", w.root, "team", unchanged=True)
        assert report["participants"][0]["activity"] == "busy"
        report, rows = w.publish(live=True, methods=[
            "session.snapshot", "session.snapshot", "pane.process_info", "session.snapshot", "pane.annotate"])
        params = rows[-1]["params"]
        assert params["text"] == "Review Team \u00b7 Reviewer \u00b7 legacy runner busy"
        assert params["ttl_ms"] == 3000
        assert sample < params["valid_until_unix_ms"] <= sample + 3000
    # Convert to snapshot provenance for the remaining live/fault workflows.
    with s.case("rebind-clears-live-declaration-not-participant"):
        participant = w.state()["groups"][0]["members"][0]["participant_id"]
        w.add(rebind=True)
        member = w.state()["groups"][0]["members"][0]
        assert member["participant_id"] == participant and member["live_attachment"] is None
        w.heartbeat()
        w.attach()
    for mode in ("unknown", "absent", "stale", "future", "unsafe", "tampered", "unselected-schema"):
        with s.case("live-source-" + mode + "-role-only"):
            w.heartbeat(mode)
            status, _ = w.run("group status", w.root, "team", unchanged=True)
            assert status["participants"][0]["activity"] == "unknown"
            _, rows = w.publish(live=True)
            params = rows[-1]["params"]
            assert params["text"] == "Review Team \u00b7 Reviewer" and params["ttl_ms"] == 6000
            assert "valid_until_unix_ms" not in params
    with s.case("unknown-attach-accepted"):
        w.run("group detach-live", w.root, "team", "--member", "one")
        w.heartbeat("unknown")
        w.attach()
    with s.case("static-never-opens-attachments"):
        # A FIFO would block an attachment reader. The static path must not open it.
        w.source.unlink()
        os.mkfifo(w.source, 0o600)
        _, rows = w.publish()
        assert rows[-1]["params"]["ttl_ms"] == 6000
        w.source.unlink()
    with s.case("detach-missing-source-no-rewrite-on-confirmation"):
        w.run("group detach-live", w.root, "team", "--member", "one")
        w.run("group detach-live", w.root, "team", "--member", "one", unchanged=True)
    for mode, code in (("busy", 0), ("unknown", 0), ("absent", 1), ("stale", 1),
                       ("future", 1), ("unsafe", 1), ("tampered", 1), ("unselected-schema", 1)):
        with s.case("observe-djinn-live-" + mode):
            w.heartbeat(mode)
            report, _ = w.run("observe djinn-live", *w.pins(), code=code, unchanged=True)
            assert report["activity"] == ("busy" if mode == "busy" else "unknown")
    with s.case("observe-kitsune-strict-and-process-generic"):
        w.run("observe kitsune", "--socket", w.socket, methods=["session.snapshot"], unchanged=True)
        w.value["agents"] = {"malformed": "legacy"}
        w.configure()
        report, _ = w.run("observe kitsune", "--socket", w.socket, "--process-pane", "w1:p1",
            methods=["session.snapshot", "pane.process_info", "session.snapshot"], unchanged=True)
        assert report["observations"][0]["matches"][0]["pid"] == 10
        w.run("observe kitsune", "--socket", w.socket, code=1, methods=["session.snapshot"], unchanged=True)
        w.value["agents"] = deepcopy(w.value["panes"])
        w.configure()
    with s.case("actual-selected-parent-alias"):
        alias = w.home / "alias"
        alias.symlink_to(w.peer, target_is_directory=True)
        report, _ = w.run("observe kitsune", "--socket", alias / "s", methods=["session.snapshot"], unchanged=True)
        assert report["endpoint"]["resolved"] == w.socket
    for capability in (None, False, 1, "true"):
        with s.case("literal-deadline-capability-" + repr(capability)):
            if capability is None:
                w.value["annotations"].pop("valid_until_unix_ms", None)
            else:
                w.value["annotations"]["valid_until_unix_ms"] = capability
            w.configure()
            raw = w.raw()
            report, _ = w.publish(live=True, code=1, methods=["session.snapshot"])
            assert report["diagnostics"] == ["annotation_deadline_unsupported"]
            exact(w.raw(), raw)
    with s.case("static-without-deadline-capability"):
        w.value["annotations"].pop("valid_until_unix_ms", None)
        w.configure()
        w.publish()
    with s.case("disabled-source-no-burn"):
        raw = w.raw()
        report, _ = w.publish(source="disabled", code=1, methods=["session.snapshot"])
        assert report["diagnostics"] == ["source_disabled"]
        exact(w.raw(), raw)
    for mode, phase, code in (("lost", "uncertain", 2), ("eof-open", "uncertain", 2), ("no-eof", "uncertain", 2),
                              ("wrong-id", "uncertain", 2), ("trailing", "uncertain", 2),
                              ("error", "rejected", 1)):
        with s.case("ack-" + mode + "-whole-cycle-burn-no-retry"):
            # Two targets reserve together even when the first ACK fails.
            if len(w.state()["groups"][0]["members"]) == 1:
                w.add("two", "w1:p2")
            before = deepcopy(w.state()["publication_counters"][0]["watermarks"])
            w.configure(annotate=mode)
            report, _ = w.publish(code=code)
            assert report[phase] == 1 and report["unsent"] == 1
            assert report["reserved_targets"] == 2 and report["cycles"] == 1
            assert report["completed_cycles"] == 0
            exact(w.state()["publication_counters"][0]["watermarks"], {k: v + 1 for k, v in before.items()})
            w.configure(annotate="")
    with s.case("public-one-MiB-budget-before-reservation"):
        w.value["padding"] = ["x" * 4000] * 263
        w.configure()
        raw = w.raw()
        report, _ = w.publish(code=1, methods=["session.snapshot"])
        assert report["diagnostics"] == ["response_limit"]
        exact(w.raw(), raw)
        w.run("observe kitsune", "--socket", w.socket, code=1, methods=["session.snapshot"], unchanged=True)
        del w.value["padding"]
        w.configure()


def invalid_inputs(w):
    s = w.suite
    # Each invalid mutation uses a pristine marker, without an existing lock.
    for name, argv in (
        ("create-selector", ["group", "create", "Bad"]),
        ("create-display", ["group", "create", "team", "--name", ""]),
        ("add-selector", ["group", "add", "team", "--member", "Bad", "--role", "Role", "--pane", "w1:p1", "--socket", w.socket]),
        ("add-pane", ["group", "add", "team", "--member", "one", "--role", "Role", "--pane", "bad pane", "--socket", w.socket]),
        ("remove-selector", ["group", "remove", "team", "--member", "Bad"]),
        ("attach-v4-pin", ["group", "attach-live", "team", "--member", "one", "--file", w.source, "--runtime", "invalid", "--instance", INSTANCE, "--session", SESSION]),
        ("detach-selector", ["group", "detach-live", "team", "--member", "Bad"]),
        ("publish-duration", ["group", "publish", "team", "--socket", w.socket, "--source", "coven", "--duration", "0"]),
    ):
        with s.case("invalid-prelock-" + name):
            pristine = s.root / ("invalid-" + name)
            pristine.mkdir()
            (pristine / "state").mkdir()
            write(pristine / "coven.json", b"marker-not-json")
            before, start = tree(pristine), len(w.records())
            args = [*argv[:2], pristine, *argv[2:], "--json"]
            result = s.invoke(args, pristine, w.env)
            assert result[0] != 0, (args, result)
            exact(tree(pristine), before)
            assert len(w.records()) == start
    for name, args in (
        ("relative-socket", ["observe", "kitsune", "--socket", "relative"]),
        ("dotdot-socket", ["observe", "kitsune", "--socket", str(w.peer / ".." / "peer" / "s")]),
        ("typed-duration", ["group", "publish", w.root, "team", "--socket", w.socket, "--source", "coven", "--duration", "true"]),
        ("typed-timeout", ["observe", "kitsune", "--socket", w.socket, "--timeout", "true"]),
        ("typed-budget", ["observe", "kitsune", "--socket", w.socket, "--max-response-bytes", "1024.0"]),
        ("bad-utf8", ["group", "create", w.root, os.fsdecode(b"bad\xff")]),
    ):
        with s.case("badargs-" + name):
            before, start = tree(w.root), len(w.records())
            result = s.invoke(args, w.root, w.env)
            assert result[0] != 0, (args, result)
            exact(tree(w.root), before)
            assert len(w.records()) == start
    for mode in ("missing", "regular", "symlink", "unsafe"):
        with s.case("invalid-endpoint-no-fallback-" + mode):
            selected = w.peer / "invalid"
            selected.unlink(missing_ok=True)
            if mode == "regular":
                write(selected, b"not a socket")
            elif mode == "symlink":
                selected.symlink_to(w.socket)
            elif mode == "unsafe":
                selected = Path(w.socket)
                selected.chmod(0o660)
            w.run("observe kitsune", "--socket", selected, code=1, unchanged=True)
            Path(w.socket).chmod(0o600)


def bignum_acceptance(w):
    huge = int("1234567890" * 10)
    assert len(str(huge)) == 100
    with w.suite.case("ignored-100-digit-wire-extensions-snapshot-process-ack"):
        original = deepcopy(w.value)
        w.value["ignored_future"] = huge
        for collection in ("panes", "agents"):
            for pane in w.value[collection]:
                pane["ignored_future"] = huge
        w.configure()
        process = json.loads((w.peer / "process.json").read_bytes())
        process["ignored_future"] = huge
        process["process_info"]["ignored_future"] = huge
        process["process_info"]["foreground_processes"][0]["ignored_future"] = huge
        write(w.peer / "process.json", process)
        write(w.peer / "annotate.json", {"type": "ok", "ignored_future": huge})
        try:
            w.run("observe kitsune", "--socket", w.socket, methods=["session.snapshot"], unchanged=True)
            w.run("observe kitsune", "--socket", w.socket, "--process-pane", "w1:p1",
                methods=["session.snapshot", "pane.process_info", "session.snapshot"], unchanged=True)
            report, _ = w.publish(methods=["session.snapshot", "pane.annotate", "pane.annotate"])
            assert report["acknowledged"] == 2
        finally:
            w.value = original
            (w.peer / "annotate.json").unlink()
            w.configure()
    with w.suite.case("100-digit-authority-endpoint-read-mutate-preserve"):
        original = w.raw()
        authority = w.state()
        for member in authority["groups"][0]["members"]:
            member["binding"]["presentation"]["endpoint"]["device"] = huge
        write(w.authority, authority)
        try:
            report, _ = w.run("group list", w.root, unchanged=True)
            exact(report["revision"], groups.read_groups(w.root)["revision"])
            for member in report["groups"][0]["members"]:
                exact(member["binding"]["presentation"]["endpoint"]["device"], huge)
            # Unrelated mutation must retain the full integer and exact oracle bytes.
            w.create("bignum-preservation")
            assert str(huge).encode() in w.raw()
            for member in w.state()["groups"][0]["members"]:
                exact(member["binding"]["presentation"]["endpoint"]["device"], huge)
        finally:
            w.restore(original)


def reservation_endpoint_order(w):
    original = w.raw()
    try:
        for order in permutations(("path", "device", "inode")):
            with w.suite.case("reservation-declaration-endpoint-order-" + "-".join(order)):
                authority = json.loads(original)
                counters = deepcopy(authority["publication_counters"])
                for member in authority["groups"][0]["members"]:
                    presentation = member["binding"]["presentation"]
                    endpoint = presentation["endpoint"]
                    presentation["endpoint"] = {key: endpoint[key] for key in order}
                declarations = deepcopy(authority["groups"])
                # Retain the existing counter and create a different source stream.
                w.value["annotations"]["sources"].append("ordered")
                w.configure()
                write(w.authority, authority)
                revision = groups.read_groups(w.root)["revision"]
                try:
                    report, _ = w.publish(source="ordered", methods=[
                        "session.snapshot", "pane.annotate", "pane.annotate"])
                    assert report["reserved_targets"] == report["acknowledged"] == 2
                    stored = w.state()
                    exact(stored["groups"], declarations)
                    exact(stored["publication_counters"][:-1], counters)
                    # Compare serialized objects as well: dict equality hides key order.
                    exact(json.dumps(stored["groups"]), json.dumps(declarations))
                    exact(json.dumps(stored["publication_counters"][:-1]), json.dumps(counters))
                    counter = stored["publication_counters"][-1]
                    exact(list(counter["endpoint"]), list(order))
                    exact(counter["watermarks"], {"terminal-1": 1, "terminal-2": 1})
                    assert groups.read_groups(w.root)["revision"] == revision
                    # Workspace.run also compares the whole authority's exact storage
                    # bytes against an independent Python publication from the same input.
                finally:
                    w.value["annotations"]["sources"].remove("ordered")
                    w.configure()
    finally:
        w.restore(original)


def concurrency(w):
    if w.suite.oracle_only:
        return  # Python must never stand in for a native publisher child.
    s = w.suite
    def start(duration=10):
        s.calls["native_cli"] += 1
        return subprocess.Popen([str(s.binary), "group", "publish", str(w.root), "team",
            "--socket", w.socket, "--source", "coven", "--duration", str(duration), "--json"],
            cwd=w.root, env=w.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    def finish(child, code):
        out, err = child.communicate(timeout=4)
        assert not err, (child.returncode, out, err)
        s.compare("group publish native exit status", lambda: exact(child.returncode, code))
        report = json.loads(out)
        assert sum(report[k] for k in ("acknowledged", "rejected", "unsent", "uncertain")) == report["reservation_attempted_targets"]
        assert report["reserved_targets"] + report["reservation_uncertain"] == report["reservation_attempted_targets"]
        s.routes["group publish"] += 1
        return report
    with s.case("native-SIGINT-summary-130-no-exit-clear"):
        start_at = len(w.records())
        child = start()
        try:
            w.wait(lambda: any(r["method"] == "pane.annotate" for r in w.records()[start_at:]))
            child.send_signal(signal.SIGINT)
            report = finish(child, 130)
            assert report["outcome"] == "interrupted"
            assert all(r["params"].get("text") for r in w.records()[start_at:] if r["method"] == "pane.annotate")
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()
    for phase in ("snapshot", "annotate"):
        with s.case("native-lease-writer-availability-during-" + phase + "-IO"):
            gate = w.peer / ("hold-" + phase)
            write(gate, b"hold")
            start_at = len(w.records())
            raw = w.raw()
            child = start()
            try:
                w.wait(lambda: any(r["method"] == ("session.snapshot" if phase == "snapshot" else "pane.annotate")
                                   for r in w.records()[start_at:]))
                report, _ = w.publish(code=1, methods=[])
                assert report["diagnostics"] == ["publisher_busy"]
                w.create("edit-" + phase)
                edited = w.raw()
                gate.unlink()
                if phase == "snapshot":
                    report = finish(child, 2)
                    native_rows = w.records()[start_at:]
                    counters = json.loads(raw)["publication_counters"]
                    native_counters = w.state()["publication_counters"]
                    exact(native_counters, counters)
                    exact(w.raw(), edited)
                    exact([r["method"] for r in native_rows], ["session.snapshot"])
                    # Replay the identical declaration bytes after Python's frozen
                    # read, while its actual snapshot request is held by the peer.
                    # The oracle runs in the parent main thread, never as a child.
                    w.restore(raw)
                    write(gate, b"hold")
                    oracle_start = len(w.records())
                    errors = []
                    def edit_after_snapshot():
                        try:
                            w.wait(lambda: len(w.records()) > oracle_start)
                            w.restore(edited)
                        except BaseException as exc:
                            errors.append(exc)
                        finally:
                            gate.unlink(missing_ok=True)
                    editor = threading.Thread(target=edit_after_snapshot)
                    editor.start()
                    try:
                        expected = python_cli(["group", "publish", w.root, "team", "--socket", w.socket,
                            "--source", "coven", "--duration", "10", "--json"], w.root, w.env)
                    finally:
                        editor.join(timeout=5)
                    assert not editor.is_alive() and not errors, errors
                    s.calls["differential_oracle_cli"] += 1
                    oracle_rows = w.records()[oracle_start:]
                    s.calls["oracle_socket_requests"] += len(oracle_rows)
                    assert expected[0] == 2 and not expected[2], expected
                    oracle_report = json.loads(expected[1])
                    exact(report, oracle_report)
                    exact(w.raw(), edited)
                    exact(w.state()["publication_counters"], counters)
                    exact([r["method"] for r in oracle_rows], ["session.snapshot"])
                    for a, b in zip(native_rows, oracle_rows):
                        w.compare_request(a, b)
                    exact({k: report[k] for k in ("outcome", "diagnostics", "reservation_attempts",
                        "reservation_attempted_targets", "reserved_targets", "reservation_uncertain",
                        "cycles", "completed_cycles", "acknowledged", "rejected", "unsent", "uncertain")},
                        dict(outcome="uncertain", diagnostics=["reservation_failed"], reservation_attempts=1,
                            reservation_attempted_targets=2, reserved_targets=0, reservation_uncertain=2,
                            cycles=0, completed_cycles=0, acknowledged=0, rejected=0, unsent=2, uncertain=0))
                    s.cas_reserve_race = {"barrier": "snapshot reply held after frozen declaration read; identical edit before reserve",
                        "native": {"exit": child.returncode, "summary": report,
                            "requests": [{k: r[k] for k in ("method", "params")} for r in native_rows],
                            "counters_before": counters, "counters_after": native_counters},
                        "python": {"exit": expected[0], "summary": oracle_report,
                            "requests": [{k: r[k] for k in ("method", "params")} for r in oracle_rows],
                            "counters_before": counters, "counters_after": w.state()["publication_counters"]},
                        "declaration_bytes_equal_after_replay": w.raw() == edited,
                        "interpretation": "Unchanged fixture counters do not prove rollback or a known reservation outcome."}
                else:
                    w.wait(lambda: not gate.exists())
                    child.send_signal(signal.SIGINT)
                    report = finish(child, 130)
                    assert report["reserved_targets"] == 2
                    # Admitted writes may complete after edits; stopping is not cancellation.
                    assert report["acknowledged"] + report["uncertain"] >= 1
            finally:
                gate.unlink(missing_ok=True)
                if child.poll() is None:
                    child.kill()
                    child.communicate()

    with s.case("native-live-horizon-same-evidence-loss-unknown-replay"):
        w.value["annotations"]["valid_until_unix_ms"] = True
        w.configure()
        original = w.heartbeat()
        w.attach()
        start_at = len(w.records())
        s.calls["native_cli"] += 1
        child = subprocess.Popen([str(s.binary), "group", "publish", str(w.root), "team",
            "--socket", w.socket, "--source", "coven", "--duration", "9", "--live", "--json"],
            cwd=w.root, env=w.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def writes():
            return [r["params"] for r in w.records()[start_at:] if r["method"] == "pane.annotate"]
        try:
            w.wait(lambda: len(writes()) >= 4)
            busy = [p for p in writes() if p["expected_terminal_id"] == "terminal-1"]
            assert len(busy) == 2
            first, second = busy
            for params in busy:
                assert params["text"].endswith("legacy runner busy") and params["ttl_ms"] == 3000
                assert original["sampled_at_ms"] < params["valid_until_unix_ms"] <= original["sampled_at_ms"] + 3000
            assert second["valid_until_unix_ms"] <= first["valid_until_unix_ms"], busy
            w.source.unlink()
            w.wait(lambda: len(writes()) >= 6)
            unknown = w.heartbeat("unknown")
            unknown["sequence"] = 2
            write(w.source, unknown)
            w.wait(lambda: len(writes()) >= 8)
            write(w.source, original)  # Older busy evidence must not acquire a new lease.
            report = finish(child, 0)
            assert report["completed_cycles"] == 5 and report["acknowledged"] == 10
            frames = writes()
            assert len(frames) == 10
            for params in frames[4:]:
                assert params["text"] == "Review Team \u00b7 Reviewer"
                assert params["ttl_ms"] == 6000 and "valid_until_unix_ms" not in params
            watermarks = w.state()["publication_counters"][0]["watermarks"]
            for terminal in ("terminal-1", "terminal-2"):
                numbers = [p["revision"] for p in frames if p["expected_terminal_id"] == terminal]
                exact(numbers, list(range(numbers[0], numbers[0] + 5)))
                assert watermarks[terminal] == numbers[-1]
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--oracle-only", action="store_true", help="development construction check, NOT acceptance")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="cgo-", dir="/tmp") as directory:
        suite = Suite(Path(directory), binary, args.oracle_only)
        w = suite.workspace("w")
        try:
            workflow(w)
            bignum_acceptance(w)
            reservation_endpoint_order(w)
            invalid_inputs(w)
            concurrency(w)
            exact(sorted(suite.routes), sorted(REQUIRED))
            assert len(suite.cases) == len(set(suite.cases))
            total_requests = len(w.records())
            suite.calls["total_fixture_socket_requests"] = total_requests
            suite.calls["native_socket_requests" if not args.oracle_only else "development_socket_requests"] = (
                total_requests - suite.calls["oracle_socket_requests"])
            print(json.dumps({"mode": "DEVELOPMENT ORACLE ONLY" if args.oracle_only else "native acceptance",
                "verified_cases": len(suite.cases), "cases": suite.cases,
                "attempted_cases": len(suite.attempted_cases), "parity_failures": suite.failures,
                "exercised_routes": len(suite.routes),
                "verified_routes": len(set(suite.routes) - suite.failed_routes),
                "route_calls": dict(sorted(suite.routes.items())),
                "calls": dict(suite.calls), "cas_reserve_race": suite.cas_reserve_race,
                "separate_unit_owner_gates": OWNER_GATES}, indent=2))
            assert not suite.failures, "native parity failures reported above (not masked)"
        finally:
            w.close()


if __name__ == "__main__":
    main()
