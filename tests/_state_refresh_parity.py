"""Existing-workspace CLI acceptance, with Python as the byte-level oracle.

No native init, Python subprocess delegation, provider, or runtime is permitted.
Native children have an empty PATH; Python fixture/oracle calls are in-process
with runtime entry points guarded. Only UUIDs/clocks actually read from native
artifacts are frozen. Oracle replays use the SAME absolute workspace path.

Private SDK units own marker/capture races, queued-head and raw-byte rechecks,
post-primary sync/render uncertainty, partial backup failures and exclusive role
collisions. No production failpoints or substitute CLI APIs are introduced here.
Native-only profiles: new SDK ledger/recovery dirs request 0700 (inherited umask
and parent SGID apply), raw recovery <=32 MiB, refresh text
<=1 MiB/file and stage <=32 MiB/4096 files (dashboard separately <=32 MiB),
bundled examples only, default COVEN_COMMAND='coven'. Oracle-only validates the
Python-compatible cases before the parent wires the new native commands.
NativeSafetyRefusal rejects authored aliases of force-written targets before
locking. NativeSafetyGeneration permits only a validated data-quoted printf
prefix for unsafe roots, not Python's inherited shell-formatting hazard.
"""

import argparse
from collections import Counter
import contextlib
import io
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
from unittest import mock
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.set_int_max_str_digits(4300)
from runner import actions, coven, storage, task_ledger as ledger

ENV = {k: v for k, v in os.environ.items() if not k.startswith("COVEN_")}
ENV.update(PATH="", COVEN_MULTIPLEXER="", COVEN_COMMAND="/fixed path/'coven'",
           PYTHONDONTWRITEBYTECODE="1", PYTHONINTMAXSTRDIGITS="4300")
CHILDREN = set()
STAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{6})?\+00:00\Z")


class CorpusTimeout(BaseException):
    """A corpus deadline must not be swallowed by per-case failure collection."""


def copy_file(source, destination):
    # copytree deliberately refuses special files; our unsafe-path fixtures must
    # preserve FIFOs without ever opening them for data reads.
    if stat.S_ISFIFO(os.stat(source, follow_symlinks=False).st_mode):
        os.mkfifo(destination, stat.S_IMODE(os.stat(source).st_mode))
        return destination
    return shutil.copy2(source, destination)


def clone(source, destination):
    shutil.copytree(source, destination, symlinks=True, copy_function=copy_file)


def files(root):
    result = {}
    for path in root.rglob("*"):
        meta = path.lstat()
        mode = stat.S_IMODE(meta.st_mode)
        name = str(path.relative_to(root))
        if path.is_symlink():
            result[name] = ("link", mode, os.readlink(path), meta.st_ino)
        elif path.is_dir():
            result[name] = ("dir", mode, meta.st_ino)
        elif path.is_file():
            result[name] = ("file", mode, path.read_bytes(), meta.st_ino, meta.st_mtime_ns, meta.st_nlink)
        else:
            result[name] = ("special", mode, meta.st_ino)
    return result


def equal(actual, expected, label):
    # Preserve JSON numeric types, negative zero, and all actual value fields.
    a = json.dumps(actual, sort_keys=True, ensure_ascii=True, allow_nan=False)
    b = json.dumps(expected, sort_keys=True, ensure_ascii=True, allow_nan=False)
    assert a == b, f"{label}: actual={a[:4000]} expected={b[:4000]}"


def python(args, cwd):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.chdir(cwd), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            status = coven.main(list(map(str, args)))
        except SystemExit as exc:
            status = exc.code
    return status, out.getvalue(), err.getvalue()


def spawn(args, cwd, env=None):
    child = subprocess.Popen(list(map(str, args)), cwd=cwd, env=env or ENV,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True)
    CHILDREN.add(child)
    return child


def finish(child, timeout=8):
    try:
        out, err = child.communicate(timeout=timeout)
        return child.returncode, out.decode(), err.decode()
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=2)
        CHILDREN.discard(child)


def identifier(value):
    parsed = uuid.UUID(value)
    assert parsed.version == 4 and parsed.variant == uuid.RFC_4122
    assert value == parsed.hex, value
    return parsed


