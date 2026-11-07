"""Bounded refresh CLI parity, with no Python/runtime on the native PATH.

Both implementations execute at the SAME workspace path, restored from identical
initial bytes. Runtime controls capture that directory's identity, so restoration
deliberately retains the workspace root inode. --oracle-only checks the corpus
before the native command is wired.
"""
import argparse
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
import tomllib
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.dont_write_bytecode = True
from runner import actions, coven, storage, task_ledger

CHILDREN = set()
ENV = {key: value for key, value in os.environ.items() if not key.startswith("COVEN_")}
ENV.update(COVEN_COMMAND="/deterministic tools/coven's cli", PYTHONDONTWRITEBYTECODE="1")


class CorpusTimeout(BaseException):
    """A total deadline must not be swallowed by per-case failure collection."""


def python(args, cwd):
    out, err = io.StringIO(), io.StringIO()
    with mock.patch.dict(os.environ, ENV, clear=True), contextlib.chdir(cwd), \
            contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    return code, out.getvalue(), err.getvalue()


def snapshot(root):
    """Never follow links or read FIFOs, including broken ledger projections."""
    result = {}
    for path in sorted(root.rglob("*")):
        meta = path.lstat()
        mode = stat.S_IMODE(meta.st_mode)
        if stat.S_ISLNK(meta.st_mode):
            kind, data = "link", os.readlink(path)
        elif stat.S_ISDIR(meta.st_mode):
            kind, data = "dir", None
        elif stat.S_ISREG(meta.st_mode):
            kind, data = "file", path.read_bytes()
        elif stat.S_ISFIFO(meta.st_mode):
            kind, data = "fifo", None
        else:
            raise AssertionError(f"unexpected fixture type: {path}")
        result[str(path.relative_to(root))] = (kind, mode, data, meta.st_ino, meta.st_mtime_ns)
    return result


def restore(root, saved):
    for path in root.iterdir():
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
    for relative, (kind, mode, data, _, _) in saved.items():
        path = root / relative
        if kind == "dir":
            path.mkdir()
        elif kind == "file":
            path.write_bytes(data)
        elif kind == "link":
            path.symlink_to(data)
        elif kind == "fifo":
            os.mkfifo(path, mode)
        if kind != "link":
            path.chmod(mode)


