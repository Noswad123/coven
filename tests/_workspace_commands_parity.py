"""Five real CLI commands, byte/effect oracle, no production hooks or runtime.

Run --oracle-only before wiring. Successful stdout is literal UTF-8, including
terminal controls and exact LF, NOT the task-summary sanitizer policy. Replay at
the same absolute root freezes ONLY newly appended message/event clocks and event
UUIDs. No body, goal, legacy field, root-looking text or numeric value is scrubbed.
Parser usage and OS/JSON diagnostic prose are implementation-specific; status,
empty stdout, warning counts and complete failure effects are still protected.

Explicit native profiles: strict JSONL 8 MiB/log and 65536 physical LF (including
blank lines); raw history separately admits 32 MiB of literal UTF-8 text without
JSONL validation. Goal is 1 MiB, captured output 32 MiB, finite JSON 8 MiB/depth 64.
Python is unbounded
in these dimensions: over-profile cases are native refusals, not oracle parity.
Finite values also retain 1 MiB strings, 65536 items, 16384 members, 262144 nodes,
8192-byte number tokens and 4300-digit integers (Python has the same integer cap).
Private partial-write/fsync faults, goal independent callbacks, message failure
after visible append, separate lock epochs, status reread races and deterministic
under-lock revalidation belong to workspace_commands/logs service unit tests.
This corpus has no native failpoint and does not claim power-loss coverage.
The oversized-output-source case is rejected by source admission first; isolated
32 MiB output-reservation and argv growth tests remain service-owned.
Native-only stdout-io cases cover actual closed pipes, not command semantics or
Python's implementation-specific interpreter-cleanup exit status.
"""

import argparse
from collections import Counter
import contextlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from unittest import mock
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.set_int_max_str_digits(4300)
from runner import actions, coven, storage, task_ledger

PYTHON = str(Path(sys.executable).resolve())
STAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?\+00:00\Z")
COMMANDS = ("goal", "message", "messages", "events", "status")
LITERAL = "literal \u03bb \U0001f98a e\u0301\t\r\n\x1b[31m\x00\x7f\x85\u2028 2000-01-01T00:00:00+00:00 00000000-0000-4000-8000-000000000000"
ARG_LITERAL = LITERAL.replace("\x00", "")  # POSIX argv cannot contain NUL; files can.
CHILDREN = set()


class CorpusTimeout(BaseException):
    """A deadline must not become a collected assertion and silently expire."""


ORACLE_CODE = f"""
import sys, subprocess
sys.path.insert(0, {str(ROOT)!r})
from runner import coven
def forbidden(*a, **kw):
    raise AssertionError('workspace corpus attempted runtime/provider subprocess')
subprocess.run = forbidden
subprocess.Popen = forbidden
raise SystemExit(coven.main(sys.argv[1:]))
"""


def python(args, cwd):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.chdir(cwd), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err), \
            mock.patch.object(subprocess, "run", side_effect=AssertionError("Python runtime forbidden")), \
            mock.patch.object(subprocess, "Popen", side_effect=AssertionError("Python subprocess forbidden")):
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
        except (OSError, ValueError) as exc:
            # Python's executable reports these via traceback; only diagnostic
            # prose differs. Do not swallow assertions or unexpected exceptions.
            code = 1
            print(f"{type(exc).__name__}: {exc}", file=sys.stderr)
    return code, out.getvalue().encode("utf-8"), err.getvalue().encode("utf-8")


def files(root):
    result = {}
    for path in root.rglob("*"):
        s = path.lstat()
        kind = "link" if path.is_symlink() else "dir" if path.is_dir() else "file" if path.is_file() else "special"
        data = os.readlink(path) if kind == "link" else path.read_bytes() if kind == "file" else None
        result[str(path.relative_to(root))] = (kind, stat.S_IMODE(s.st_mode), data,
                                              s.st_ino, s.st_mtime_ns, s.st_nlink)
    return result


def stop_children():
    for child in tuple(CHILDREN):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
        child.communicate(timeout=3)
        CHILDREN.discard(child)