def effects(before, after, allowed):
    for key in set(before) | set(after):
        if key not in allowed:
            assert before.get(key) == after.get(key), f"unallowed filesystem effect: {key}"
    lock = "state/.workspace.lock"
    if lock in before:
        assert after.get(lock) == before[lock], "lock inode/content/mode changed"
    elif lock in after:
        assert after[lock][:3] == ("file", 0o600, b""), after[lock]


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        # This oracle process is single-threaded; restore immediately so fixtures,
        # Python replays and native children all inherit the same original mask.
        self.umask = os.umask(0)
        os.umask(self.umask)
        self.serial = 0
        self.counts, self.failed, self.failures = Counter(), Counter(), []
        self.bases = {}
        for kind in ("blank", "default", "interactive-demo"):
            path = root / ("base-" + kind)
            args = ["init", path]
            if kind == "blank":
                args += ["--blank", "--agents", "operator,other"]
            elif kind == "interactive-demo":
                args += ["--example", kind]
            result = python(args, root)
            assert result[0] == 0, result
            (path / "state/.workspace.lock").unlink(missing_ok=True)
            for relative in ("work/sentinel", "output/sentinel"):
                (path / relative).write_bytes(b"user data\x00\xff\n")
            self.bases[kind] = path

    def run(self, category, label, callback):
        self.counts[category] += 1
        owned = set(CHILDREN)
        try:
            callback()
        except Exception as exc:
            self.failed[category] += 1
            self.failures.append(f"{category}/{label}: {type(exc).__name__}: {exc}")
        finally:
            for child in CHILDREN - owned:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGKILL)
                child.communicate(timeout=2)
                CHILDREN.discard(child)

    def fixture(self, kind="blank", active=False, pending=False):
        self.serial += 1
        # Unsafe shell-metadata roots have dedicated native-policy cases below;
        # ordinary byte parity must retain Python's unchanged manifest template.
        path = self.root / f"case-{self.serial} space \u03bb\nworkspace"
        shutil.copytree(self.bases[kind], path)
        if active or pending:
            revision = coven.read_task_snapshot(path)[0]
            result = python(["state", "import-tasks", path, "--expect-revision", revision, "--json"], path)
            assert result[0] == 0, result
            if pending:
                # A complete, unactivated epoch is a real interruption boundary.
                # Its manifest/frame hashes and all source bytes remain original.
                (path / ledger.MARKER).unlink()
            (path / "state/.workspace.lock").unlink(missing_ok=True)
        return path

    def candidate(self, args, cwd, env=None):
        if self.oracle_only:
            return python(args, cwd)
        return finish(spawn([self.binary, *args], cwd, env))

    def reject(self, w, args, parser=False, lock=False, semantic=None):
        before = files(w)
        actual = self.candidate(args, w)
        expected = python(args, w)
        assert actual[0] == expected[0] == (2 if parser else 1), (args, actual, expected)
        assert actual[1] == expected[1] == "", (actual, expected)
        assert actual[2] and expected[2], (actual, expected)
        if semantic:
            assert re.search(semantic, actual[2], re.I), actual
            assert re.search(semantic, expected[2], re.I), expected
        effects(before, files(w), {"state/.workspace.lock"} if lock else set())

    def inspect(self, w, pretty=False, implicit=False, bad_source=False):
        before = files(w)
        args = ["state", "inspect", *([] if implicit else [w]), *([] if pretty else ["--json"])]
        actual, expected = self.candidate(args, w), python(args, w)
        assert actual[0] == expected[0] == 0, (actual, expected)
        assert actual[2] == expected[2] == "", (actual, expected)
        a, b = json.loads(actual[1]), json.loads(expected[1])
        if bad_source:
            # Only diagnostic prose is language-specific. All other fields stay exact.
            assert isinstance(a["source_error"], str) and a["source_error"]
            assert isinstance(b["source_error"], str) and b["source_error"]
            a["source_error"] = b["source_error"]
        equal(a, b, "inspection")
        self.format(actual[1], pretty)
        assert files(w) == before, "inspect acquired a lock or changed files"

    @staticmethod
    def format(text, pretty):
        value = json.loads(text)
        if pretty:
            assert text in (json.dumps(value, indent=2, ensure_ascii=False) + "\n",
                            json.dumps(value, indent=2, ensure_ascii=True) + "\n"), text
        else:
            assert text.endswith("\n") and "\n" not in text[:-1], text

    def success(self, w, args, operation, pretty=False, safety_manifest=False):
        before = files(w)
        saved = self.root / f"pre-{self.serial}"
        clone(w, saved)
        location = actions.ActionStore(w).location()
        actual = self.candidate(args, w)
        assert actual[0] == 0 and actual[2] == "", (args, actual)
        after = files(w)
        report = None if operation == "refresh" else json.loads(actual[1])
        ids = []
        allowed = {"state/.workspace.lock"}
        sdk_dirs = set()
        if operation in ("import", "resume", "repair"):
            view = ledger.LedgerStore(w).read()  # Python validates native protocol, not just CLI JSON.
            assert view is not None
            equal(report, {**coven.task_inspection(w, view), "outcome": report["outcome"],
                           **({"recovery": report["recovery"]} if "recovery" in report else {}),
                           "warnings": []}, "inspection-shaped result")
            assert report["outcome"] == ("committed" if operation == "import" else "recovered")
            self.format(actual[1], pretty)
            allowed.add("dashboard.md")
            if operation == "import":
                ids = [identifier(view.workspace_id), identifier(view.epoch_id)]
                base = f"{ledger.LEDGER}/{view.epoch_id}"
                sdk_dirs.update({ledger.LEDGER, base, f"{base}/frames"})
                allowed.update(name for name in after if name == ledger.MARKER or
                               name == ledger.LEDGER or name.startswith(ledger.LEDGER + "/"))
            elif operation == "resume":
                allowed.add(ledger.MARKER)
                assert report["recovery"] == {"import_resumed": True}
            else:
                detail = report["recovery"]
                if detail["backup_path"] is None:
                    assert detail["status"] == "current"
                    assert {n: v for n, v in after.items() if n.startswith("state/task-recovery")} == {
                        n: v for n, v in before.items() if n.startswith("state/task-recovery")}, "current repair created/changed backup artifacts"
                else:
                    backup = Path(detail["backup_path"])
                    assert backup.parent == w / "state/task-recovery"
                    ids = [identifier(backup.name)]
                    sdk_dirs.update({"state/task-recovery", str(backup.relative_to(w))})
                    allowed.update(name for name in after if name == "state/task-recovery" or
                                   name.startswith(f"state/task-recovery/{backup.name}"))
                    allowed.add("state/tasks.json")
                    raw = before.get("state/tasks.json", (None, None, None))[2]
                    assert (backup / "metadata.json").read_bytes() == storage.encode_json({
                        "version": 1, "missing": raw is None, "revision": storage.snapshot_revision(raw)})
                    if raw is None:
                        assert not (backup / "tasks.bytes").exists()
                    else:
                        assert (backup / "tasks.bytes").read_bytes() == raw
                        assert stat.S_IMODE((backup / "tasks.bytes").stat().st_mode) == 0o600
            if "state/tasks.json" not in before:
                allowed.add("state/tasks.json")
            assert ledger.LedgerStore(w).projection_health(view)["status"] == "current"
            if "state/tasks.json" in before and operation in ("import", "resume"):
                assert after["state/tasks.json"] == before["state/tasks.json"], "source changed on activation"
        elif operation == "abort":
            assert set(report) == {"outcome", "authority", "preserved_artifacts"}
            assert report["outcome"] == "aborted" and report["authority"] == "snapshot-v1"
            archive = Path(report["preserved_artifacts"])
            assert archive.parent == w / "state"
            assert archive.name.startswith("task-ledger-aborted-")
            ids = [identifier(archive.name.removeprefix("task-ledger-aborted-"))]
            allowed.update(n for n in before if n == ledger.LEDGER or n.startswith(ledger.LEDGER + "/"))
            allowed.update(n for n in after if n == str(archive.relative_to(w)) or
                           n.startswith(str(archive.relative_to(w)) + "/"))
            for name, value in before.items():
                if name.startswith(ledger.LEDGER + "/"):
                    dest = name.replace(ledger.LEDGER, str(archive.relative_to(w)), 1)
                    assert after[dest] == value, "archive modified rather than renamed artifacts"
            self.format(actual[1], pretty)
        elif operation == "refresh":
            team = coven.load_team(w)
            allowed.update({"tmux/coven.toml", "dashboard.md"})
            allowed.update(f"prompts/{agent}.start.txt" for agent in coven.team_agents(team))
            allowed.update(f"prompts/{agent}.md" for agent in coven.team_agents(team)
                           if f"prompts/{agent}.md" not in before)
            allowed.update(n for n in ("prompts", "tmux") if n not in before)
            for name in allowed - {"dashboard.md", "state/.workspace.lock"}:
                if name in before and before[name][0] == "file":
                    assert after[name][3] == before[name][3], f"plain generated write replaced inode: {name}"
            if "dashboard.md" in before:
                assert after["dashboard.md"][3] != before["dashboard.md"][3], "dashboard must replace atomically"
        effects(before, after, allowed)

        # Replay at the exact original path, preserving native root/inodes while
        # the oracle occupies a temporary copy. Runtime control binds that root.
        native = self.root / f"native-{self.serial}"
        w.rename(native)
        saved.rename(w)
        try:
            with mock.patch.object(uuid, "uuid4", side_effect=ids), \
                    mock.patch.object(actions.ActionStore, "location", return_value=location):
                expected = python(args, w)
            assert expected[0] == 0 and expected[2] == "", expected
            if report is not None:
                equal(report, json.loads(expected[1]), "complete state output")
            else:
                assert actual == expected, (actual, expected)
            oracle = files(w)
            assert set(after) == set(oracle), "filesystem namespace differs from Python"
            manifest = "tmux/coven.toml"
            safe_manifest = (self.safe_manifest(w, after[manifest][2], oracle[manifest][2])
                             if safety_manifest else None)
            for name, value in after.items():
                if value[0] == "file":
                    if safety_manifest and name == manifest:
                        assert value[:2] == oracle[name][:2], "safe manifest type/mode"
                        assert value[2] == safe_manifest, "manifest changed beyond validated printf prefixes"
                    else:
                        assert value[:3] == oracle[name][:3], f"raw bytes/mode differ: {name}"
                elif value[0] == "dir":
                    if not self.oracle_only and name in sdk_dirs and name not in before:
                        # Only these exact newly created SDK directories request
                        # private access. Existing dirs and every other new dir
                        # keep Python's exact mode policy. No special bits other
                        # than the parent's inherited SGID are permitted.
                        parent = str(Path(name).parent)
                        expected_mode = (0o700 & ~self.umask) | (after[parent][1] & stat.S_ISGID)
                        assert oracle[name][0] == "dir", name
                        assert value[1] == expected_mode, f"SDK private directory mode: {name}: {value[1]:04o} != {expected_mode:04o}"
                    else:
                        assert value[:2] == oracle[name][:2], f"directory mode differs: {name}"
        finally:
            shutil.rmtree(w)
            native.rename(w)
        return report

    @staticmethod
    def safe_manifest(w, actual_raw, python_raw):
        """Permit only the intentionally safe first printf, never an entire command."""
        actual = tomllib.loads(actual_raw.decode())
        expected = tomllib.loads(python_raw.decode())
        windows = actual["sessions"][0]["windows"]
        old_windows = expected["sessions"][0]["windows"]
        assert len(windows) == len(old_windows)
        golden = python_raw.decode()
        count = 0
        for window, old_window in zip(windows, old_windows):
            if not window["name"].startswith("agent-"):
                continue
            agent = window["name"].removeprefix("agent-")
            command = window["panes"][0]["command"]
            old_command = old_window["panes"][0]["command"]
            message = f"Agent: {agent}\nPrompt: {w}/prompts/{agent}.md"
            data = [f"Agent: {agent}", f"Prompt: {w}/prompts/{agent}.md"]
            prefix = "printf '%s\\n' " + " ".join(map(shlex.quote, data))
            old_prefix = f"printf 'Agent: {agent}\\nPrompt: {w}/prompts/{agent}.md\\n'"
            assert command.startswith(prefix + "; "), (agent, command)
            tokens = shlex.split(command[:len(prefix)])
            assert tokens == ["printf", "%s\\n", *data], "metadata must be data, not format/program"
            assert "\n".join(tokens[2:]) + "\n" == message + "\n", "printed prompt/root bytes differ"
            assert old_command.startswith(old_prefix + "; ")
            assert command[len(prefix):] == old_command[len(old_prefix):], "agent command tail changed"
            # Syntax checking only: none of these generated commands is executed.
            syntax = finish(spawn(["/bin/sh", "-n", "-c", command], w))
            assert syntax == (0, "", ""), (agent, syntax)
            old_line = "command = " + coven.toml_string(old_command)
            new_line = "command = " + coven.toml_string(command)
            assert golden.count(old_line) == 1, agent
            golden = golden.replace(old_line, new_line, 1)
            count += 1
        assert count == len(coven.team_agents(coven.load_team(w))) and count > 0
        return golden.encode()

    def state_args(self, w, action, extra=(), pretty=False, implicit=False):
        return ["state", action, *([] if implicit else [w]), *extra,
                *([] if pretty else ["--json"])]

    def import_(self, w, pretty=False, implicit=False):
        return self.success(w, self.state_args(w, "import-tasks", ["--expect-revision",
                            coven.read_task_snapshot(w)[0]], pretty, implicit), "import", pretty)

    def repair(self, w, pretty=False):
        return self.success(w, self.state_args(w, "recover-tasks", ["--repair-projection"], pretty),
                            "repair", pretty)

    def task(self, w, action, items, key=None, pin=None):
        """Real native mutations; validate frame bytes, frozen result and Python replay."""
        before = files(w)
        saved = self.root / f"task-pre-{self.serial}"
        clone(w, saved)
        location = actions.ActionStore(w).location()
        args = ["task", action, w, *items, "--json"]
        if key:
            args += ["--request-id", key]
        if pin:
            args += ["--expect-revision", pin]
        result = self.candidate(args, w)
        assert result[0] == 0 and result[2] == "", result
        report = json.loads(result[1])
        view = ledger.LedgerStore(w).read()
        if view is not None:
            frozen = report["result"]
            equal(view.receipts[key]["result"], frozen, "Python readback receipt")
            frame = f"{ledger.LEDGER}/{view.epoch_id}/frames/{view.sequence:020d}.frame"
            raw = (w / frame).read_bytes()
            assert raw == ledger._envelope(ledger._payload(raw)), "frame wire bytes changed"
            allowed = {frame}
        else:
            frozen, allowed = report, set()
        identifier(frozen["mutation_id"].replace("-", ""))
        assert STAMP.fullmatch(frozen["task"]["updated_at"])
        allowed.update({"state/tasks.json", "dashboard.md", "logs/events.jsonl", "state/.workspace.lock"})
        after = files(w)
        effects(before, after, allowed)
        assert (w / "dashboard.md").read_bytes() == coven.render_dashboard(w).encode()
        event_raw = after["logs/events.jsonl"][2][len(before["logs/events.jsonl"][2]):]
        assert len(event_raw.splitlines()) == 1, event_raw
        event = json.loads(event_raw)
        event_id = uuid.UUID(event["event_id"])
        assert str(event_id) == event["event_id"] and event_id.version == 4
        assert STAMP.fullmatch(event["ts"])
        task = frozen["task"]
        clocks = ([task["created_at"], task["updated_at"]] if action == "add" else [task["updated_at"]])
        if view is None:
            clocks *= 2  # Snapshot writers stage once, then prepare again under the lock.
        clocks += [event["ts"]]
        native = self.root / f"task-native-{self.serial}"
        w.rename(native)
        saved.rename(w)
        try:
            with mock.patch.object(uuid, "uuid4", side_effect=[uuid.UUID(frozen["mutation_id"]), event_id]), \
                    mock.patch.object(coven, "now", side_effect=clocks), \
                    mock.patch.object(actions.ActionStore, "location", return_value=location):
                oracle = python(args, w)
            assert oracle[0] == 0 and oracle[2] == "", oracle
            equal(report, json.loads(oracle[1]), "full fresh mutation envelope")
            expected_files = files(w)
            assert set(after) == set(expected_files), "task filesystem namespace"
            for name, value in after.items():
                if value[0] == "file":
                    assert value[:3] == expected_files[name][:3], f"fresh mutation raw bytes/mode: {name}"
        finally:
            shutil.rmtree(w)
            native.rename(w)
        if key:
            retry_before = files(w)
            retry = self.candidate(args, w)
            oracle = python(args, w)
            assert retry[0] == oracle[0] == 0, (retry, oracle)
            equal(json.loads(retry[1]), json.loads(oracle[1]), "native request replay")
            assert files(w) == retry_before, "request replay wrote state"
        return report

    def state_matrix(self):
        for pretty in (False, True):
            for implicit in (False, True):
                for backend in ("snapshot", "active", "pending"):
                    def case(pretty=pretty, implicit=implicit, backend=backend):
                        w = self.fixture(active=backend == "active", pending=backend == "pending")
                        self.inspect(w, pretty, implicit)
                    self.run("inspect", f"{backend}/{pretty}/{implicit}", case)
                for missing in (False, True):
                    def case(pretty=pretty, implicit=implicit, missing=missing):
                        w = self.fixture()
                        source = w / "state/tasks.json"
                        if missing:
                            source.unlink()
                        else:
                            source.write_bytes(b' [ {"id":"seed","title":"Raw \\u03bb","status":"custom","extra":{"b":1.0,"a":-0.0}} ]\r\n')
                            source.chmod(0o640)
                        self.import_(w, pretty, implicit)
                    self.run("import", f"{missing}/{pretty}/{implicit}", case)
        for pretty in (False, True):
            for raw in (None, b"", b"\xff\x00custom\n", b"{broken", b"[]\n", b"current"):
                def case(raw=raw, pretty=pretty):
                    w = self.fixture(active=True)
                    path = w / "state/tasks.json"
                    if raw is None:
                        path.unlink()
                    elif raw != b"current":
                        path.write_bytes(raw)
                        path.chmod(0o640)
                    self.repair(w, pretty)
                self.run("repair", f"{raw!r}/{pretty}", case)
            for action in ("resume", "abort"):
                for corrupt in (False, True):
                    def case(action=action, corrupt=corrupt, pretty=pretty):
                        w = self.fixture(pending=True)
                        if corrupt:
                            (w / "state/tasks.json").write_bytes(b"\xffbad source")
                            self.inspect(w, pretty, bad_source=True)
                        extra = ["--" + action + "-import"]
                        if action == "resume":
                            extra += ["--expect-revision", "tasks-sha256:" + "0" * 64 if corrupt else coven.read_task_snapshot(w)[0]]
                        args = self.state_args(w, "recover-tasks", extra, pretty)
                        if corrupt and action == "resume":
                            self.reject(w, args)
                        else:
                            self.success(w, args, action, pretty)
                    self.run("pending", f"{action}/{corrupt}/{pretty}", case)
        for relative in ("state/agents.json", "state/lead.json"):
            def case(relative=relative):
                w = self.fixture()
                (w / relative).write_bytes(b"invalid content")
                self.inspect(w)
            self.run("inspect", "no record parse/" + relative, case)

    def rejection_matrix(self):
        parsers = [(["state"], "state"), (["state", "import-tasks"], "missing pin"),
                   (["state", "recover-tasks"], "missing mode"),
                   (["state", "inspect", ".", "."], "extra workspace"),
                   (["refresh", "--json"], "refresh JSON")]
        modes = ["--repair-projection", "--resume-import", "--abort-import"]
        for i, mode in enumerate(modes):
            for other in modes[i + 1:]:
                parsers.append((["state", "recover-tasks", mode, other], mode + other))
        for args, label in parsers:
            self.run("parser", label, lambda args=args: self.reject(self.fixture(), args, parser=True))
        for backend in ("snapshot", "active", "pending"):
            for mode in modes:
                for pin in (None, "bad", "tasks-missing:v1"):
                    def case(backend=backend, mode=mode, pin=pin):
                        w = self.fixture(active=backend == "active", pending=backend == "pending")
                        extra = [mode] + ([] if pin is None else ["--expect-revision", pin])
                        # The sole successful combination has its own raw archive test.
                        if backend == "pending" and mode == "--abort-import" and pin is None:
                            self.success(w, self.state_args(w, "recover-tasks", extra), "abort")
                        elif backend == "active" and mode == "--repair-projection" and pin is None:
                            self.repair(w)
                        else:
                            self.reject(w, self.state_args(w, "recover-tasks", extra), lock=True)
                    self.run("recovery-preflight", f"{backend}/{mode}/{pin}", case)
        for backend in ("snapshot", "active", "pending"):
            for pin in ("bad", "tasks-missing:v1", "tasks-sha256:" + "0" * 64):
                def case(backend=backend, pin=pin):
                    w = self.fixture(active=backend == "active", pending=backend == "pending")
                    self.reject(w, self.state_args(w, "import-tasks", ["--expect-revision", pin]))
                self.run("import-preflight", f"{backend}/{pin}", case)
        for unsafe in ("symlink", "directory", "fifo"):
            def case(unsafe=unsafe):
                w = self.fixture(active=True)
                path = w / "state/tasks.json"
                path.unlink()
                if unsafe == "symlink":
                    path.symlink_to(w / "output/sentinel")
                elif unsafe == "directory":
                    path.mkdir()
                else:
                    os.mkfifo(path)
                self.reject(w, self.state_args(w, "recover-tasks", ["--repair-projection"]), lock=True)
                self.success(w, ["refresh", w], "refresh")
            self.run("unsafe-projection", unsafe, case)
        for command in ("inspect", "import", "abort", "refresh"):
            for bad in ("config", "control", "known-path"):
                def case(command=command, bad=bad):
                    w = self.fixture(pending=command == "abort")
                    if bad == "config":
                        (w / "coven.json").write_bytes(b"{}")
                    elif bad == "control":
                        (w / actions.CONTROL).write_bytes(b"{}")
                    else:
                        (w / "goal.md").unlink(missing_ok=True)
                        (w / "goal.md").symlink_to(w / "output/sentinel")
                    args = (["refresh", w] if command == "refresh" else
                            self.state_args(w, "inspect" if command == "inspect" else
                                            "import-tasks" if command == "import" else "recover-tasks",
                                            ["--expect-revision", "bad"] if command == "import" else
                                            ["--abort-import"] if command == "abort" else []))
                    self.reject(w, args)
                self.run("diagnostic-preflight", command + "/" + bad, case)

    def authority_edges(self):
        for relative in ("state/tasks.json", "state/agents.json", "state/lead.json"):
            for action in ("import", "resume", "repair"):
                def case(relative=relative, action=action):
                    w = self.fixture(pending=action == "resume")
                    (w / relative).write_bytes(b"{invalid")
                    args = self.state_args(w, "import-tasks" if action == "import" else "recover-tasks",
                                           ([] if action == "import" else ["--" + action + "-import"]
                                            if action == "resume" else ["--repair-projection"]) +
                                           ["--expect-revision", "bad"])
                    # Full record loading must precede malformed-pin semantics.
                    self.reject(w, args, semantic=r"record|snapshot|task|agent|lead|json")
                    if relative != "state/tasks.json":
                        self.inspect(w)
                self.run("ordered-load", action + "/" + relative, case)
        for relative in ("state/tasks.json", "state/agents.json", "state/lead.json", "state/execution/control.json"):
            def case(relative=relative):
                w = self.fixture(pending=True)
                (w / relative).parent.mkdir(exist_ok=True, parents=True)
                (w / relative).write_bytes(b"\xffnot parsed by abort")
                self.success(w, self.state_args(w, "recover-tasks", ["--abort-import"]), "abort")
            self.run("abort-narrow-content", relative, case)
        for action in ("import", "repair", "resume"):
            def case(action=action):
                w = self.fixture(pending=action == "resume")
                token = "tasks-ledger:v1:" + "0" * 32 + ":" + "1" * 32 + ":0:" + "2" * 64
                extra = ([] if action == "import" else ["--repair-projection"] if action == "repair" else ["--resume-import"])
                self.reject(w, self.state_args(w, "import-tasks" if action == "import" else "recover-tasks",
                                               extra + ["--expect-revision", token]))
            self.run("snapshot-pin-only", action, case)
        for corruption in ("marker", "manifest", "baseline", "final-frame"):
            def case(corruption=corruption):
                w = self.fixture(active=True)
                marker = json.loads((w / ledger.MARKER).read_bytes())
                base = w / ledger.LEDGER / marker["epoch_id"]
                if corruption == "marker":
                    target = w / ledger.MARKER
                elif corruption == "manifest":
                    target = base / "manifest.json"
                elif corruption == "baseline":
                    target = base / "baseline.tasks.bytes"
                else:
                    self.task(w, "add", ["Committed frame"], "corrupt-after")
                    target = base / "frames/00000000000000000001.frame"
                target.write_bytes(b"broken authority")
                self.reject(w, self.state_args(w, "inspect"))
                self.reject(w, self.state_args(w, "recover-tasks", ["--repair-projection"]))
            self.run("authority-no-fallback", corruption, case)
        for incomplete in ("missing-manifest", "missing-backup", "changed-source", "two-epochs"):
            def case(incomplete=incomplete):
                w = self.fixture(pending=True)
                pin = coven.read_task_snapshot(w)[0]
                base = next((w / ledger.LEDGER).iterdir())
                if incomplete == "missing-manifest":
                    (base / "manifest.json").unlink()
                elif incomplete == "missing-backup":
                    (base / "baseline.tasks.bytes").unlink()
                elif incomplete == "changed-source":
                    (w / "state/tasks.json").write_bytes(b" [] \n")
                else:
                    shutil.copytree(base, base.parent / ("f" * 32))
                self.reject(w, self.state_args(w, "recover-tasks", ["--resume-import", "--expect-revision", pin]), lock=True)
                self.success(w, self.state_args(w, "recover-tasks", ["--abort-import"]), "abort")
            self.run("pending-artifacts", incomplete, case)
        def abort_pin_first():
            w = self.fixture(pending=True)
            (w / "coven.json").write_bytes(b"invalid config")
            self.reject(w, self.state_args(w, "recover-tasks", ["--abort-import", "--expect-revision", "bad"]),
                        semantic=r"expect.revision|resume.import")
        self.run("ordered-load", "abort pin before diagnostics", abort_pin_first)
        for raw in (b"", b"\xff", b"{}", b"[null]", b'[{"id":"x"}]'):
            def case(raw=raw):
                w = self.fixture()
                (w / "state/tasks.json").write_bytes(raw)
                self.reject(w, self.state_args(w, "inspect"))
                self.reject(w, self.state_args(w, "import-tasks", ["--expect-revision", storage.snapshot_revision(raw)]))
            self.run("invalid-source", repr(raw), case)

    def history(self):
        w = self.fixture()
        self.task(w, "add", ["Before import"])
        self.import_(w)
        revision = coven.read_task_snapshot(w)[0]
        first = self.task(w, "add", ["Authoritative", "--owner", "OPERATOR"], "add-real", revision)
        task_id = first["result"]["task"]["id"]
        self.task(w, "assign", [task_id, "other"], "assign-real")
        source = w / "state/tasks.json"
        source.write_bytes(b"uncommitted raw projection\xff")
        before = files(w)
        args = ["task", "add", w, "Authoritative", "--owner", "OPERATOR", "--request-id", "add-real",
                "--expect-revision", revision, "--json"]
        actual = self.candidate(args, w)
        expected = python(args, w)
        assert actual[0] == expected[0] == 0, (actual, expected)
        equal(json.loads(actual[1]), json.loads(expected[1]), "diverged frozen receipt")
        equal(json.loads(actual[1])["result"], first["result"], "original receipt after assignment")
        assert files(w) == before
        self.reject(w, ["task", "add", w, "Blocked", "--request-id", "fresh", "--json"], lock=True)
        self.repair(w)
        self.repair(w)  # Confirm a current head without allocating a second backup.
        self.task(w, "add", ["After repair"], "fresh")
        self.success(w, ["refresh", w], "refresh")

    def refresh_matrix(self):
        for kind in self.bases:
            for variant in ("normal", "missing-role", "binary-role", "custom-modes", "missing-guidance", "missing-dirs",
                            "pending", "active", "legacy-decision", "revoked-decision"):
                def case(kind=kind, variant=variant):
                    w = self.fixture(kind, active=variant == "active", pending=variant == "pending")
                    team = json.loads((w / "coven.json").read_bytes())
                    agent = team["agents"][0]
                    if variant == "missing-role":
                        (w / f"prompts/{agent}.md").unlink()
                    elif variant == "binary-role":
                        (w / f"prompts/{agent}.md").write_bytes(b"authored binary \xff\x00")
                    elif variant == "custom-modes":
                        for relative in ("tmux/coven.toml", "dashboard.md", f"prompts/{agent}.md",
                                         f"prompts/{agent}.start.txt", "logs/events.jsonl", "logs/messages.jsonl"):
                            (w / relative).chmod(0o640)
                    elif variant == "missing-guidance":
                        for relative in ("goal.md", "spec.md", "notes.md", "work/README.md", "output/README.md",
                                         "checkpoints/plan-approval.md", "checkpoints/pre-delivery-review.md"):
                            (w / relative).unlink(missing_ok=True)
                    elif variant == "missing-dirs":
                        shutil.rmtree(w / "prompts")
                        shutil.rmtree(w / "tmux")
                    elif variant in ("legacy-decision", "revoked-decision"):
                        checkpoint = w / "checkpoints/plan-approval.md"
                        if variant == "legacy-decision":
                            checkpoint.write_text("# Custom\nStatus: approved\n## User decision\n- [x] Approved\n")
                        else:
                            assert python(["approve", w, "plan-approval"], w)[0] == 0
                            assert python(["suggest", w, "plan-approval", "Need evidence"], w)[0] == 0
                        (w / "state/.workspace.lock").unlink(missing_ok=True)
                    self.success(w, ["refresh", w], "refresh")
                    self.success(w, ["refresh", w], "refresh")
                self.run("refresh", kind + "/" + variant, case)
        for command in ("oc", " oc --model 'x y' ", "opencode --flag", "false; touch NEVER_EXECUTED"):
            def case(command=command):
                w = self.fixture("default")
                team = json.loads((w / "coven.json").read_bytes())
                team.update(name="Named ' \" \u03bb\nroot", opencode_command=command)
                (w / "coven.json").write_bytes(storage.encode_json(team))
                self.success(w, ["refresh", w], "refresh")
                assert not (w / "NEVER_EXECUTED").exists()
            self.run("refresh-command-data", command, case)
        for log in ("messages", "events"):
            for value in (None, True, 7, "../escape", "bad/name", "missing-custom", "custom"):
                def case(log=log, value=value):
                    w = self.fixture()
                    record = {"type": "not-a-request", "checkpoint": value}
                    (w / f"logs/{log}.jsonl").write_bytes(storage.encode_json(record).replace(b"\n", b"") + b"\n")
                    if value == "custom":
                        # Custom references require valid UTF-8, not decision grammar.
                        (w / "checkpoints/custom.md").write_text("Status: approved\nStatus: pending\n")
                    if value in ("custom", "missing-custom"):
                        self.success(w, ["refresh", w], "refresh")
                    else:
                        self.reject(w, ["refresh", w])
                self.run("refresh-log-fields", f"{log}/{value!r}", case)
        for failure in ("unknown-example", "bad-messages", "bad-events", "bad-reference-utf8", "builtin-grammar"):
            def case(failure=failure):
                w = self.fixture()
                if failure == "unknown-example":
                    team = json.loads((w / "coven.json").read_bytes())
                    team["example"] = "not-bundled-example"
                    (w / "coven.json").write_bytes(storage.encode_json(team))
                elif failure.startswith("bad-") and failure.endswith(("messages", "events")):
                    (w / ("logs/" + failure[4:] + ".jsonl")).write_bytes(b"broken\n")
                elif failure == "bad-reference-utf8":
                    (w / "logs/events.jsonl").write_text('{"type":"ignored","checkpoint":"custom"}\n')
                    (w / "checkpoints/custom.md").write_bytes(b"\xff")
                else:
                    (w / "checkpoints/plan-approval.md").write_text("Status: pending\nStatus: approved\n")
                self.reject(w, ["refresh", w])
            self.run("refresh-preflight", failure, case)

    def runtime_control(self):
        for action in ("inspect", "import", "repair", "resume", "abort", "refresh"):
            def case(action=action):
                w = self.fixture(active=action == "repair", pending=action in ("resume", "abort"))
                result = python(["action", "enable-runtime-launch", w, "--json"], w)
                assert result[0] == 0, result
                (w / "state/.workspace.lock").unlink(missing_ok=True)
                if action == "inspect":
                    self.inspect(w)
                elif action == "import":
                    self.import_(w)
                elif action == "repair":
                    self.repair(w)
                elif action == "refresh":
                    self.success(w, ["refresh", w], "refresh")
                else:
                    extra = ["--" + action + "-import"]
                    if action == "resume":
                        extra += ["--expect-revision", coven.read_task_snapshot(w)[0]]
                    self.success(w, self.state_args(w, "recover-tasks", extra), action)
            self.run("runtime-control", action, case)

    def native_profiles(self):
        if self.oracle_only:
            return
        def default_command():
            w = self.fixture()
            env = dict(ENV)
            env.pop("COVEN_COMMAND")
            before = files(w)
            actual = self.candidate(["refresh", w], w, env)
            assert actual[0] == 0 and actual[2] == "", actual
            with mock.patch.dict(os.environ, {"COVEN_COMMAND": "coven"}):
                draft = coven.prepare_refresh(w)
            assert (w / "tmux/coven.toml").read_bytes() == draft[0].encode()
            for relative, text in {**draft[1], **draft[2]}.items():
                assert (w / relative).read_bytes() == text.encode()
            assert (w / "dashboard.md").read_bytes() == draft[3].encode()
            effects(before, files(w), {"state/.workspace.lock", "tmux/coven.toml", "dashboard.md"} |
                    set(draft[1]) | set(draft[2]))
        self.run("native-profile", "canonical default command", default_command)
        def init_unknown_flag():
            w = self.fixture()
            before = files(w)
            result = self.candidate(["init", w, "--json"], w)
            assert result[0] == 2 and result[1] == "" and result[2], result
            assert files(w) == before
        self.run("native-profile", "init rejects unknown JSON flag without effects", init_unknown_flag)
        for profile in ("raw-backup-32MiB", "text-1MiB"):
            def case(profile=profile):
                w = self.fixture(active=profile == "raw-backup-32MiB")
                target = w / ("state/tasks.json" if profile == "raw-backup-32MiB" else "goal.md")
                with target.open("wb") as handle:
                    handle.truncate((32 if profile == "raw-backup-32MiB" else 1) * 1024 * 1024 + 1)
                args = self.state_args(w, "recover-tasks", ["--repair-projection"]) if profile == "raw-backup-32MiB" else ["refresh", w]
                before = files(w)
                result = self.candidate(args, w)
                assert result[0] == 1 and result[1] == "" and result[2], result
                effects(before, files(w), {"state/.workspace.lock"})
            self.run("native-profile", profile, case)

    def native_safety(self):
        if self.oracle_only:
            return
        # Python's force write would truncate the authored alias. Do not invoke
        # that destructive oracle on these native refusal fixtures.
        for generated, authored in (("prompts/operator.start.txt", "goal.md"),
                                    ("tmux/coven.toml", "prompts/operator.md"),
                                    ("prompts/other.start.txt", "state/tasks.json")):
            def case(generated=generated, authored=authored):
                w = self.fixture()
                if authored == "prompts/operator.md":
                    (w / authored).write_bytes(b"# Custom role\nKeep all authored bytes.\n")
                target, source = w / generated, w / authored
                target.unlink()
                os.link(source, target)
                assert source.stat().st_ino == target.stat().st_ino
                assert source.stat().st_nlink == 2
                (w / "state/.workspace.lock").unlink(missing_ok=True)
                before = files(w)
                result = self.candidate(["refresh", w], w)
                assert result[0] == 1 and result[1] == "", result
                assert "unsafe_generated_hardlink" in result[2], result
                assert files(w) == before, "hardlink refusal wrote data/lock or changed inventory metadata"
                assert source.stat().st_ino == target.stat().st_ino and source.stat().st_nlink == 2
            self.run("NativeSafetyRefusal", generated + " -> " + authored, case)
        def existing_role():
            w = self.fixture()
            source = w / "work/custom-role"
            source.write_bytes(b"Authored role with binary data \xff\x00\n")
            role = w / "prompts/operator.md"
            role.unlink()
            os.link(source, role)
            before = files(w)
            self.success(w, ["refresh", w], "refresh")
            after = files(w)
            assert after["work/custom-role"] == before["work/custom-role"]
            assert after["prompts/operator.md"] == before["prompts/operator.md"]
            assert source.stat().st_ino == role.stat().st_ino and source.stat().st_nlink == 2
        self.run("NativeSafetyPreservation", "existing hardlinked role is not force-written", existing_role)
        for suffix in ("apostrophe'root", "percent%s%nroot", r"backslash\n\croot"):
            def case(suffix=suffix):
                w = self.fixture()
                unsafe = w.with_name(w.name + " " + suffix)
                w.rename(unsafe)
                self.success(unsafe, ["refresh", unsafe], "refresh", safety_manifest=True)
            self.run("NativeSafetyGeneration", suffix, case)

    def locks(self):
        if self.oracle_only:
            return
        def inspect_without_lock():
            w = self.fixture()
            with coven.workspace_mutation(w):
                before = files(w)
                result = self.candidate(["state", "inspect", w, "--json"], w)
                assert result[0] == 0, result
                assert files(w) == before
        self.run("mixed-lock", "inspect bypasses held writer lock", inspect_without_lock)
        for action in ("refresh", "repair", "resume", "abort"):
            def case(action=action):
                if action == "refresh" and (sys.platform != "linux" or
                                            not Path("/proc/self/fd").exists()):
                    print("refresh serialization: skipped (requires Linux /proc handshake)")
                    return
                w = self.fixture(active=action == "repair", pending=action in ("resume", "abort"))
                args = ["refresh", w] if action == "refresh" else self.state_args(w, "recover-tasks", [
                    "--repair-projection" if action == "repair" else "--" + action + "-import",
                    *(["--expect-revision", coven.read_task_snapshot(w)[0]] if action == "resume" else [])])
                with coven.workspace_mutation(w):
                    before = files(w)
                    child = spawn([self.binary, *args], w)
                    if action == "refresh":
                        lock = (w / "state/.workspace.lock").stat()
                        deadline = time.monotonic() + 3
                        while True:
                            assert child.poll() is None, "refresh exited while Python held flock"
                            opened = False
                            for fd in Path(f"/proc/{child.pid}/fd").iterdir():
                                try:
                                    meta = fd.stat()
                                    opened |= (meta.st_dev, meta.st_ino) == (lock.st_dev, lock.st_ino)
                                except FileNotFoundError:
                                    pass
                            wait = Path(f"/proc/{child.pid}/wchan").read_text()
                            if opened and ("sleep" in wait or "lock" in wait):
                                break
                            assert time.monotonic() < deadline, f"no blocked-flock handshake: {wait}"
                            time.sleep(0.005)
                    else:
                        time.sleep(0.08)
                    assert child.poll() is None, "native command did not wait for Python lock"
                    assert files(w) == before, "native wrote before lock release"
                    if action == "refresh":
                        # The native unlocked draft is complete before its lock
                        # descriptor opens. Commit new state under the parent lock.
                        fresh = storage.encode_json([{
                            "id": "after-lock", "title": "Task committed while refresh waited",
                            "status": "pending"}])
                        (w / "state/tasks.json").write_bytes(fresh)
                result = finish(child)
                assert result[0] == 0, result
                assert files(w)["state/.workspace.lock"] == before["state/.workspace.lock"]
                if action == "refresh":
                    assert (w / "state/tasks.json").read_bytes() == fresh
                    dashboard = (w / "dashboard.md").read_bytes()
                    assert b"Task committed while refresh waited" in dashboard, "stale unlocked dashboard"
                    assert dashboard == coven.render_dashboard(w).encode()
            self.run("mixed-lock", "Python holds/native " + action, case)
        # Native blocks on Python's inode; queued Python task blocks on that same
        # inode while native owns it. The parent owns and reaps every process group.
        def reverse():
            w = self.fixture()
            code = ("import sys; from runner import coven; print('ready',flush=True); "
                    "raise SystemExit(coven.main(sys.argv[1:]))")
            with coven.workspace_mutation(w):
                native = spawn([self.binary, "refresh", w], w)
                time.sleep(0.05)
                py = spawn([str(Path(sys.executable).resolve()), "-B", "-c", code,
                            "task", "add", w, "Queued Python task", "--json"], ROOT)
                assert select.select([py.stdout], [], [], 2)[0], "Python not ready"
                assert py.stdout.readline() == b"ready\n"
                time.sleep(0.05)
                assert native.poll() is None and py.poll() is None
                inode = (w / "state/.workspace.lock").stat().st_ino
            assert finish(native)[0] == 0
            result = finish(py)
            assert result[0] == 0, result
            assert (w / "state/.workspace.lock").stat().st_ino == inode
            assert any(t["title"] == "Queued Python task" for t in coven.read_tasks(w))
        self.run("mixed-lock", "queued native refresh/Python task", reverse)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    binary = str(Path(args.binary).resolve()) if args.binary else None
    started = time.monotonic()
    def expired(*_):
        raise CorpusTimeout("state/refresh corpus exceeded 58 seconds")
    previous = signal.signal(signal.SIGALRM, expired)
    signal.alarm(58)
    try:
        with tempfile.TemporaryDirectory(prefix="state-refresh-") as directory, \
                mock.patch.dict(os.environ, ENV, clear=True), \
                mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("runtime subprocess.run")), \
                mock.patch.object(coven, "send_to_agent_pane", side_effect=AssertionError("runtime send")), \
                mock.patch.object(coven, "agent_runtime_status", side_effect=AssertionError("runtime status")):
            suite = Suite(Path(directory), binary, args.oracle_only)
            suite.state_matrix()
            suite.rejection_matrix()
            suite.authority_edges()
            suite.run("end-to-end", "native task/import/replay/diverge/repair/fresh/refresh", suite.history)
            suite.refresh_matrix()
            suite.runtime_control()
            suite.native_profiles()
            suite.native_safety()
            suite.locks()
            print(json.dumps({"cases": sum(suite.counts.values()), "categories": suite.counts,
                              "failures": sum(suite.failed.values()), "failure_categories": suite.failed,
                              "oracle_only": args.oracle_only, "seconds": round(time.monotonic() - started, 3),
                              "gaps": ["private SDK fault/race cases owned by module units",
                                       "native-lock-held reverse probe requires private SDK barrier",
                                       "aggregate refresh stage bounds owned by refresh unit tests"]}))
            for failure in suite.failures:
                print(failure, file=sys.stderr)
            return bool(suite.failures)
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, previous)
        for child in tuple(CHILDREN):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=2)
            CHILDREN.discard(child)


if __name__ == "__main__":
    raise SystemExit(main())