def stop(child):
    if child.poll() is None:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    child.communicate(timeout=2)
    CHILDREN.discard(child)


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.serial, self.count, self.failures = 0, 0, []

    def run(self, label, fn):
        self.count += 1
        try:
            fn()
        except Exception as exc:
            self.failures.append(f"{label}: {type(exc).__name__}: {exc}")
        finally:
            for child in tuple(CHILDREN):
                stop(child)

    def fixture(self, example=False, agents=None):
        self.serial += 1
        w = self.root / f"workspace {self.serial}"
        args = ["init", w, *([] if example else ["--blank"])]
        if agents:
            args += ["--agents", agents]
        result = python(args, self.root)
        assert result[0] == 0, result
        (w / "state/.workspace.lock").unlink(missing_ok=True)
        return w

    def spawn(self, args, cwd):
        command = ([str(Path(sys.executable).resolve()), "-c",
                    "from runner import coven; import sys; raise SystemExit(coven.main(sys.argv[1:]))"]
                   if self.oracle_only else [self.binary])
        child = subprocess.Popen([*command, *map(str, args)], cwd=cwd, env=ENV,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, start_new_session=True)
        CHILDREN.add(child)
        return child

    def candidate(self, args, cwd):
        if self.oracle_only:
            return python(args, cwd)
        child = self.spawn(args, cwd)
        try:
            out, err = child.communicate(timeout=8)
            return child.returncode, out.decode(), err.decode()
        finally:
            stop(child)

    def success(self, w, implicit=False):
        saved = snapshot(w)
        agents = json.loads((w / "coven.json").read_bytes())["agents"]
        generated = {"dashboard.md", "tmux/coven.toml"}
        generated.update(f"prompts/{agent}.start.txt" for agent in agents)
        generated.update(f"prompts/{agent}.md" for agent in agents if f"prompts/{agent}.md" not in saved)
        args = ["refresh", *([] if implicit else [w])]
        expected = None
        for native in (False, True):
            restore(w, saved)
            results = []
            for _ in range(2):
                before = snapshot(w)
                result = self.candidate(args, w) if native else python(args, w)
                assert result[0] == 0 and result[2] == "", result
                after = snapshot(w)
                allowed = generated | {"state/.workspace.lock", "state", "tmux", "prompts"}
                # Once a missing role has been created, it is authored state too.
                allowed -= {f"prompts/{agent}.md" for agent in agents
                            if f"prompts/{agent}.md" in before}
                for key in before.keys() | after.keys():
                    if key not in allowed:
                        assert before.get(key) == after.get(key), f"authored entry changed: {key}"
                for key in ("state", "tmux", "prompts"):
                    if key in before:
                        assert after[key][:4] == before[key][:4], f"parent replaced: {key}"
                for key in generated & before.keys():
                    assert after[key][1] == before[key][1], f"generated mode changed: {key}"
                lock = "state/.workspace.lock"
                if lock in before:
                    assert after[lock] == before[lock], "persistent lock inode replaced"
                else:
                    assert after[lock][:3] == ("file", 0o600, b""), after[lock]
                assert after["dashboard.md"][3] != before.get("dashboard.md", (0, 0, 0, None))[3], "dashboard not atomically replaced"
                tomllib.loads((w / "tmux/coven.toml").read_text())
                # New-file defaults may be more restrictive in native storage;
                # exact bytes/types and all pre-existing modes remain required.
                results.append((result, {k: (v[0], v[2]) for k, v in after.items()}))
            assert results[0] == results[1], "repeat refresh changed output bytes"
            if native:
                assert results == expected, f"Python/native output differs: {self.difference(results, expected)}"
            else:
                expected = results

    @staticmethod
    def difference(actual, expected):
        a, b = actual[0][1], expected[0][1]
        return repr({"result": (actual[0][0], expected[0][0]),
                     "files": {k: (a.get(k), b.get(k)) for k in a.keys() | b.keys()
                               if a.get(k) != b.get(k)}})[:12000]

    def reject(self, w, extra=(), parser=False, native_pattern=None, python_pattern=None):
        saved = snapshot(w)
        for native in (False, True):
            restore(w, saved)
            before = snapshot(w)
            args = ["refresh", w, *extra]
            result = self.candidate(args, w) if native else python(args, w)
            assert result[0] == (2 if parser else 1) and not result[1] and result[2], result
            pattern = native_pattern if native and not self.oracle_only else python_pattern
            if pattern:
                assert re.search(pattern, result[2], re.I), (pattern, result)
            assert snapshot(w) == before, f"refusal had effects: {result}"
            if "state/.workspace.lock" not in saved:
                assert not (w / "state/.workspace.lock").exists(), "preflight created first lock"

    def metadata(self, w, **changes):
        path = w / "coven.json"
        team = json.loads(path.read_bytes())
        team.update(changes)
        path.write_bytes(storage.encode_json(team))

    def ordinary(self, example, agents, custom=False):
        w = self.fixture(example, agents)
        roster = json.loads((w / "coven.json").read_bytes())["agents"]
        for agent in roster:
            (w / f"prompts/{agent}.md").unlink()
        if custom:
            path = w / f"prompts/{roster[0]}.md"
            path.write_bytes(b"Custom role\r\nDo not regenerate.\n")
            path.chmod(0o640)
        for relative in ("dashboard.md", "tmux/coven.toml", f"prompts/{roster[0]}.start.txt"):
            (w / relative).write_bytes(b"stale\n")
            (w / relative).chmod(0o640)
        (w / "work/authored").write_bytes(b"user work\r\n")
        (w / "output/authored").write_bytes(b"user output\n")
        self.success(w, implicit=custom)

    def runtime(self, mux, command):
        w = self.fixture(agents="worker")
        self.metadata(w, name='Quoted "team" \\ \t\n\u03bb \U0001f98a\x7f')
        (w / "coven.toml").write_text(
            "[runtime]\nmultiplexer = " + coven.toml_string(mux) +
            "\nopencode_command = " + coven.toml_string(command) + "\n")
        self.success(w)

    def missing(self, relatives):
        w = self.fixture(example=True)
        for relative in relatives:
            path = w / relative
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)
        self.success(w)
        for relative in relatives:
            if relative == "state":
                assert set((w / "state").iterdir()) == {w / "state/.workspace.lock"}, "state backfilled beyond coordination lock"
            else:
                assert not (w / relative).exists(), f"authored state recreated: {relative}"

    def legacy(self):
        w = self.fixture(example=True)
        (w / "state/tasks.json").write_bytes(
            b' [ {"id":"T001","title":"Plan","status":"legacy","owner":null,'
            b'"notes":"Keep custom fields","x":{"zero":-0.0,"big":18446744073709551617}},'
            b'{"id":"plan-checkpoint","title":"Authored","status":"blocked"} ]\r\n')
        (w / "checkpoints/plan-approval.md").write_text(
            "# Plan\nStatus: changes_requested\n\n## User decision\n- [ ] Approved\n"
            "- [x] Rejected / needs changes\n\n## Notes\nKeep rationale.\n")
        (w / "logs/events.jsonl").write_bytes(
            b'{"type":"checkpoint.approved","checkpoint":"plan-approval"}\n')
        self.success(w)
        assert b"changes_requested" in (w / "dashboard.md").read_bytes()

    def ledger(self, projection):
        w = self.fixture(example=True)
        revision = coven.read_task_snapshot(w)[0]
        result = python(["state", "import-tasks", w, "--expect-revision", revision, "--json"], w)
        assert result[0] == 0, result
        (w / "state/.workspace.lock").unlink()
        path = w / "state/tasks.json"
        if projection == "pending":
            (w / task_ledger.MARKER).unlink()
        elif projection != "intact":
            path.unlink()
            if projection == "corrupt":
                path.write_bytes(b"not JSON\xff\n")
            elif projection == "symlink":
                target = self.root / f"outside-{self.serial}"
                target.write_bytes(b"private outside content\n")
                path.symlink_to(target)
            elif projection == "fifo":
                os.mkfifo(path)
        self.success(w)
        if projection == "symlink":
            assert target.read_bytes() == b"private outside content\n"

    def control(self, kind):
        w = self.fixture(agents="worker")
        store = actions.ActionStore(w)
        if kind == "valid":
            with storage.transport_gate(w, w / "state/.runtime-transport.lock", exclusive=True) as gate:
                with storage.workspace_lock(w, w / "state/.workspace.lock") as held:
                    store.enable(held, gate=gate)
            (w / "state/.workspace.lock").unlink()
            self.success(w)
        else:
            if kind == "corrupt":
                (w / actions.CONTROL).write_bytes(b"{}")
            else:
                (w / actions.BUNDLES).mkdir()
                (w / actions.BUNDLES / "orphan.json").write_bytes(b"{}")
            self.reject(w)

    def invalid(self, relative, value, kind="file"):
        w = self.fixture(agents="worker")
        path = w / relative
        if kind != "file":
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink(missing_ok=True)
            if kind == "fifo":
                os.mkfifo(path)
            else:
                target = self.root / f"outside-{self.serial}"
                target.write_bytes(b"outside sentinel\n")
                path.symlink_to(target)
        else:
            path.write_bytes(value)
        self.reject(w)
        if kind == "link":
            assert target.read_bytes() == b"outside sentinel\n"

    def logs(self, filename, checkpoint):
        w = self.fixture(agents="worker")
        records = [{"type": "checkpoint.approved", "checkpoint": "safe-custom"},
                   {"type": "checkpoint.requested", "checkpoint": checkpoint}]
        (w / f"logs/{filename}.jsonl").write_bytes(
            b"".join((json.dumps(row) + "\n").encode() for row in records))
        self.reject(w, native_pattern=r"invalid_checkpoint_identifier",
                    python_pattern=r"Invalid checkpoint ID")

    def log_order(self):
        w = self.fixture(agents="worker")
        (w / "logs/messages.jsonl").write_bytes(
            b'{"type":"checkpoint.requested","checkpoint":"../message-first"}\n')
        (w / "logs/events.jsonl").write_bytes(b"not-json\n")
        # Metadata error in messages must win over a distinct events parse error.
        self.reject(w, native_pattern=r"invalid_checkpoint_identifier",
                    python_pattern=r"Invalid checkpoint ID")

    def serialization(self):
        if not Path("/proc/self/fd").exists():
            print("refresh serialization: skipped (requires Linux /proc child-state handshake)")
            return
        w = self.fixture(agents="worker")
        args = ["refresh", w]
        # The parent is the Python lock holder; a child cannot inherit its token.
        with coven.workspace_mutation(w):
            before = snapshot(w)
            child = self.spawn(args, ROOT)
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
            assert snapshot(w) == before, "unlocked refresh published outputs"
            # Simulate another writer's committed bytes while holding the common
            # lock, after refresh's unlocked draft and before its locked render.
            (w / "state/tasks.json").write_bytes(storage.encode_json([
                {"id": "after-lock", "title": "Task committed while refresh waited", "status": "pending"}]))
        try:
            out, err = child.communicate(timeout=8)
            assert child.returncode == 0 and not err, (child.returncode, out, err)
        finally:
            stop(child)
        assert b"Task committed while refresh waited" in (w / "dashboard.md").read_bytes(), "stale unlocked dashboard"
        assert (w / "dashboard.md").read_bytes() == coven.render_dashboard(w).encode()

    def corpus(self):
        for example in (False, True):
            for agents in (None, "orchestrator,architect,implementer,reviewer,guest"):
                self.run(f"ordinary/{example}/{agents}", lambda e=example, a=agents: self.ordinary(e, a))
            self.run(f"custom/{example}", lambda e=example: self.ordinary(e, "clippy,jamal,guest", True))
        for mux in ("herdr", "tmux"):
            for command in ("  oc  ", ' oc --title "\u03bb \U0001f98a" --path "a\\b" ',
                            ' runner "quoted"\tline\nnext\x7f '):
                self.run(f"runtime/{mux}/{command!r}", lambda m=mux, c=command: self.runtime(m, c))
        for relatives in (("state/tasks.json", "state/agents.json", "state/lead.json"),
                          ("state",), ("goal.md", "spec.md", "work/README.md", "output/README.md",
                                        "checkpoints/plan-approval.md", "checkpoints/pre-delivery-review.md"),
                          ("agents/clippy.md",), ("logs",), ("tmux", "prompts", "dashboard.md")):
            # Generated entries are intentionally recreated, unlike authored state.
            if "tmux" in relatives:
                def generated():
                    w = self.fixture()
                    for name in ("tmux", "prompts"):
                        shutil.rmtree(w / name)
                    (w / "dashboard.md").unlink()
                    self.success(w)
                self.run("missing/generated", generated)
            else:
                self.run(f"missing/{relatives}", lambda r=relatives: self.missing(r))
        self.run("legacy/custom-fields-and-revoked-decision", self.legacy)
        for projection in ("intact", "missing", "corrupt", "symlink", "fifo", "pending"):
            self.run(f"ledger/{projection}", lambda p=projection: self.ledger(p))
        for kind in ("valid", "corrupt", "orphan"):
            self.run(f"runtime-control/{kind}", lambda k=kind: self.control(k))
        for relative, value in (("goal.md", b"\xff"), ("coven.toml", b"invalid !"),
                                ("state/tasks.json", b"{"), ("coven.json", b"{}"),
                                ("checkpoints/not.valid.MD", b"bad grammar"),
                                ("checkpoints/custom.md", b"\xff"),
                                ("logs/messages.jsonl", b"not-json\n"), ("logs/events.jsonl", b"[]\n")):
            self.run(f"preflight/{relative}", lambda r=relative, v=value: self.invalid(r, v))
        for relative in ("goal.md", "tmux/coven.toml", "prompts/worker.md", "dashboard.md",
                         "state/tasks.json", "logs/messages.jsonl", "checkpoints/custom.md", "state/.workspace.lock"):
            for kind in ("link", "fifo"):
                self.run(f"unsafe/{relative}/{kind}", lambda r=relative, k=kind: self.invalid(r, None, k))
        for filename in ("messages", "events"):
            for checkpoint in ("../goal", None, {}, 23, ""):
                self.run(f"log-checkpoint/{filename}/{checkpoint}", lambda f=filename, c=checkpoint: self.logs(f, c))
        self.run("messages-first", self.log_order)
        for example in ("missing-example", {"invalid": "metadata"}):
            def bad_example(example=example):
                w = self.fixture()
                self.metadata(w, example=example)
                self.reject(w)
            self.run(f"invalid-example/{example}", bad_example)
        for extra in (("--json",), ("extra",), ("extra", "operand")):
            self.run(f"parser/{extra}", lambda e=extra: self.reject(self.fixture(), e, parser=True))
        self.run("python-held-native-refresh-serialization", self.serialization)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    assert args.binary or args.oracle_only, "--binary is required unless --oracle-only"
    binary = str(Path(args.binary).resolve()) if args.binary else None
    def expired(signum, frame):
        raise CorpusTimeout("refresh corpus exceeded 90 seconds")
    signal.signal(signal.SIGALRM, expired)
    signal.alarm(90)
    try:
        with tempfile.TemporaryDirectory(prefix="coven-refresh-") as folder:
            root = Path(folder)
            empty = root / "empty-path"
            empty.mkdir()
            ENV.update(PATH=str(empty), PYTHONPATH=str(ROOT))
            suite = Suite(root, binary, args.oracle_only)
            with mock.patch.dict(os.environ, ENV, clear=True):
                suite.corpus()
            print(f"refresh parity: {suite.count} cases, {len(suite.failures)} failures")
            for failure in suite.failures:
                print(failure, file=sys.stderr)
            return bool(suite.failures)
    finally:
        signal.alarm(0)
        for child in tuple(CHILDREN):
            stop(child)


if __name__ == "__main__":
    raise SystemExit(main())
