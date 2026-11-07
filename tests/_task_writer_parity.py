"""Usable CLI task writers, not an SDK-only migration acceptance test.

Only UUIDs/clocks obtained from a completed candidate are frozen in the Python
oracle. No value fields, numeric types, insertion order, warnings, or effects are
discarded. Human output may escape controls/non-ASCII (JSON must remain exact).
Runtime/pending task *list* fences remain the explicit preview contract; writes
are tested independently of those readers. Fault-injected durability, publication
ordering and output reserve admission belong to the owning Rust module units.
Run --oracle-only to validate fixtures/parser expectations before CLI wiring.
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
from runner import actions, checkpoints, coven, launch, storage, task_attempts, task_ledger

PYTHON = str(Path(sys.executable).resolve())
CHILDREN = set()
ENV = {key: value for key, value in os.environ.items()
       if not key.startswith("COVEN_")}
ENV.update(PATH="", COVEN_MULTIPLEXER="", PYTHONDONTWRITEBYTECODE="1",
           PYTHONINTMAXSTRDIGITS="4300")
STAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{6})?\+00:00\Z")


class CorpusTimeout(BaseException):
    """Not swallowed by per-case assertion collection."""


def stop_children(children):
    for child in tuple(children):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
        child.communicate(timeout=2)
        CHILDREN.discard(child)


def equal(actual, expected, label):
    # JSON canonicalization distinguishes bool/int, int/float and signed zero;
    # raw persisted byte assertions below additionally protect insertion order.
    a = json.dumps(actual, ensure_ascii=True, allow_nan=False, sort_keys=True)
    b = json.dumps(expected, ensure_ascii=True, allow_nan=False, sort_keys=True)
    assert a == b, f"{label}\nactual: {a[:6000]}\nexpected: {b[:6000]}"


def python(args, cwd):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.chdir(cwd), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    return code, out.getvalue(), err.getvalue()


def spawn(command, cwd):
    child = subprocess.Popen(list(command), cwd=cwd, env=ENV,
                             stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             start_new_session=True)
    CHILDREN.add(child)
    return child


def finish(child, timeout=8):
    try:
        out, err = child.communicate(timeout=timeout)
        return child.returncode, out.decode("utf-8"), err.decode("utf-8")
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=2)
        CHILDREN.discard(child)


def files(root):
    result = {}
    for path in root.rglob("*"):
        meta = path.lstat()
        mode = stat.S_IMODE(meta.st_mode)
        key = str(path.relative_to(root))
        if path.is_symlink():
            value = ("link", mode, os.readlink(path), meta.st_ino)
        elif path.is_dir():
            value = ("dir", mode, meta.st_ino)
        elif path.is_file():
            value = ("file", mode, path.read_bytes(), meta.st_ino, meta.st_mtime_ns)
        else:
            value = ("special", mode, meta.st_ino)
        result[key] = value
    return result


def uuid4(value, hex_only=False):
    parsed = uuid.UUID(value)
    assert parsed.version == 4 and parsed.variant == uuid.RFC_4122, value
    assert value == (parsed.hex if hex_only else str(parsed)), value


def warnings(result):
    return result.get("delivery", result)["warnings"]


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts, self.failure_counts, self.failures = Counter(), Counter(), []
        self.serial = 0
        self.base = root / "base"
        code, _, err = python(["init", self.base, "--blank", "--agents", "worker,other"], root)
        assert code == 0, err
        team = json.loads((self.base / "coven.json").read_bytes())
        team["display_names"] = {"worker": "Display Only", "other": "Other Label"}
        (self.base / "coven.json").write_bytes(storage.encode_json(team))
        # Intentionally noncanonical source bytes, extension order and numbers.
        (self.base / "state/tasks.json").write_bytes(
            b' [ {"id":"seed","title":"Seed","status":"legacy","owner":"worker",'
            b'"extra":{"z":18446744073709551617,"float":2.9802322387695312e-08,'
            b'"zero":-0.0,"one":1.0,"nested":{"b":2,"a":1},"null":null,"control":"\\u0000"}} ]\r\n')
        (self.base / "goal.md").write_text("# Goal\n\nLiteral goal | `data`\n", encoding="utf-8")
        (self.base / "output/sentinel").write_bytes(b"no broad output writes\n")
        (self.base / "state/.workspace.lock").unlink(missing_ok=True)

    def fixture(self, backend="snapshot"):
        self.serial += 1
        path = self.root / f"case-{self.serial}"
        shutil.copytree(self.base, path)
        if backend == "ledger":
            revision = coven.read_task_snapshot(path)[0]
            code, _, err = python(["state", "import-tasks", path, "--expect-revision", revision, "--json"], self.root)
            assert code == 0, err
            (path / "state/.workspace.lock").unlink(missing_ok=True)
        return path

    def run(self, category, label, fn):
        self.counts[category] += 1
        already_running = set(CHILDREN)
        try:
            fn()
        except Exception as exc:
            self.failure_counts[category] += 1
            self.failures.append(f"{category}/{label}: {type(exc).__name__}: {exc}")
        finally:
            stop_children(CHILDREN - already_running)

    def candidate(self, args, cwd):
        if self.oracle_only:
            return python(args, cwd)
        return finish(spawn([self.binary, *args], cwd))

    def args(self, workspace, backend, action, items, extra=(), json_output=True):
        return ["task", action, str(workspace), *items, *extra,
                *(["--request-id", f"key-{self.serial}"] if backend == "ledger" else []),
                *(["--json"] if json_output else [])]

    def listing(self, workspace):
        before = files(workspace)
        actual = self.candidate(["task", "list", workspace, "--json"], workspace)
        expected = python(["task", "list", workspace, "--json"], workspace)
        assert actual[0] == expected[0] == 0, (actual, expected)
        equal(json.loads(actual[1]), json.loads(expected[1]), "usable add/list/assign/list")
        assert actual[2] == expected[2] == "", (actual, expected)
        assert files(workspace) == before, "list mutated workspace"

    def success(self, workspace, backend, action, args, json_output=True, check_list=True, safe_human=False):
        oracle = self.root / f"oracle-{self.serial}"
        shutil.copytree(workspace, oracle)
        before = files(workspace)
        before_view = task_ledger.LedgerStore(workspace).read()
        old_events = before.get("logs/events.jsonl", (None, None, b""))[2]
        actual = self.candidate(args, workspace)
        assert actual[0] == 0, f"CLI {args!r}: {actual!r}"
        events_raw = (workspace / "logs/events.jsonl").read_bytes()
        assert events_raw.startswith(old_events), "history rewritten"
        added = events_raw[len(old_events):]
        assert len(added.splitlines()) == 1, f"expected exactly one event: {added[:500]!r}"
        event = json.loads(added)
        uuid4(event["event_id"])
        assert STAMP.fullmatch(event["ts"]), event
        if json_output:
            result = json.loads(actual[1])
            assert actual[2] == "", "JSON warnings must stay in envelope"
        elif backend == "ledger":
            view = task_ledger.LedgerStore(workspace).read()
            frozen = next(value["result"] for key, value in view.receipts.items()
                          if key not in before_view.receipts)
            result = coven.ledger_delivery(task_ledger.LedgerStore(workspace), view, frozen, replayed=False)
        else:
            task = (json.loads((workspace / "state/tasks.json").read_bytes())[-1] if action == "add"
                    else next(t for t in json.loads((workspace / "state/tasks.json").read_bytes()) if t["id"] == "seed"))
            result = {"outcome": "committed", "mutation_id": event["mutation_id"], "task": task,
                      "before_revision": storage.snapshot_revision(before["state/tasks.json"][2]),
                      "after_revision": storage.snapshot_revision((workspace / "state/tasks.json").read_bytes()), "warnings": []}
        frozen = result.get("result", result)
        task = frozen["task"]
        uuid4(frozen["mutation_id"], backend == "ledger")
        assert STAMP.fullmatch(task["updated_at"]), task
        if action == "add":
            assert STAMP.fullmatch(task["created_at"]), task
        # Snapshot preflight drafts are thrown away. Freeze both rounds, not just
        # the commit round; never patch storage or secondary callbacks.
        clocks = ([task["created_at"], task["updated_at"]] if action == "add" else [task["updated_at"]])
        if backend == "snapshot":
            clocks *= 2
        clocks += [event["ts"]]
        ids = [uuid.UUID(frozen["mutation_id"]), uuid.UUID(event["event_id"])]
        translated = [str(oracle) if str(arg) == str(workspace) or
                      (str(arg).startswith("~/") and Path(str(arg)).expanduser() == workspace)
                      else arg for arg in args]
        # Implicit workspace arguments must still execute in the oracle clone.
        # A runtime control captures root/device/inode. The cloned oracle is a
        # draft of this SAME location, not a relocation/reactivation test. Keep
        # its captured location intact, including every byte in claims/plans.
        location = actions.ActionStore(workspace).location()
        with mock.patch.object(coven, "now", side_effect=clocks), mock.patch.object(uuid, "uuid4", side_effect=ids), \
                mock.patch.object(actions.ActionStore, "location", return_value=location):
            expected = python(translated, oracle)
        assert expected[0] == 0, expected
        if json_output:
            equal(result, json.loads(expected[1]), "complete mutation envelope")
            assert expected[2] == actual[2] == ""
        else:
            if safe_human and not self.oracle_only:
                line = expected[1].removesuffix("\n")
                safe = "".join(json.dumps(char, ensure_ascii=True)[1:-1]
                               if ord(char) < 32 or 127 <= ord(char) <= 159 else char for char in line)
                # Literal Unicode is permitted; terminal controls must be
                # escaped. An all-ASCII JSON-style Unicode rendering is also
                # allowed, without changing any machine/persisted values.
                ascii_safe = "".join(json.dumps(char, ensure_ascii=True)[1:-1]
                                     if ord(char) > 127 else char for char in safe)
                assert actual[0] == expected[0] and actual[2] == expected[2]
                assert actual[1] in (safe + "\n", ascii_safe + "\n"), (actual, safe)
            else:
                assert actual == expected, (actual, expected)
        assert not warnings(result), result
        after = files(workspace)
        allowed = {"state/tasks.json", "state/.workspace.lock", "logs/events.jsonl", "dashboard.md"}
        if "logs" not in before:
            allowed.add("logs")
        if backend == "ledger":
            view = task_ledger.LedgerStore(workspace).read()
            frame = f"state/task-ledger/{view.epoch_id}/frames/{view.sequence:020d}.frame"
            allowed.add(frame)
            raw = (workspace / frame).read_bytes()
            payload = task_ledger._payload(raw)
            assert raw == task_ledger._envelope(payload), "noncanonical frame"
            replay = task_ledger._apply(before_view, payload, task_ledger._digest(raw))
            equal(view.receipts[payload["request_id"]]["result"], frozen, "frozen Python receipt")
            assert view.projection_bytes == replay.projection_bytes == (workspace / "state/tasks.json").read_bytes()
        else:
            assert frozen["before_revision"] == storage.snapshot_revision(before.get("state/tasks.json", (None, None, None))[2])
            assert frozen["after_revision"] == storage.snapshot_revision((workspace / "state/tasks.json").read_bytes())
        for key in set(before) | set(after):
            if key not in allowed:
                assert after.get(key) == before.get(key), f"unexpected filesystem effect: {key}"
        if "logs" in allowed:
            assert after["logs"][:2] == ("dir", stat.S_IMODE((oracle / "logs").stat().st_mode)), "unsafe/different logs parent"
        for key in allowed - {"state/.workspace.lock", "logs"}:
            if key in after:
                assert after[key][2] == (oracle / key).read_bytes(), f"Python bytes differ: {key}"
                expected_mode = before[key][1] if key in before else 0o600
                assert after[key][1] == expected_mode, f"mode changed: {key}"
        assert (workspace / "dashboard.md").read_bytes() == coven.render_dashboard(workspace).encode("utf-8"), "dashboard golden"
        if "state/.workspace.lock" not in before:
            assert after["state/.workspace.lock"][:3] == ("file", 0o600, b"")
        else:
            assert after["state/.workspace.lock"] == before["state/.workspace.lock"], "coordination inode replaced"
        if "logs/events.jsonl" in before:
            assert after["logs/events.jsonl"][3] == before["logs/events.jsonl"][3], "append replaced history inode"
        else:
            assert after["logs/events.jsonl"][:2] == ("file", 0o600), "unsafe new event file"
        assert after["dashboard.md"][3] != before["dashboard.md"][3], "dashboard not atomically replaced"
        if "state/tasks.json" in before:
            assert after["state/tasks.json"][3] != before["state/tasks.json"][3], "tasks not atomically replaced"
        shutil.rmtree(oracle)
        if check_list:
            self.listing(workspace)
        return result

    def reject(self, workspace, args, semantic=None, parser=False, lock_allowed=False):
        before = files(workspace)
        actual = self.candidate(args, workspace)
        expected = python(args, workspace)
        assert actual[0] == expected[0] == (2 if parser else 1), (args, actual, expected)
        assert actual[1] == expected[1] == "", (actual, expected)
        assert actual[2], actual
        if semantic:
            # Stable native error codes may replace Python/OS prose, but the
            # exact semantic class is still required, not just 'any error'.
            assert re.search(semantic, actual[2], re.I), (semantic, actual)
            assert re.search(semantic, expected[2], re.I), (semantic, expected)
        after = files(workspace)
        if lock_allowed:
            if "state/.workspace.lock" in before:
                assert after.get("state/.workspace.lock") == before["state/.workspace.lock"], "rejection replaced lock inode"
            before.pop("state/.workspace.lock", None)
            after.pop("state/.workspace.lock", None)
        assert after == before, f"rejection changed filesystem: {args!r}"

    def matrix(self):
        titles = ["New task", "Seed", "A_B", "A--B", "  spaced  ", "MiXeD", "Kelvin \u212a", "I\u0130", "Stra\u00dfe",
                  "UTF8 \u03bb \U0001f98a", "quoted \"title\"", "line\nbreak", "tab\tvalue", "DEL\x7f", "dash - data"]
        for backend in ("snapshot", "ledger"):
            for index, title in enumerate(titles):
                for owner in (None, "WORKER"):
                    def case(backend=backend, title=title, owner=owner, index=index):
                        w = self.fixture(backend)
                        extra = ["--notes", "literal \u03bb\n\t\x1f\x7f"]
                        if owner:
                            extra += ["--owner", owner]
                        if index % 2:
                            extra += ["--expect-revision", coven.read_task_snapshot(w)[0]]
                        self.success(w, backend, "add", self.args(w, backend, "add", [title], extra))
                    self.run("add", f"{backend}/{index}/{owner}", case)
            for owner in ("worker", "WORKER", " other ", "Display Only", "missing", "_", "\u03bb", ""):
                for conditional in (False, True):
                    def case(backend=backend, owner=owner, conditional=conditional):
                        w = self.fixture(backend)
                        if owner == "_":
                            team = json.loads((w / "coven.json").read_bytes())
                            team["agents"].append("_")
                            (w / "coven.json").write_bytes(storage.encode_json(team))
                        extra = ["--expect-revision", coven.read_task_snapshot(w)[0]] if conditional else []
                        args = self.args(w, backend, "assign", ["seed", owner], extra)
                        if owner in ("Display Only", "missing", "\u03bb", ""):
                            self.reject(w, args, r"agent|name|slug|identity|letter or number", lock_allowed=backend == "ledger")
                        else:
                            self.success(w, backend, "assign", args)
                    self.run("assign", f"{backend}/{owner!r}/{conditional}", case)
            for title in ("", " ", "!!!", "\u03bb", "\U0001f98a", "___"):
                self.run("invalid-title", f"{backend}/{title!r}", lambda backend=backend, title=title:
                         self.reject_title(backend, title))
            for action in ("add", "assign"):
                for stale in ("tasks-sha256:" + "0" * 64, "bad", "tasks-missing:v1"):
                    def case(backend=backend, action=action, stale=stale):
                        w = self.fixture(backend)
                        args = self.args(w, backend, action, ["New"] if action == "add" else ["seed", "other"],
                                         ["--expect-revision", stale])
                        self.reject(w, args, r"revision", lock_allowed=backend == "ledger")
                    self.run("CAS", f"{backend}/{action}/{stale}", case)

    def reject_title(self, backend, title):
        w = self.fixture(backend)
        self.reject(w, self.args(w, backend, "add", [title]), r"title|slug|name", lock_allowed=backend == "ledger")

    def parsing(self):
        # argparse nargs='+' deliberately rejects a resumed positional after an
        # option; accepting arbitrary intermixing would not be CLI parity.
        layouts = [
            ("words", ["task", "add", "@", "two", "words", "--json"]),
            ("implicit", ["task", "add", "two", "words", "--json"]),
            ("empty-workspace", ["task", "add", "", "two", "--json"]),
            ("prefix", ["task", "add", "--json", "@", "prefix"]),
            ("repeat-owner", ["task", "add", "@", "Work", "--owner", "missing", "--owner", "worker", "--json"]),
            ("repeat-notes", ["task", "add", "@", "Work", "--notes", "old", "--notes", "", "--json"]),
            ("abbrev", ["task", "add", "@", "Work", "--own", "worker", "--not", "memo", "--j"]),
            ("equals", ["task", "add", "@", "Work", "--owner=worker", "--notes=-memo", "--json"]),
            ("terminator", ["task", "add", "--json", "--", "@", "--negative-title"]),
            ("negative-number", ["task", "add", "@", "-1", "--json"]),
            ("assign-extra", ["task", "assign", "@", "seed", "other", "ignored", "--json"]),
            ("assign-prefix", ["task", "assign", "--json", "@", "seed", "worker"]),
            ("assign-abbrev", ["task", "assign", "@", "seed", "worker", "--j"]),
            ("resume-positional", ["task", "add", "@", "first", "--owner", "worker", "last", "--json"]),
            ("repeat-owner-interrupted", ["task", "add", "@", "--owner", "worker", "title", "--owner", "worker", "--json"]),
            ("repeat-json-interrupted", ["task", "add", "@", "first", "--json", "resumed", "--json"]),
            ("negative-exponent", ["task", "add", "@", "-1e3", "--json"]),
            ("negative-trailing-dot", ["task", "add", "@", "-1.", "--json"]),
            ("negative-inf-not-an-operand", ["task", "add", "@", "-inf", "--json"]),
            ("negative-nan-not-an-operand", ["task", "add", "@", "-nan", "--json"]),
            ("notes-negative-exponent", ["task", "add", "@", "Work", "--notes=-1e3", "--json"]),
            ("terminator-negative-exponent", ["task", "add", "--json", "--", "@", "-1e3"]),
            ("unknown-option", ["task", "add", "@", "Work", "--wat"]),
            ("missing-owner", ["task", "add", "@", "Work", "--owner"]),
            ("missing-notes", ["task", "add", "@", "Work", "--notes"]),
            ("missing-revision", ["task", "add", "@", "Work", "--expect-revision"]),
            ("missing-key", ["task", "add", "@", "Work", "--request-id"]),
            ("missing-add", ["task", "add"]),
            ("missing-assign", ["task", "assign"]),
            ("assign-one", ["task", "assign", "@", "seed", "--json"]),
            ("marker-no-title", ["task", "add", "@", "--json"]),
            ("negative-selector", ["task", "assign", "@", "-seed", "worker", "--json"]),
            ("negative-selector-terminator", ["task", "assign", "--json", "--", "@", "-seed", "worker"]),
            ("unknown-command", ["task", "teleport", "@", "seed", "--json"]),
        ]
        for name, layout in layouts:
            def case(name=name, layout=layout):
                w = self.fixture()
                args = [str(w) if value == "@" else value for value in layout]
                # Classify without executing the service: parser cases are not
                # permitted to mutate just to discover an expected exit code.
                with contextlib.redirect_stderr(io.StringIO()):
                    try:
                        coven.parse_args(args)
                        parser_error = False
                    except SystemExit as exc:
                        assert exc.code == 2
                        parser_error = True
                if parser_error or name in ("assign-one", "marker-no-title", "negative-selector-terminator"):
                    self.reject(w, args, parser=parser_error)
                else:
                    self.success(w, "snapshot", args[1], args)
            self.run("parser", name, case)
        for backend in ("snapshot", "ledger"):
            for action in ("add", "assign"):
                def case(backend=backend, action=action):
                    w = self.fixture(backend)
                    self.success(w, backend, action, self.args(w, backend, action,
                                 ["Human task"] if action == "add" else ["seed", "other"], json_output=False), json_output=False)
                self.run("human", f"{backend}/{action}", case)

    def corruption(self):
        changes = [("state/tasks.json", raw) for raw in (
            b"", b"{", b"null", b"{}", b"\xff", b'[{"id":"x","title":true,"status":"pending"}]',
            b'[{"id":"x","title":"x","status":"p","notes":null}]')]
        changes += [(f"logs/{name}.jsonl", raw) for name in ("events", "messages")
                    for raw in (b"not-json\n", b'{"ok":1}', b"[]\n", b"\xff\n")]
        changes += [("coven.json", b"{}"), ("coven.json", b"null"), ("coven.toml", b"invalid !")]
        for index, (relative, raw) in enumerate(changes):
            def case(relative=relative, raw=raw):
                w = self.fixture()
                (w / relative).write_bytes(raw)
                self.reject(w, self.args(w, "snapshot", "add", ["Blocked"]))
                assert not (w / "state/.workspace.lock").exists()
            self.run("snapshot-preflight", str(index), case)
        for backend in ("snapshot", "ledger"):
            for name in ("pending", "marker", "execution", "orphan-execution", "key-policy", "projection", "unsafe-projection"):
                def case(backend=backend, name=name):
                    w = self.fixture(backend)
                    semantic = None
                    if name == "pending":
                        if backend == "ledger":
                            view = task_ledger.LedgerStore(w).read()
                            (w / f"state/task-ledger/{view.epoch_id}/frames/{view.sequence:020d}.frame").write_bytes(b"corrupt frame\n")
                            return self.reject(w, self.args(w, backend, "add", ["No snapshot fallback"]), r"baseline.*digest|frame|ledger|header")
                        (w / "state/task-ledger").mkdir()
                        semantic = r"pending"
                    elif name == "marker":
                        (w / "state/task-authority.json").write_bytes(b"null")
                    elif name == "execution":
                        (w / "state/execution").mkdir()
                        (w / "state/execution/control.json").write_bytes(b"null")
                    elif name == "orphan-execution":
                        (w / "state/execution").mkdir()
                        (w / "state/execution/orphan").write_bytes(b"orphan")
                    elif name == "key-policy":
                        args = ["task", "add", str(w), "Blocked", "--json"]
                        if backend == "snapshot":
                            args += ["--request-id", "key"]
                        return self.reject(w, args, r"request.?id|request_key|import", lock_allowed=backend == "ledger")
                    elif name in ("projection", "unsafe-projection"):
                        if backend == "snapshot":
                            (w / "state/tasks.json").write_bytes(b"broken")
                        elif name == "projection":
                            (w / "state/tasks.json").write_bytes(b"[]\n")
                            semantic = r"projection"
                        else:
                            (w / "state/tasks.json").unlink()
                            (w / "state/tasks.json").symlink_to(self.root / "absent")
                            semantic = r"projection|unsafe"
                    self.reject(w, self.args(w, backend, "add", ["Blocked"]), semantic, lock_allowed=backend == "ledger" or "execution" in name)
                self.run("authority-admission", f"{backend}/{name}", case)

    def history_and_dashboard(self):
        for backend in ("snapshot", "ledger"):
            for fault in ("goal-utf8", "checkpoint-grammar", "events-corrupt", "messages-corrupt"):
                for json_output in (False, True):
                    def case(backend=backend, fault=fault, json_output=json_output):
                        w = self.fixture(backend)
                        if fault == "goal-utf8":
                            (w / "goal.md").write_bytes(b"\xff")
                        elif fault == "checkpoint-grammar":
                            (w / "checkpoints/not.valid.MD").write_bytes(b"bad grammar")
                        else:
                            (w / f"logs/{fault.split('-')[0]}.jsonl").write_bytes(b"broken\n")
                        args = self.args(w, backend, "add", ["Secondary failure"], json_output=json_output)
                        if fault == "checkpoint-grammar":
                            # Directory grammar is deterministic admission, not a
                            # postcommit render failure, including unselected IDs.
                            return self.reject(w, args, r"checkpoint", lock_allowed=backend == "ledger")
                        if backend == "snapshot" and fault.endswith("corrupt"):
                            return self.reject(w, args, r"line|jsonl|history|inspect/repair")
                        before = files(w)
                        result = self.candidate(args, w)
                        assert result[0] == 0, result
                        view = task_ledger.LedgerStore(w).read()
                        tasks = view.tasks if view else json.loads((w / "state/tasks.json").read_bytes())
                        assert tasks[-1]["title"] == "Secondary failure"
                        new_task = {"id": "secondary-failure", "title": "Secondary failure", "status": "pending", "owner": None,
                                    "notes": "", "created_at": tasks[-1]["created_at"], "updated_at": tasks[-1]["updated_at"]}
                        assert STAMP.fullmatch(new_task["created_at"]) and STAMP.fullmatch(new_task["updated_at"])
                        equal(tasks[-1], new_task, "secondary failure cannot change task shape")
                        expected_tasks = json.loads(before["state/tasks.json"][2])
                        expected_tasks.append(json.loads(task_ledger._canonical(new_task)) if view else new_task)
                        assert (w / "state/tasks.json").read_bytes() == storage.encode_json(expected_tasks)
                        label = "history incomplete" if fault == "events-corrupt" else "dashboard stale"
                        if fault == "messages-corrupt":
                            # Dashboard reads workspace records, not observational
                            # history; message corruption does not gate rendering.
                            label = None
                        if json_output:
                            envelope = json.loads(result[1])
                            if self.oracle_only and fault == "goal-utf8":
                                # Python's existing callback prints ERROR before
                                # SystemExit(1) is caught. Native must not leak it.
                                assert re.search(r"\[ERROR\] Cannot read managed file .*goal.md: .*utf-8", result[2]), result
                            else:
                                assert result[2] == "", result
                            ws = warnings(envelope)
                        else:
                            lines = result[2].splitlines()
                            if self.oracle_only and fault == "goal-utf8":
                                assert re.search(r"\[ERROR\] Cannot read managed file .*goal.md: .*utf-8", lines[0]), result
                                lines = lines[1:]
                            ws = [line.removeprefix("[WARN] ") for line in lines]
                            assert all(line.startswith("[WARN] ") for line in lines), result
                            prefix = "Committed ledger task secondary-failure (sequence 1)" if backend == "ledger" else "Created secondary-failure: Secondary failure"
                            assert result[1] == prefix + "\n", result
                        assert len(ws) == (1 if label else 0), ws
                        if label:
                            assert re.match(r"(?:Ledger task|Task) committed; " + label + r": .+", ws[0]), ws
                            semantic = (r"utf.?8|decode|invalid.*text" if fault == "goal-utf8" else
                                        r"checkpoint|grammar" if fault == "checkpoint-grammar" else r"line|json|parse|decode")
                            if self.oracle_only and fault == "goal-utf8":
                                suffix = ". Inspect before retrying this request." if backend == "snapshot" else ""
                                assert ws[0] == ("Task" if backend == "snapshot" else "Ledger task") + " committed; dashboard stale: 1" + suffix
                            else:
                                assert re.search(semantic, ws[0].split(label + ": ", 1)[1], re.I), ws
                        if fault in ("events-corrupt", "messages-corrupt"):
                            assert (w / "dashboard.md").read_bytes() == coven.render_dashboard(w).encode(), "event failure prevented dashboard"
                        else:
                            assert (w / "dashboard.md").read_bytes() == before["dashboard.md"][2]
                        allowed = {"state/tasks.json", "state/.workspace.lock", "logs/events.jsonl", "dashboard.md"}
                        if view:
                            allowed.add(f"state/task-ledger/{view.epoch_id}/frames/{view.sequence:020d}.frame")
                        after = files(w)
                        assert all(after.get(k) == v for k, v in before.items() if k not in allowed)
                        assert set(after) - set(before) <= allowed
                        for key in allowed & before.keys():
                            assert after[key][1] == before[key][1], f"secondary changed mode: {key}"
                        event_bytes = after["logs/events.jsonl"][2]
                        if fault == "events-corrupt":
                            assert after["logs/events.jsonl"] == before["logs/events.jsonl"], "corrupt history was rewritten"
                        else:
                            old = before["logs/events.jsonl"][2]
                            assert event_bytes.startswith(old)
                            appended = event_bytes[len(old):]
                            assert len(appended.splitlines()) == 1
                            event = json.loads(appended)
                            uuid4(event["event_id"])
                            assert STAMP.fullmatch(event["ts"])
                            expected_event = {"event_id": event["event_id"], "ts": event["ts"], "type": "task.created", "task": new_task}
                            if view:
                                frozen = next(value["result"] for value in view.receipts.values())
                                expected_event.update(mutation_id=frozen["mutation_id"], epoch_id=view.epoch_id,
                                                      sequence=view.sequence, frame_digest=view.head_digest)
                            else:
                                uuid4(event["mutation_id"])
                                expected_event.update(mutation_id=event["mutation_id"],
                                    before_revision=storage.snapshot_revision(before["state/tasks.json"][2]),
                                    after_revision=storage.snapshot_revision(after["state/tasks.json"][2]))
                            assert appended == (json.dumps(expected_event, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
                    self.run("secondary", f"{backend}/{fault}/{json_output}", case)

    def missing_logs_parent(self):
        for backend in ("snapshot", "ledger"):
            for action in ("add", "assign"):
                def case(backend=backend, action=action):
                    w = self.fixture(backend)
                    shutil.rmtree(w / "logs")
                    # Match ordinary Python append_event mkdir under its held
                    # workspace lock, not a committed-with-history-warning path.
                    # Add exercises JSON; assign exercises exact human output.
                    json_output = action == "add"
                    args = self.args(w, backend, action, ["Recreate safe history"] if action == "add" else ["seed", "other"],
                                     json_output=json_output)
                    self.success(w, backend, action, args, json_output=json_output)
                    assert set((w / "logs").iterdir()) == {w / "logs/events.jsonl"}, "unexpected log/message backfill"
                self.run("missing-logs-parent", f"{backend}/{action}", case)

    def replay(self):
        for change in ("owner", "projection", "missing-projection", "events", "dashboard", "execution", "selector", "config", "raw-binding"):
            def case(change=change):
                w = self.fixture("ledger")
                revision = coven.read_task_snapshot(w)[0]
                args = self.args(w, "ledger", "add", ["Frozen"], ["--owner", "WORKER", "--expect-revision", revision])
                first = self.success(w, "ledger", "add", args)
                if change in ("owner", "selector"):
                    assert python(["task", "assign", w, "frozen", "other", "--request-id", "later"], w)[0] == 0
                    team = json.loads((w / "coven.json").read_bytes())
                    team["agents"] = ["other"]
                    (w / "coven.json").write_bytes(storage.encode_json(team))
                elif change == "projection":
                    (w / "state/tasks.json").write_bytes(b"diverged\xff")
                elif change in ("missing-projection", "events", "dashboard"):
                    relative = {"missing-projection": "state/tasks.json", "events": "logs/events.jsonl", "dashboard": "dashboard.md"}[change]
                    (w / relative).unlink()
                elif change == "execution":
                    (w / "state/execution").mkdir()
                    (w / "state/execution/control.json").write_bytes(b"broken")
                elif change == "config":
                    (w / "coven.json").write_bytes(b"{}")
                elif change == "raw-binding":
                    args[args.index("WORKER")] = "worker"
                    (w / "state/execution").mkdir()
                    (w / "state/execution/control.json").write_bytes(b"broken")
                before = files(w)
                if change in ("config", "raw-binding"):
                    self.reject(w, args, r"binding|request.*conflict" if change == "raw-binding" else None)
                else:
                    actual = self.candidate(args, w)
                    assert actual[0] == 0 and actual[2] == "", actual
                    result = json.loads(actual[1])
                    expected = json.loads(python(args, w)[1])
                    equal(result, expected, "replay delivery including projection diagnostics")
                    equal(result["result"], first["result"], "original receipt despite changed state")
                    assert result["delivery"]["replayed"] is True
                assert files(w) == before, "replay backfilled secondary state"
            self.run("replay", change, case)

    def attempts(self):
        for backend in ("snapshot", "ledger"):
            for target in ("same-owner", "different-owner", "unrelated"):
                def case(backend=backend, target=target):
                    w = self.fixture(backend)
                    def call(*args):
                        result = python(args, w)
                        assert result[0] == 0, result
                    call("execution", "enable", w, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable", "--json")
                    call("execution", "enroll", w, "seed", "--expect-execution-revision", task_attempts.Store(w).read().revision,
                         "--expect-tasks-revision", coven.read_task_snapshot(w)[0], "--request-id", "enroll", "--json")
                    intent = self.root / f"intent-{self.serial}.json"
                    intent.write_bytes(storage.encode_json({"schema_version": 1, "kind": "task-intent", "task_id": "seed", "owner": "worker",
                                                           "acceptance_criteria": [{"id": "check", "text": "not executed"}]}))
                    call("task", "attempt", "prepare", w, "seed", "--intent", intent,
                         "--expect-execution-revision", task_attempts.Store(w).read().revision,
                         "--expect-tasks-revision", coven.read_task_snapshot(w)[0], "--request-id", "prepare", "--json")
                    if target == "unrelated":
                        self.success(w, backend, "add", self.args(w, backend, "add", ["Unrelated"]), check_list=False)
                    else:
                        self.reject(w, self.args(w, backend, "assign", ["seed", "worker" if target == "same-owner" else "other"]),
                                    r"prepared.*attempt|prepared_attempt", lock_allowed=True)
                self.run("prepared", f"{backend}/{target}", case)

    def races(self):
        code = "from runner import coven;import sys;raise SystemExit(coven.main(sys.argv[1:]))"
        for backend in ("snapshot", "ledger"):
            for kind in ("adds", "CAS", "same-key", "changed-binding"):
                if backend == "snapshot" and kind in ("same-key", "changed-binding"):
                    continue
                def case(backend=backend, kind=kind):
                    w = self.fixture(backend)
                    revision = coven.read_task_snapshot(w)[0]
                    count = 4 if kind == "adds" else 2
                    args = []
                    for index in range(count):
                        action, items = ("assign", ["seed", "worker" if index == 0 else "other"]) if kind == "CAS" else ("add", ["Race" if kind != "changed-binding" else f"Race {index}"])
                        command = ["task", action, str(w), *items, "--json"]
                        if backend == "ledger":
                            command += ["--request-id", "shared" if kind in ("same-key", "changed-binding") else f"race-{index}"]
                        if kind == "CAS":
                            command += ["--expect-revision", revision]
                        args.append(command)
                    children = []
                    with coven.workspace_mutation(w):
                        for index, command in enumerate(args):
                            executable = [PYTHON, "-c", code] if index % 2 or self.oracle_only else [self.binary]
                            children.append(spawn([*executable, *command], ROOT))
                    results = [finish(child) for child in children]
                    assert sorted(r[0] for r in results) == ([0] * count if kind in ("adds", "same-key") else [0, 1]), results
                    successful = [json.loads(r[1]) for r in results if r[0] == 0]
                    tasks = coven.read_task_snapshot(w)[1]
                    events = storage.read_jsonl(w / "logs/events.jsonl")
                    mutations = [e for e in events if "mutation_id" in e]
                    effects = count if kind == "adds" else 1
                    assert len(mutations) == effects
                    assert len({e["event_id"] for e in events}) == len(events), "duplicate history IDs"
                    assert len({e["mutation_id"] for e in mutations}) == effects, "duplicate mutation IDs"
                    assert len(tasks) == (1 if kind == "CAS" else 1 + effects)
                    assert len({t["id"] for t in tasks}) == len(tasks)
                    if kind == "same-key":
                        equal(successful[0]["result"], successful[1]["result"], "cross-language shared receipt")
                        assert sorted(r["delivery"]["replayed"] for r in successful) == [False, True]
                    if kind == "CAS":
                        assert re.search(r"revision", next(r[2] for r in results if r[0] == 1), re.I)
                    if kind == "changed-binding":
                        assert re.search(r"binding|request.*conflict", next(r[2] for r in results if r[0] == 1), re.I)
                    assert (w / "dashboard.md").read_bytes() == coven.render_dashboard(w).encode()
                    self.listing(w)
                self.run("mixed-race", f"{backend}/{kind}", case)

    def storage_profiles(self):
        sources = [None, b"[]", b" [ ]\r\n",
                   b'[{"id":"seed","title":"old","title":"new","status":"pending"}]',
                   b'[{"id":"seed","title":"No owner","status":"p"}]',
                   b'[{"id":"seed","title":"Null owner","status":"p","owner":null}]',
                   b'[{"id":"seed","title":"Empty notes","status":"p","notes":""}]',
                   b'[{"id":"seed","title":"Old time","status":"p","created_at":"not-a-clock"}]']
        for index, raw in enumerate(sources):
            def case(raw=raw):
                w = self.fixture()
                path = w / "state/tasks.json"
                if raw is None:
                    path.unlink()
                else:
                    path.write_bytes(raw)
                    path.chmod(0o640)
                self.success(w, "snapshot", "add", self.args(w, "snapshot", "add", ["Exact source"],
                             ["--expect-revision", storage.snapshot_revision(raw)]))
            self.run("source-bytes", str(index), case)
        for backend in ("snapshot", "ledger"):
            def sequence(backend=backend):
                w = self.fixture(backend)
                first = self.success(w, backend, "add", self.args(w, backend, "add", ["Complete usable CLI"]))
                task = first.get("result", first)["task"]
                args = self.args(w, backend, "assign", [task["id"], "worker"])
                if backend == "ledger":
                    args[args.index("--request-id") + 1] += "-assign"
                self.success(w, backend, "assign", args)
            self.run("end-to-end", backend, sequence)
            for native_first in (False, True):
                def mixed(backend=backend, native_first=native_first):
                    w = self.fixture(backend)
                    add = self.args(w, backend, "add", ["Mixed writer"])
                    if native_first:
                        self.success(w, backend, "add", add)
                    else:
                        result = python(add, w)
                        assert result[0] == 0, result
                    assign = self.args(w, backend, "assign", ["mixed-writer", "other"])
                    if backend == "ledger":
                        assign[assign.index("--request-id") + 1] += "-assign"
                    if native_first:
                        result = python(assign, w)
                        assert result[0] == 0, result
                        self.listing(w)
                    else:
                        self.success(w, backend, "assign", assign)
                self.run("mixed-direction", f"{backend}/{native_first}", mixed)
        for name in ("quoted-home", "marker-ambiguity"):
            def case(name=name):
                w = self.fixture()
                if name == "quoted-home":
                    home = w.parent
                    with mock.patch.dict(os.environ, {"HOME": str(home)}), mock.patch.dict(ENV, {"HOME": str(home)}):
                        self.success(w, "snapshot", "add", ["task", "add", "~/" + w.name, "Quoted home", "--json"])
                else:
                    # A directory without coven.json is title data, never an
                    # explicit workspace. An existing marker consumes the item.
                    (w / "looks-like-workspace").mkdir()
                    self.success(w, "snapshot", "add", ["task", "add", "looks-like-workspace", "tail", "--json"])
            self.run("workspace", name, case)
        for name in ("lines", "bytes"):
            def case(name=name):
                w = self.fixture()
                path = w / "logs/events.jsonl"
                raw = b"{}\n" * 65537 if name == "lines" else (b'{"data":"' + b"a" * (8 * 1024 * 1024) + b'"}\n')
                path.write_bytes(raw)
                # Python accepts these histories. Native's bounded source-buffer
                # profile is deliberate, not a change to Python's default CLI.
                assert len(storage.read_jsonl(path)) == (65537 if name == "lines" else 1)
                before = files(w)
                if not self.oracle_only:
                    result = self.candidate(self.args(w, "snapshot", "add", ["Profile blocked"]), w)
                    assert result[0] == 1 and result[1] == "", result
                    assert re.search(r"profile|limit|too_large|too_many", result[2], re.I), result
                assert files(w) == before
            self.run("native-source-profile", name, case)

    def runtime_controls(self):
        for backend in ("snapshot", "ledger"):
            for mode in ("valid", "claimed", "corrupt", "orphan"):
                def case(backend=backend, mode=mode):
                    w = self.fixture(backend)
                    store = actions.ActionStore(w)
                    if mode == "orphan":
                        (w / actions.BUNDLES).mkdir()
                        (w / actions.BUNDLES / "ignored.json").write_bytes(b"not a bundle")
                    elif mode == "corrupt":
                        (w / actions.CONTROL).write_bytes(b"{}")
                    else:
                        with storage.transport_gate(w, w / "state/.runtime-transport.lock", exclusive=True) as gate:
                            with storage.workspace_lock(w, w / "state/.workspace.lock") as held:
                                control = store.enable(held, gate=gate)
                                if mode == "claimed":
                                    action_id = uuid.uuid4().hex
                                    inputs = {"files": {"coven.json": {"sha256": "frozen"}}, "inventory": ["coven.json"]}
                                    plan = launch.build_plan(str(w), f"coven-{control['workspace_id']}-{action_id}", "tmux",
                                        [{"id": "worker", "command": "never execute", "starter": "test only"}], {"PATH": ""})
                                    bundle = store.make_bundle(inputs, plan, action_id, held)
                                    store.publish_bundle(bundle, held)
                                    cp_id = actions.checkpoint_id(bundle)
                                    raw, cp = checkpoints.prepare(cp_id, actions.checkpoint_bytes(bundle), "approved", "test only",
                                        decision_id=uuid.uuid4().hex, decided_at="now", source="cli")
                                    (w / "checkpoints" / f"{cp_id}.md").write_bytes(raw)
                                    store.claim(action_id=action_id, expected_manifest=actions.digest(bundle), expected_checkpoint=cp.revision,
                                        expected_decision=cp.decision["decision_id"], current_inputs=inputs, lock=held)
                                # Once control exists, task admission must not
                                # parse unrelated action bundle children.
                                (w / actions.BUNDLES).mkdir(exist_ok=True)
                                (w / actions.BUNDLES / "unrelated.json").write_bytes(b"broken child ignored")
                    if mode in ("corrupt", "orphan"):
                        self.reject(w, self.args(w, backend, "add", ["Blocked"]), r"control|runtime|orphan|action|authority", lock_allowed=True)
                    else:
                        self.success(w, backend, "add", self.args(w, backend, "add", ["Allowed runtime write"]), check_list=False)
                self.run("runtime-control", f"{backend}/{mode}", case)

    def ordered_lock_races(self):
        # This pause exists only in a Python test process. No production
        # environment failpoint, runtime command, shell, or user process is used.
        worker = """