def finish(child):
    try:
        out, err = child.communicate(timeout=12)
        # Strict decoding, never replacement or universal-newline subprocess IO.
        out.decode("utf-8")
        err.decode("utf-8")
        return child.returncode, out, err
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=3)
        CHILDREN.discard(child)


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts, self.failed, self.failures = Counter(), Counter(), []
        self.serial = 0
        self.home = root / "home"
        self.home.mkdir()
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("COVEN_", "XDG_", "KITSUNE_", "HERDR_", "TMUX"))}
        self.env.update(PATH="", HOME=str(self.home), COVEN_MULTIPLEXER="",
                        COVEN_COMMAND="coven", PYTHONDONTWRITEBYTECODE="1",
                        PYTHONINTMAXSTRDIGITS="4300", PYTHONIOENCODING="utf-8")
        for key in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            self.env[f"XDG_{key}_" + ("DIR" if key == "RUNTIME" else "HOME")] = str(self.home / key.lower())
        self.base = root / "base"
        result = python(["init", self.base, "--blank", "--agents", "worker,other"], root)
        assert result[0] == 0, result
        (self.base / "state/.workspace.lock").unlink(missing_ok=True)
        (self.base / "work/sentinel").write_bytes(b"user data\x00\xff\n")
        (self.base / "output/sentinel").write_bytes(b"do not regenerate\n")
        (self.base / "goal.md").write_bytes(b"# Goal\r\n\r\nold\rgoal\n")

    def spawn(self, command, cwd):
        child = subprocess.Popen(list(map(os.fspath, command)), cwd=cwd, env=self.env,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, start_new_session=True)
        CHILDREN.add(child)
        return child

    def prefix(self, oracle=False):
        return [PYTHON, "-c", ORACLE_CODE] if oracle or self.oracle_only else [self.binary]

    def candidate(self, args, cwd):
        return finish(self.spawn([*self.prefix(), *args], cwd))

    def fixture(self, backend="snapshot"):
        self.serial += 1
        w = self.home / f"case-{self.serial} space \u03bb"
        shutil.copytree(self.base, w)
        if backend != "snapshot":
            revision = coven.read_task_snapshot(w)[0]
            result = python(["state", "import-tasks", w, "--expect-revision", revision, "--json"], w)
            assert result[0] == 0, result
            if backend == "pending":
                (w / task_ledger.MARKER).unlink()
            (w / "state/.workspace.lock").unlink(missing_ok=True)
        return w

    def run(self, category, label, callback):
        self.counts[category] += 1
        try:
            callback()
        except Exception as exc:
            self.failed[category] += 1
            self.failures.append(f"{category}/{label}: {type(exc).__name__}: {str(exc)[:1800]}")
        finally:
            stop_children()

    @staticmethod
    def transition(before, after):
        """Compare identity/time preservation, not unrelated cloned inode numbers."""
        result = {}
        for name, value in after.items():
            old = before.get(name)
            result[name] = (*value[:3], value[5],
                            old is not None and old[3] == value[3],
                            old is not None and old[4] == value[4])
        return result

    def parity(self, w, args, code=0, allowed=(), stdout=None, warning=False):
        """Restore initial data to the SAME path; compare every byte and effect."""
        initial = self.root / f"initial-{self.serial}"
        saved = self.root / f"candidate-{self.serial}"
        shutil.copytree(w, initial, symlinks=True)
        before = files(w)
        location = actions.ActionStore(w).location()
        # Open old replacement targets so inode reuse cannot fake preservation.
        with contextlib.ExitStack() as held:
            for name in ("goal.md", "dashboard.md"):
                if (w / name).is_file():
                    held.enter_context((w / name).open("rb"))
            actual = self.candidate(args, w)
            after = files(w)
            assert actual[0] == code, (args, actual[:2], actual[2][:1500])
            if stdout is not None:
                assert actual[1] == stdout, (actual[1][:2000], stdout[:2000])
            allowed = set(allowed)
            for name in set(before) | set(after):
                if name not in allowed:
                    assert before.get(name) == after.get(name), f"unexpected effect: {name}"
            if "state/.workspace.lock" in before:
                assert after["state/.workspace.lock"] == before["state/.workspace.lock"], "lock replaced"
            for name in ("logs/messages.jsonl", "logs/events.jsonl"):
                if name in before and name in after:
                    assert before[name][3] == after[name][3], f"append replaced old inode: {name}"
                    assert after[name][2].startswith(before[name][2]), f"history rewritten: {name}"
            clocks, ids = [], []
            for name in ("logs/messages.jsonl", "logs/events.jsonl"):
                old = before.get(name, (None, None, b""))[2]
                raw = after.get(name, (None, None, b""))[2]
                if raw == old:
                    continue
                for line in raw[len(old):].split(b"\n"):
                    if not line:
                        continue
                    item = json.loads(line)
                    assert STAMP.fullmatch(item["ts"]), item
                    clocks.append(item["ts"])
                    if name.endswith("events.jsonl"):
                        identity = uuid.UUID(item["event_id"])
                        assert identity.version == 4 and str(identity) == item["event_id"]
                        ids.append(identity)
            w.rename(saved)
            initial.rename(w)
            oracle_before = files(w)
            with contextlib.ExitStack() as oracle_held:
                for name in ("goal.md", "dashboard.md"):
                    if (w / name).is_file():
                        oracle_held.enter_context((w / name).open("rb"))
                with mock.patch.object(coven, "now", side_effect=clocks or None,
                                       return_value="2000-01-01T00:00:00+00:00"), \
                        mock.patch.object(uuid, "uuid4", side_effect=ids or None), \
                        mock.patch.object(actions.ActionStore, "location", return_value=location):
                    expected = python(args, w)
                oracle_after = files(w)
            assert actual[0] == expected[0], (actual, expected)
            assert actual[1] == expected[1], ("literal stdout", actual[1][:3000], expected[1][:3000])
            if code == 0 and not warning:
                assert actual[2] == expected[2] == b"", (actual[2], expected[2])
            elif warning:
                assert actual[2].count(b"[WARN] Goal committed; secondary update failed:") == \
                    expected[2].count(b"[WARN] Goal committed; secondary update failed:") > 0, (actual, expected)
            else:
                assert actual[2] and expected[2], (actual, expected)
            a, b = self.transition(before, after), self.transition(oracle_before, oracle_after)
            assert a == b, "full bytes/modes/links/inode/mtime transition mismatch: " + repr(
                [name for name in set(a) | set(b) if a.get(name) != b.get(name)])
            shutil.rmtree(w)
            saved.rename(w)
        return actual

    @staticmethod
    def allowed(command, missing_logs=False):
        if command in ("messages", "events", "goal-read"):
            return set()
        names = {"state", "state/.workspace.lock", "dashboard.md"}
        if command == "goal":
            names |= {"goal.md", "logs/events.jsonl"}
        if command == "message":
            names |= {"logs/events.jsonl", "logs/messages.jsonl"}
        if missing_logs:
            names.add("logs")
        return names

    def matrix(self):
        for backend in ("snapshot", "pending", "active"):
            for command in COMMANDS:
                for implicit in (False, True):
                    def case(backend=backend, command=command, implicit=implicit):
                        w = self.fixture(backend)
                        items = ["  " + ARG_LITERAL + "  "] if command == "goal" else \
                            ["free sender\x1b", "unknown recipient\u03bb", "  " + ARG_LITERAL, "tail  "] if command == "message" else []
                        self.parity(w, [command, *([] if implicit else [w]), *items],
                                    allowed=self.allowed(command))
                    self.run("command-matrix", f"{backend}/{command}/{implicit}", case)
        for command in COMMANDS:
            def case(command=command):
                w = self.fixture("active")
                (w / "state/tasks.json").write_bytes(b"broken projection ignored\xff")
                tail = ["replacement"] if command == "goal" else ["from", "to", "body"] if command == "message" else []
                self.parity(w, [command, w, *tail], allowed=self.allowed(command))
            self.run("active-projection", command, case)

    def parsing(self):
        cases = [("goal", []), ("goal", [""]), ("goal", [" ", "\t\n"]),
                 ("goal", ["--", "-flag", "tail"]), ("goal", ["-1"]),
                 ("goal", ["--json"]), ("goal", ["--expect-revision", "x"]),
                 ("message", []), ("message", ["from"]), ("message", ["from", "to"]),
                 ("message", ["", "", ""]), ("message", ["-1", "-2", "-3"]),
                 ("message", ["--", "--from", "--to", " body "]),
                 ("message", ["from", "to", "body", "--request-id", "x"])]
        for command in ("messages", "events"):
            cases += [(command, flags) for flags in ([], ["--raw"], ["--pretty"],
                ["--raw", "--pretty"], ["--pretty", "--raw"], ["--raw", "--raw"],
                ["--pretty", "--pretty"], ["--r"], ["--p"], ["--json"], ["-1"],
                ["--raw", "--", "."], ["--", "--raw"], [".", "extra"])]
        cases += [("status", flags) for flags in ([], [""], ["--", "."], ["--json"], [".", "extra"])]
        for command, tail in cases:
            def case(command=command, tail=tail):
                w = self.fixture()
                # Use Python parser/semantic admission, not a guessed clap grammar.
                initial = files(w)
                with mock.patch.object(coven, "now", return_value="2000-01-01T00:00:00+00:00"):
                    expected = python([command, *tail], w)
                # Restore without retaining any trial effects.
                shutil.rmtree(w)
                shutil.copytree(self.base, w)
                assert {k: v[:3] for k, v in files(w).items()} == {k: v[:3] for k, v in initial.items()}
                self.parity(w, [command, *tail], code=expected[0],
                            allowed=self.allowed(command) if expected[0] == 0 else ())
            self.run("parser", f"{command}/{tail!r}", case)
        for command in COMMANDS:
            for selection in ("tilde", "dot", "empty", "no-marker", "terminator"):
                def case(command=command, selection=selection):
                    w = self.fixture()
                    (w / "no-marker").mkdir()
                    selected = {"tilde": "~/" + w.name, "dot": ".", "empty": "",
                                "no-marker": "no-marker", "terminator": str(w)}[selection]
                    args = [command, *(["--"] if selection == "terminator" else []), selected]
                    args += ["from", "to", "body"] if command == "message" else ["tail"] if command == "goal" else []
                    # Non-marker first operands remain goal/message data.
                    code = 1 if command not in ("goal", "message") and selection == "no-marker" else 0
                    self.parity(w, args, code=code, allowed=self.allowed(command) if code == 0 else ())
                self.run("workspace-selection", f"{command}/{selection}", case)
        for command in ("goal", "message"):
            for kind in ("directory-marker", "dangling-marker", "symlink-marker", "root-alias"):
                def case(command=command, kind=kind):
                    w = self.fixture()
                    selected = w
                    if kind == "root-alias":
                        selected = self.root / f"alias-{self.serial}"
                        selected.symlink_to(w, target_is_directory=True)
                    else:
                        marker = w / "coven.json"
                        marker.unlink()
                        if kind == "directory-marker":
                            marker.mkdir()
                        else:
                            marker.symlink_to(self.base / "coven.json" if kind == "symlink-marker" else w / "absent")
                    tail = ["text"] if command == "goal" else ["sender", "recipient", "body"]
                    code = 0 if kind == "root-alias" else 1
                    self.parity(w, [command, selected, *tail], code=code,
                                allowed=self.allowed(command) if code == 0 else ())
                self.run("workspace-marker", f"{command}/{kind}", case)

    def history(self):
        raw_cases = (None, b"", b"\n", b"\r\n\r", b"not JSON\r\nlast\rline",
                     b'{}\n{"partial":', b'{}\n', b'{}', b'{}\r\n{}\r',
                     b'{"text":"\xce\xbb"}\n', b"bad\xff\n", LITERAL.encode())
        for command in ("messages", "events"):
            for index, raw in enumerate(raw_cases):
                for raw_mode in (True, False):
                    def case(command=command, raw=raw, raw_mode=raw_mode):
                        w = self.fixture()
                        path = w / f"logs/{command}.jsonl"
                        if raw is None:
                            path.unlink(missing_ok=True)
                        else:
                            path.write_bytes(raw)
                        try:
                            if raw_mode:
                                expected = ((f"No {command} yet\n\n").encode() if raw is None else
                                            raw.decode("utf-8").replace("\r\n", "\n").replace("\r", "\n").encode())
                            else:
                                storage.read_jsonl(path)
                                expected = None
                            code = 0
                        except (ValueError, storage.StorageError):
                            expected, code = b"", 1
                        self.parity(w, [command, w, *(["--raw", "--pretty"] if raw_mode else [])],
                                    code=code, stdout=expected)
                    self.run("history-bytes", f"{command}/{index}/{raw_mode}", case)
            # All fields accept finite legacy values. Body precedence is Python
            # truthiness, formatting is str/repr (including quote selection).
            values = [None, False, True, 0, -0.0, 1.0, 1e-7, 1e20, 1e21, 5e-324,
                      1.7976931348623157e308, 18446744073709551617, "", LITERAL,
                      [], {}, [None, False, "a'b", 'a"b', "\u00ad\U0001f98a"],
                      {"z": [1.0, -0.0, None], "a'b": 'a"b'}]
            for index, value in enumerate(values):
                def case(command=command, value=value):
                    w = self.fixture()
                    items = [{"ts": value, "from": value, "to": value, "type": value,
                              "message": value, "request": "fallback " + LITERAL,
                              "body": "not selected", "checkpoint": value,
                              "agent": value, "task_id": value, "task": {"id": value}},
                             {"body": value, "request": False, "message": [], "task": value}, {}]
                    (w / f"logs/{command}.jsonl").write_bytes(b"".join(
                        json.dumps(v, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode() + b"\n"
                        for v in items))
                    self.parity(w, [command, "--pretty", w])
                self.run("legacy-values", f"{command}/{index}", case)
            for raw in (b'{}\n[]\n', b'{}\nnull\n', b'{}\n{"v":NaN}\n',
                        b'{}\n{"v":1e999}\n',
                        b'{"ts":1,"ts":2,"body":"first","body":"last"}\n'):
                def case(command=command, raw=raw):
                    w = self.fixture()
                    path = w / f"logs/{command}.jsonl"
                    path.write_bytes(raw)
                    try:
                        storage.read_jsonl(path)
                        code = 0
                    except storage.StorageError:
                        code = 1
                    self.parity(w, [command, w], code=code)
                self.run("strict-history", f"{command}/{raw!r}", case)

    def goal_reads(self):
        for index, raw in enumerate((None, b"", b"\n", b"no final LF", b"a\r\nb\rc\n",
                                     LITERAL.encode(), b"invalid\xff")):
            for tail in ([], ["", " \t\n"]):
                def case(raw=raw, tail=tail):
                    w = self.fixture()
                    path = w / "goal.md"
                    if raw is None:
                        path.unlink()
                    else:
                        path.write_bytes(raw)
                    try:
                        text = raw.decode("utf-8") if raw is not None else None
                        expected = (text.replace("\r\n", "\n").replace("\r", "\n") + "\n").encode() if text is not None else b""
                        code = 0 if text is not None else 1
                    except UnicodeDecodeError:
                        expected, code = b"", 1
                    self.parity(w, ["goal", w, *tail], code=code, stdout=expected)
                self.run("goal-read-not-clear", f"{index}/{tail!r}", case)

    def preflight_order(self):
        pairs = [("coven.json", b"{}", "state/tasks.json", b"{}"),
                 ("state/tasks.json", b"{}", "state/agents.json", b"{}"),
                 ("state/agents.json", b"{}", "state/lead.json", b"{}"),
                 ("state/lead.json", b"{}", "goal.md", b"\xff")]
        for command in COMMANDS:
            for first, raw, second, later in pairs:
                def case(command=command, first=first, raw=raw, second=second, later=later):
                    w = self.fixture()
                    (w / first).write_bytes(raw)
                    # The last pair deliberately uses goal read, not replacement.
                    tail = ["a", "b", "c"] if command == "message" else []
                    args = [command, w, *tail]
                    primary = self.parity(w, args, code=1)
                    (w / second).write_bytes(later)
                    both = self.parity(w, args, code=1)
                    assert primary == both, "later preflight fault masked first error"
                self.run("preflight-order", f"{command}/{first}/{second}", case)
        for count in (1, 2):
            def case(count=count):
                w = self.fixture()
                args = ["message", w, *["operand"] * count]
                semantic = self.parity(w, args, code=1)
                (w / "coven.json").write_bytes(b"{}")
                invalid = self.parity(w, args, code=1)
                assert semantic == invalid, "message loaded workspace before semantic arity"
            self.run("semantic-before-load", str(count), case)

    def preflight(self):
        corruptions = {"coven.json": b"{}", "coven.toml": b"[runtime]\nmultiplexer = 3\n",
                       "state/tasks.json": b"{}", "state/agents.json": b"{}",
                       "state/lead.json": b"{}", "checkpoints/plan-approval.md": b"\xff"}
        for name, raw in corruptions.items():
            for command in COMMANDS:
                def case(name=name, raw=raw, command=command):
                    w = self.fixture()
                    (w / name).write_bytes(raw)
                    tail = ["new"] if command == "goal" else ["a", "b", "body"] if command == "message" else []
                    self.parity(w, [command, w, *tail], code=1)
                self.run("full-preflight", f"{name}/{command}", case)
        for command in COMMANDS:
            for kind in ("valid", "invalid", "orphan"):
                def case(command=command, kind=kind):
                    w = self.fixture()
                    if kind == "valid":
                        with storage.transport_gate(w, w / "state/.runtime-transport.lock", exclusive=True) as gate:
                            with storage.workspace_lock(w, w / "state/.workspace.lock") as held:
                                actions.ActionStore(w).enable(held, gate=gate)
                    elif kind == "invalid":
                        (w / actions.CONTROL).write_bytes(b"{}")
                    else:
                        (w / actions.BUNDLES).mkdir()
                        (w / actions.BUNDLES / "orphan.json").write_bytes(b"broken")
                    tail = ["new"] if command == "goal" else ["a", "b", "body"] if command == "message" else []
                    self.parity(w, [command, w, *tail], code=0 if kind == "valid" else 1,
                                allowed=self.allowed(command) if kind == "valid" else ())
                self.run("runtime-controls", f"{command}/{kind}", case)
        for command in COMMANDS:
            for kind in ("symlink", "directory"):
                def case(command=command, kind=kind):
                    w = self.fixture()
                    target = w / "state/agents.json"
                    target.unlink()
                    if kind == "symlink":
                        target.symlink_to(self.base / "state/agents.json")
                    else:
                        target.mkdir()
                    self.parity(w, [command, w, *(["a", "b", "c"] if command == "message" else [])], code=1)
                self.run("managed-paths", f"{command}/{kind}", case)

    def effects(self):
        for command in COMMANDS:
            def missing(command=command):
                w = self.fixture()
                shutil.rmtree(w / "logs")
                tail = ["new"] if command == "goal" else ["a", "b", "c"] if command == "message" else []
                self.parity(w, [command, w, *tail], allowed=self.allowed(command, True))
            self.run("missing-logs-parent", command, missing)
        for command in ("goal", "message", "status"):
            for selected in ("events", "messages", "both"):
                def corrupt(command=command, selected=selected):
                    w = self.fixture()
                    for log in ("events", "messages"):
                        if selected in (log, "both"):
                            (w / f"logs/{log}.jsonl").write_bytes(b'{}\n{"partial":')
                    tail = ["new"] if command == "goal" else ["a", "b", "c"] if command == "message" else []
                    code = 1 if command == "message" else 0
                    warning = command == "goal" and selected in ("events", "both")
                    allowed = self.allowed(command) if code == 0 else set()
                    self.parity(w, [command, w, *tail], code=code, allowed=allowed, warning=warning)
                    if command == "goal":
                        assert b"new" in (w / "dashboard.md").read_bytes()
                self.run("corrupt-log-effects", f"{command}/{selected}", corrupt)
        for command in ("goal", "message", "status"):
            def modes(command=command):
                w = self.fixture()
                for name in ("goal.md", "dashboard.md", "logs/events.jsonl", "logs/messages.jsonl"):
                    path = w / name
                    path.touch(exist_ok=True)
                    path.chmod(0o640)
                    os.utime(path, ns=(1_000_000_000, 1_000_000_000))
                with storage.workspace_lock(w, w / "state/.workspace.lock"):
                    pass
                tail = ["new"] if command == "goal" else ["a", "b", "c"] if command == "message" else []
                self.parity(w, [command, w, *tail], allowed=self.allowed(command))
            self.run("mode-time-inode", command, modes)
        for command in ("goal", "messages", "events"):
            def no_lock(command=command):
                w = self.fixture()
                with storage.workspace_lock(w, w / "state/.workspace.lock"):
                    before = files(w)
                    result = self.candidate([command, w], w)
                    expected = python([command, w], w)
                    assert result == expected, (result, expected)
                    assert files(w) == before
            self.run("reads-no-lock-no-writes", command, no_lock)

    def profiles(self):
        # None means acceptance; refusals name the exact existing native code.
        profiles = [("log-bytes", "logs/events.jsonl", b'{"x":"' + b"a" * (8 * 1024 * 1024) + b'"}\n', ["events"], "unsupported_event_log_size_profile"),
                    ("physical-lines", "logs/messages.jsonl", b"\n" * 65537, ["messages"], "unsupported_event_log_line_profile"),
                    ("raw-lines-bypass", "logs/events.jsonl", b"\n" * 65537, ["events", "--raw"], None),
                    ("goal-bytes", "goal.md", b"a" * (1024 * 1024 + 1), ["goal"], "unsupported_workspace_text_profile"),
                    ("depth", "logs/messages.jsonl", b'{"body":' + b"[" * 65 + b"0" + b"]" * 65 + b"}\n", ["messages"], "unsupported_json_depth_profile"),
                    ("unicode-scalar", "logs/events.jsonl", b'{"v":"\\ud800"}\n', ["events"], "invalid_jsonl_record"),
                    ("raw-byte-boundary", "logs/events.jsonl", b"x" * (32 * 1024 * 1024), ["events", "--raw"], None),
                    ("raw-byte-over", "logs/events.jsonl", b"x" * (32 * 1024 * 1024 + 1), ["events", "--raw"], "unsupported_history_output_profile"),
                    ("raw-strict-byte-limit-bypass", "logs/events.jsonl", b"x" * (8 * 1024 * 1024 + 1), ["events", "--raw"], None),
                    ("oversized-output-source", "logs/messages.jsonl", b'{"body":"' + b"a" * (32 * 1024 * 1024 + 1) + b'"}\n', ["messages"], "unsupported_event_log_size_profile"),
                    ("physical-lines-boundary", "logs/events.jsonl", b"\n" * 65536, ["events"], None),
                    ("goal-boundary", "goal.md", b"a" * (1024 * 1024), ["goal"], None)]
        finite = [("string-boundary", b'"' + b"a" * (1024 * 1024) + b'"', None),
                  ("string-over", b'"' + b"a" * (1024 * 1024 + 1) + b'"', "unsupported_json_string_profile"),
                  ("depth-boundary", b"[" * 63 + b"0" + b"]" * 63, None),
                  ("array-over", b"[" + b",".join([b"0"] * 65537) + b"]", "unsupported_json_array_profile"),
                  ("object-over", b"{" + b",".join(f'"{n}":0'.encode() for n in range(16385)) + b"}", "unsupported_json_object_profile"),
                  ("nodes-over", b"[" + b",".join([b"[" + b",".join([b"0"] * 65536) + b"]"] * 5) + b"]", "unsupported_json_node_profile"),
                  ("number-token-over", b"0." + b"0" * 8192, "unsupported_number_token_profile"),
                  ("integer-boundary", b"1" * 4300, None)]
        profiles += [(label, "logs/messages.jsonl", b'{"body":' + raw + b"}\n", ["messages"], expected_code)
                     for label, raw, expected_code in finite]
        large_extension = b'["' + b'","'.join([b"a" * (1024 * 1024)] * 8) + b'"]'
        profiles.append(("finite-workspace-input-over", "state/tasks.json",
                         b'[{"id":"seed","title":"seed","status":"pending","x":' + large_extension + b"}]",
                         ["events"], "unsupported_checkpoint_workspace_input_profile"))
        for label, name, raw, args, expected_code in profiles:
            def case(name=name, raw=raw, args=args, expected_code=expected_code):
                w = self.fixture()
                (w / name).write_bytes(raw)
                before = files(w)
                # Python admission is real, but its unbounded output is never
                # mistaken for permission to weaken the native profile.
                expected = python(args, w)
                assert expected[0] == 0, expected[2]
                if not self.oracle_only:
                    actual = self.candidate(args, w)
                    if expected_code is None:
                        assert actual == expected, (actual[0], actual[2])
                    else:
                        assert actual[0] == 1 and actual[1] == b"" and actual[2], actual[:1]
                        assert re.search(rb"\b" + re.escape(expected_code.encode()) + rb"\b", actual[2]), (expected_code, actual[2])
                assert files(w) == before
            self.run("native-profiles", label, case)

    def loop(self):
        def case():
            self.serial += 1
            w = self.home / f"loop-{self.serial}"
            result = self.candidate(["init", w, "--blank", "--agents", "worker,other"], self.root)
            assert result[0] == 0, result
            for backend in ("snapshot", "active"):
                for command, tail in (("goal", [ARG_LITERAL]), ("message", ["free", "other", ARG_LITERAL]),
                                      ("messages", []), ("events", ["--raw"]), ("status", [])):
                    self.serial += 1
                    self.parity(w, [command, w, *tail], allowed=self.allowed(command))
                if backend == "snapshot":
                    revision = coven.read_task_snapshot(w)[0]
                    result = self.candidate(["state", "import-tasks", w, "--expect-revision", revision, "--json"], w)
                    assert result[0] == 0, result
        self.run("offline-command-loop", "native-init-through-active", case)

    def mixed_writers(self):
        for holder in ("goal", "message", "status", "task"):
            def case(holder=holder):
                w = self.fixture()
                (w / "logs/messages.jsonl").write_bytes(b'{"ts":"old","from":"old","to":"old","body":"old"}\n')
                with storage.workspace_lock(w, w / "state/.workspace.lock"):
                    pass
                before = files(w)
                # Both implementations share Python's persistent flock inode.
                # No timing-based mutation race is asserted here; deterministic
                # between-phase rechecks belong to the owning service units.
                with coven.workspace_mutation(w):
                    children = []
                    for oracle, command in ((False, holder), (True, "message"), (False, "goal"), (True, "status")):
                        args = ["task", "add", w, "mixed task", "--json"] if command == "task" else \
                            [command, w, *(["mixed goal"] if command == "goal" else
                                         ["mixed", "recipient", "body " + ARG_LITERAL] if command == "message" else [])]
                        children.append(self.spawn([*self.prefix(oracle), *args], w))
                    time.sleep(0.08)
                    assert all(child.poll() is None for child in children), "writer bypassed Python flock"
                    assert files(w) == before, "blocked writers changed workspace"
                for child in children:
                    result = finish(child)
                    assert result[0] == 0, result
                after = files(w)
                assert after["state/.workspace.lock"] == before["state/.workspace.lock"]
                events = storage.read_jsonl(w / "logs/events.jsonl")
                messages = storage.read_jsonl(w / "logs/messages.jsonl")
                added_messages = messages[1:]
                assert messages[0] == {"ts": "old", "from": "old", "to": "old", "body": "old"}
                assert len(added_messages) == (2 if holder == "message" else 1)
                assert sum(e.get("type") == "message.sent" for e in events) == len(added_messages)
                assert sum(e.get("type") == "goal.updated" for e in events) == (2 if holder == "goal" else 1)
                for message in added_messages:
                    assert message["body"] == "body " + ARG_LITERAL
                for name in ("logs/events.jsonl", "logs/messages.jsonl"):
                    raw = (w / name).read_bytes()
                    assert raw.endswith(b"\n")
                    assert all(isinstance(json.loads(line), dict) for line in raw.split(b"\n")[:-1])
                    if name in before:
                        assert after[name][3] == before[name][3]
                        assert after[name][2].startswith(before[name][2])
                assert (w / "dashboard.md").read_bytes() == coven.render_dashboard(w).encode()
            self.run("mixed-flock-writers", holder, case)

    def stdout_io(self):
        if self.oracle_only:
            return
        for command in ("events", "messages"):
            def case(command=command):
                w = self.fixture()
                path = w / f"logs/{command}.jsonl"
                path.write_bytes(b"tail")
                before = files(w)
                args = [command, w, "--raw"]
                assert self.candidate(args, w) == (0, b"tail", b""), "raw added LF or changed literal bytes"
                assert files(w) == before
                read_fd, write_fd = os.pipe()
                # Close BEFORE spawning: even a four-byte buffered write must
                # fail when flushed, without relying on scheduler timing.
                os.close(read_fd)
                try:
                    child = subprocess.Popen([self.binary, *map(os.fspath, args)],
                                             cwd=w, env=self.env, stdin=subprocess.DEVNULL,
                                             stdout=write_fd, stderr=subprocess.PIPE,
                                             start_new_session=True)
                    CHILDREN.add(child)
                finally:
                    os.close(write_fd)
                _, err = child.communicate(timeout=12)
                CHILDREN.discard(child)
                assert child.returncode == 1, ("closed stdout must fail, not succeed or die via SIGPIPE", child.returncode, err[:4096])
                assert len(err) <= 4096, ("excessive stdout I/O diagnostic", len(err))
                err.decode("utf-8")
                assert path.read_bytes() == b"tail", "output failure rewrote raw log or added LF"
                assert files(w) == before, "stdout I/O failure changed files or acquired a workspace lock"
            self.run("native-stdout-io", command, case)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=lambda value: str(Path(value).resolve()))
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    def expired(*unused):
        raise CorpusTimeout("workspace command corpus exceeded 110-second watchdog")
    signal.signal(signal.SIGALRM, expired)
    signal.alarm(110)
    start = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="workspace-commands-parity-") as temporary, contextlib.ExitStack() as cleanup:
            cleanup.callback(stop_children)
            root = Path(temporary)
            # All in-process oracle expansion and fixtures use disposable HOME.
            with mock.patch.dict(os.environ, {"HOME": str(root / "home"), "COVEN_COMMAND": "coven",
                                              "COVEN_MULTIPLEXER": "", "PYTHONDONTWRITEBYTECODE": "1"}):
                suite = Suite(root, args.binary, args.oracle_only)
                for section in (suite.matrix, suite.parsing, suite.history, suite.preflight,
                                suite.goal_reads, suite.preflight_order, suite.effects,
                                suite.profiles, suite.loop, suite.mixed_writers, suite.stdout_io):
                    section()
            print(json.dumps({"workspace_command_cases": sum(suite.counts.values()),
                              "categories": dict(suite.counts), "failed": len(suite.failures),
                              "failures_by_category": dict(suite.failed), "oracle_only": args.oracle_only,
                              "seconds": round(time.monotonic() - start, 3)}), flush=True)
            representatives = {}
            for failure in suite.failures:
                representatives.setdefault(failure.split("/", 1)[0], failure)
            for failure in representatives.values():
                print(failure, file=sys.stderr)
            if len(suite.failures) > len(representatives):
                print(f"... {len(suite.failures) - len(representatives)} further failures (counts above)", file=sys.stderr)
            return bool(suite.failures)
    finally:
        signal.alarm(0)
        stop_children()


if __name__ == "__main__":
    raise SystemExit(main())