import contextlib,sys
from runner import coven,storage
original=storage.workspace_lock
announced=False
@contextlib.contextmanager
def paused(*args,**kwargs):
    global announced
    with original(*args,**kwargs) as held:
        if not announced and kwargs.get('held') is None:
            announced=True
            print('locked',flush=True)
            assert sys.stdin.readline()=='release\\n'
        yield held
storage.workspace_lock=paused
raise SystemExit(coven.main(sys.argv[1:]))
"""
        for backend in ("snapshot", "ledger"):
            def case(backend=backend):
                w = self.fixture(backend)
                for command in (
                    ["execution", "enable", w, "--expect-execution-revision", task_attempts.DISABLED, "--request-id", "enable", "--json"],
                    ["execution", "enroll", w, "seed", "--expect-execution-revision", "@", "--expect-tasks-revision",
                     coven.read_task_snapshot(w)[0], "--request-id", "enroll", "--json"]):
                    if "@" in command:
                        command[command.index("@")] = task_attempts.Store(w).read().revision
                    result = python(command, w)
                    assert result[0] == 0, result
                intent = self.root / f"locked-intent-{self.serial}.json"
                intent.write_bytes(storage.encode_json({"schema_version": 1, "kind": "task-intent", "task_id": "seed", "owner": "worker",
                    "acceptance_criteria": [{"id": "check", "text": "never execute"}]}))
                prepare = ["task", "attempt", "prepare", w, "seed", "--intent", intent,
                    "--expect-execution-revision", task_attempts.Store(w).read().revision,
                    "--expect-tasks-revision", coven.read_task_snapshot(w)[0], "--request-id", "prepare", "--json"]
                holder = spawn([PYTHON, "-c", worker, *prepare], ROOT)
                assert select.select([holder.stdout], [], [], 5)[0], "prepare lock barrier timed out"
                assert holder.stdout.readline() == b"locked\n", "prepare failed before lock barrier"
                command = self.args(w, backend, "assign", ["seed", "worker"])
                prefix = [PYTHON, "-c", "from runner import coven;import sys;raise SystemExit(coven.main(sys.argv[1:]))"] if self.oracle_only else [self.binary]
                assignment = spawn([*prefix, *command], ROOT)
                time.sleep(0.06)
                assert assignment.poll() is None, "assignment bypassed held preparation lock"
                holder.stdin.write(b"release\n")
                holder.stdin.flush()
                result = finish(holder)
                assert result[0] == 0, result
                before = files(w)
                result = finish(assignment)
                assert result[0] == 1 and re.search(r"prepared.*attempt|prepared_attempt", result[2], re.I), result
                assert files(w) == before, "queued assignment rewrote prepared task"
            self.run("prepare-lock", backend, case)
        for action in ("add", "assign"):
            def case(action=action):
                w = self.fixture()
                args = self.args(w, "snapshot", action, ["Queued import"] if action == "add" else ["seed", "other"])
                prefix = [PYTHON, "-c", "from runner import coven;import sys;raise SystemExit(coven.main(sys.argv[1:]))"] if self.oracle_only else [self.binary]
                with coven.workspace_mutation(w) as held:
                    child = spawn([*prefix, *args], ROOT)
                    time.sleep(0.06)
                    assert child.poll() is None, "task writer bypassed import lock"
                    task_ledger.LedgerStore(w).import_tasks(coven.read_task_snapshot(w)[0], held)
                    before = files(w)
                result = finish(child)
                assert result[0] == 1 and re.search(r"request.?id|request_key", result[2], re.I), result
                assert files(w) == before, "queued snapshot overwrote imported authority"
            self.run("queued-import", action, case)

    def final_edges(self):
        for backend in ("snapshot", "ledger"):
            def case(backend=backend):
                w = self.fixture(backend)
                extra = ["--expect-revision", "bad", "--expect-revision", coven.read_task_snapshot(w)[0]]
                if backend == "ledger":
                    extra += ["--request-id", "discarded-key"]
                self.success(w, backend, "add", self.args(w, backend, "add", ["Last flags win"], extra))
            self.run("repeated-flags", backend, case)
        def invalid_utf8():
            w = self.fixture()
            args = [b"task", b"add", os.fsencode(w), b"bad-\xff", b"--json"]
            before = files(w)
            python_result = finish(spawn([PYTHON, "-c", "from runner import coven;import sys;raise SystemExit(coven.main(sys.argv[1:]))", *args], ROOT))
            assert python_result[0] == 1 and python_result[1] == "", python_result
            assert files(w) == before
            if not self.oracle_only:
                # subprocess needs bytes preserved, unlike normal string args.
                child = subprocess.Popen([os.fsencode(self.binary), *args], cwd=w, env=ENV,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
                CHILDREN.add(child)
                result = finish(child)
                assert result[0] == 1 and result[1] == "", result
                assert re.search(r"utf.?8|unicode|profile", result[2], re.I), result
            assert files(w) == before
        self.run("native-argument-profile", "non-UTF8", invalid_utf8)
        def checkpoint():
            w = self.fixture()
            cp_id = "plan-approval"
            path = w / "checkpoints" / (cp_id + ".md")
            raw, _ = checkpoints.prepare(cp_id, path.read_bytes(), "approved", "fixture approval",
                decision_id=uuid.uuid4().hex, decided_at="2026-11-01T00:00:00+00:00", source="cli")
            path.write_bytes(raw)
            assert coven.checkpoint_status(w, cp_id) == "approved"
            self.success(w, "snapshot", "add", self.args(w, "snapshot", "add", ["Effective checkpoint"]))
        self.run("dashboard-checkpoint", "effective-approved", checkpoint)
        def human_controls():
            w = self.fixture()
            self.success(w, "snapshot", "add", self.args(w, "snapshot", "add", ["Safe \u03bb\nline\t\x1f\x7f"], json_output=False),
                         json_output=False, safe_human=True)
        self.run("human-policy", "Unicode-and-controls", human_controls)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=lambda value: str(Path(value).resolve()))
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    def expired(*unused):
        raise CorpusTimeout("task writer corpus exceeded 55-second watchdog")
    signal.signal(signal.SIGALRM, expired)
    signal.alarm(55)
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix="task-writer-parity-") as temporary, contextlib.ExitStack() as cleanup:
            cleanup.callback(lambda: stop_children(CHILDREN))
            suite = Suite(Path(temporary), args.binary, args.oracle_only)
            for section in (suite.matrix, suite.parsing, suite.corruption, suite.history_and_dashboard,
                            suite.replay, suite.attempts, suite.races, suite.storage_profiles, suite.runtime_controls,
                            suite.ordered_lock_races, suite.final_edges, suite.missing_logs_parent):
                section()
            print(json.dumps({"task_writer_cases": sum(suite.counts.values()), "categories": dict(suite.counts),
                              "failed": len(suite.failures), "oracle_only": args.oracle_only,
                              "failures_by_category": dict(suite.failure_counts),
                              "seconds": round(time.monotonic() - started, 3)}), flush=True)
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
        stop_children(CHILDREN)


if __name__ == "__main__":
    raise SystemExit(main())
