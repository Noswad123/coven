"""TEMPORARY four-route parser/handler oracle, not supervisor acceptance.

Only disposable real Git and a compiled, explicitly pinned producer run. Native
children have no Python on PATH. Each differential pair uses the SAME producer
inode/path/bytes; fingerprints, facts, hashes and authority bytes are never masked.
Only adapter receipt clocks/durations are normalized. New native registration
UUID/time are fed back into Python, not normalized out of reports or raw records.
Private fault gates listed below must be supplied by domain/transport owners.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, chdir, redirect_stderr, redirect_stdout
from copy import deepcopy
from datetime import datetime
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import coven, repositories as repos, storage, launch, workers
from test_djinn_observation import native_fixture, change

ROUTES = ("repository inspect [workspace] [--verify] [--json]",
          "repository register [workspace] --path PATH --base OID --delivery-target REF --expect-repository-revision TOKEN [--json]",
          "worker readiness [workspace] [--adapter djinn] [--profile research-readonly] [--executable FILE] [--timeout N] [--max-output-bytes N] [--json]",
          "observe djinn --executable FILE --folder FOLDER [--timeout N] [--max-output-bytes N] [--json]")
OWNER_GATES = (
    "Transport owner: private supervisor bootstrap, actual NativeRunner explicit supervisor selector, SIGCHLD, caller death, watchdog, pipe descendants, cleanup and exact combined-output boundary",
    "Repository owner: deterministic both-order same-flock CAS barriers, process death, postpublication reread/fsync/admission failures and known-result uncertainty",
    "Repository owner: instrument native probe ordering before index reads, 20s aggregate/3s per-probe deadlines; CLI verifies refusal/preservation, not private timing seams",
    "Identity owner: same-metadata executable rewrites, descriptor pinning and hashing limits; CLI covers alias retarget precedence",
    "Platform: Linux fixture FD inspection only; no macOS lifecycle validation claimed",
)
SECRET = "PRIVATE_NATIVE_TRANSCRIPT"
KEYS = {"PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"}


class Escape(BaseException):
    pass


def forbidden(*args, **kwargs):
    raise Escape((args, kwargs))


def tree(root):
    """No symlink traversal; protect regular bytes, modes, inodes and mtimes."""
    result = {}
    def visit(path):
        info = path.lstat()
        if stat.S_ISDIR(info.st_mode):
            for child in sorted(path.iterdir()):
                visit(child)
        else:
            data = path.read_bytes() if stat.S_ISREG(info.st_mode) else os.readlink(path)
            result[str(path.relative_to(root))] = (info.st_mode, info.st_ino, info.st_mtime_ns, data)
    visit(root)
    return result


def exact(a, b):
    # Equality must not accept a typed boolean as an integer.
    assert json.dumps(a, sort_keys=True, ensure_ascii=True, allow_nan=False) == json.dumps(
        b, sort_keys=True, ensure_ascii=True, allow_nan=False), (a, b)


def receipts(value, route):
    value = deepcopy(value)
    if route == "worker":
        for probe in value["probes"]:
            datetime.fromisoformat(probe.pop("observed_at"))
            assert probe.pop("duration_ms") >= 0
    if route == "observe":
        for key in ("started_at", "observed_at"):
            datetime.fromisoformat(value.pop(key))
        assert value.pop("duration_ms") >= 0
    return value


def python_cli(args, cwd, env, entropy=None):
    out, err = io.StringIO(), io.StringIO()
    with ExitStack() as stack:
        stack.enter_context(chdir(cwd))
        stack.enter_context(redirect_stdout(out))
        stack.enter_context(redirect_stderr(err))
        stack.enter_context(patch.dict(os.environ, env, clear=True))
        for target, name in ((socket, "socket"), (os, "system"), (launch, "execute"),
                             (launch, "stop"), (coven, "send_to_agent_pane")):
            stack.enter_context(patch.object(target, name, forbidden))
        if entropy:
            ident, stamp = entropy
            stack.enter_context(patch.object(repos.uuid, "uuid4", return_value=uuid.UUID(ident)))
            class Clock(datetime):
                @classmethod
                def now(cls, tz=None):
                    return datetime.fromisoformat(stamp)
            stack.enter_context(patch.object(repos, "datetime", Clock))
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    return code, out.getvalue().encode(), err.getvalue().encode()


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts = Counter()
        # Resolve tools from the caller, before constructing any sanitized child env.
        self.git = Path(shutil.which("git") or "missing").resolve(strict=True)
        self.rustc = Path(shutil.which("rustc") or "missing").absolute()
        self.env = {"PATH": "", "HOME": str(root / "home"), "SHELL": "/bin/sh",
                    "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TERM": "dumb",
                    "PYTHONDONTWRITEBYTECODE": "1", "COVEN_MULTIPLEXER": "",
                    "DJINN_HOME": "/must-not-inherit", "OPENAI_API_KEY": SECRET,
                    "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.fsmonitor",
                    "GIT_CONFIG_VALUE_0": "/must-not-run"}
        (root / "home").mkdir()
        self.git_env = {"PATH": str(self.git.parent), "HOME": "/dev/null",
                        "XDG_CONFIG_HOME": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
                        "GIT_CONFIG_SYSTEM": "/dev/null", "GIT_CONFIG_GLOBAL": "/dev/null",
                        "LC_ALL": "C", "GIT_TERMINAL_PROMPT": "0", "GIT_ALLOW_PROTOCOL": "",
                        "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                        "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        version = self.git_run(root, "--version")
        assert version.startswith(b"git version 2.56."), version
        self.fixture = root / "producer-fixture"
        self.fixture.mkdir()
        self.exe = self.fixture / "producer-\u96ea"
        subprocess.run([str(self.rustc), "--edition=2021", "-o", str(self.exe),
                        str(ROOT / "tests/support/process_integrations_producer.rs")],
                       check=True, capture_output=True, timeout=60)
        self.configure("help")

    def git_run(self, cwd, *args):
        assert cwd.is_relative_to(self.root), "Git setup outside disposable root"
        return subprocess.run([str(self.git), "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", *map(str, args)],
                              cwd=cwd, env=self.git_env, check=True, capture_output=True, timeout=10).stdout

    def workspace(self, name):
        w = self.root / name
        w.mkdir()
        (w / "state").mkdir()
        # Deliberately malformed mutable authorities: these routes need no source gate.
        for name, data in (("coven.json", b"corrupt\xff"), ("coven.toml", b"[broken"),
                           ("state/tasks.json", b"null"), ("state/agents.json", b"[]"),
                           ("state/execution.json", b"authority-sentinel")):
            (w / name).write_bytes(data)
        return w

    def checkout(self, name, fmt="sha1"):
        repo = self.root / name
        repo.mkdir()
        self.git_run(repo, "init", "--quiet", "--object-format=" + fmt)
        (repo / "source").write_text("base\n")
        (repo / "subtree").mkdir()
        (repo / "subtree/file").write_bytes(b"nested closure is not promised\n")
        (repo / "link").symlink_to("source")
        self.git_run(repo, "add", ".")
        self.git_run(repo, "commit", "--quiet", "-m", "disposable")
        return repo, self.git_run(repo, "rev-parse", "HEAD").decode().strip()

    def configure(self, mode, variant="", payload=b"", code=0, stderr=b"", delay_ms=0):
        for key, value in (("mode", mode.encode()), ("variant", variant.encode()),
                           ("payload", payload), ("exit", str(code).encode()), ("stderr", stderr),
                           ("delay_ms", str(delay_ms).encode())):
            (self.fixture / key).write_bytes(value)

    def clear_log(self):
        (self.fixture / "calls.jsonl").unlink(missing_ok=True)

    def calls(self):
        path = self.fixture / "calls.jsonl"
        return [json.loads(line) for line in path.read_bytes().splitlines()] if path.exists() else []

    def logs(self, calls, cwd, expected):
        exact([call["argv"] for call in calls], expected)
        for call in calls:
            assert call["cwd"] == str(cwd) and call["stdin_bytes"] == 0, call
            exact(call["environment"], {k: v for k, v in self.env.items() if k in KEYS})
            # Private completion/liveness/workspace descriptors must not reach the child.
            assert not any(".workspace.lock" in fd or "repository.json" in fd for fd in call["fds"]), call
            if sys.platform.startswith("linux"):
                assert len(call["fds"]) == 3, call

    def invoke(self, args, cwd, env=None):
        if self.oracle_only:
            return python_cli(args, cwd, env or self.env)
        result = subprocess.run([self.binary, *map(str, args)], cwd=cwd, env=env or self.env,
                                capture_output=True, timeout=48)
        return result.returncode, result.stdout, result.stderr

    def check(self, args, cwd, code=0, category="read", git=False, mutation=False,
              expected_calls=None, probe_cwd=None, reset=None, profile=False):
        args = list(map(str, args))
        env = self.env | ({"PATH": str(self.git.parent)} if git else {})
        before = tree(cwd)
        authority = cwd / "state/repository.json"
        old = authority.read_bytes() if authority.is_file() else None
        lock = cwd / "state/.workspace.lock"
        had_lock = lock.exists()
        self.clear_log()
        actual = self.invoke(args, cwd, env)
        expected_code = 1 if profile and not self.oracle_only else code
        assert actual[0] == expected_code, (args, actual)
        assert SECRET.encode() not in actual[1] + actual[2], (args, actual)
        calls = self.calls()
        if expected_calls is not None:
            self.logs(calls, probe_cwd or cwd, expected_calls)
        if mutation:
            after = tree(cwd)
            assert {k: v for k, v in after.items() if k in before} == before
            assert set(after) - set(before) <= {"state/repository.json", "state/.workspace.lock"}
            assert stat.S_IMODE(authority.stat().st_mode) == 0o600
        else:
            assert tree(cwd) == before, (args, "workspace changed")
        parsed = json.loads(actual[1]) if actual[1].lstrip().startswith(b"{") else None
        if args[:2] == ["worker", "readiness"] and parsed is not None and "\u96ea" in str(cwd):
            assert "\u96ea".encode() in actual[1], "readiness JSON must preserve Unicode labels"
        if args[0] in ("repository", "observe") and parsed is not None:
            actual[1].decode("ascii")  # These renderers explicitly ASCII-escape labels.
        if args[:2] == ["worker", "readiness"] and parsed is None and not actual[2]:
            assert b"\x1b" not in actual[1], "human report leaked terminal controls"
            if "--executable" in args:
                label = args[args.index("--executable") + 1]
                if "\n" in label:
                    assert label.encode() not in actual[1], "human executable label injected a newline"
        if not self.oracle_only:
            native_data = authority.read_bytes() if mutation else None
            entropy = None
            if mutation and old is None:
                record = json.loads(native_data)
                entropy = record["registration_id"], record["registered_at"]
                authority.unlink()
                if not had_lock:
                    lock.unlink()
            if reset:
                reset()
            self.clear_log()
            expected = python_cli(args, cwd, env, entropy)
            if profile:
                assert expected[0] == code, expected
                assert actual[2] and not actual[1], actual
            else:
                assert expected[0] == actual[0], (args, actual, expected)
                expected_json = json.loads(expected[1]) if parsed is not None else None
                if parsed is not None:
                    exact(receipts(parsed, args[0]), receipts(expected_json, args[0]))
                    assert not actual[2] and not expected[2]
                elif code == 0 or args[:2] == ["worker", "readiness"] and not actual[2]:
                    assert actual[1:] == expected[1:], (args, actual, expected)
                else:
                    assert not actual[1] and actual[2] and not expected[1] and expected[2]
            if mutation:
                assert authority.read_bytes() == native_data, "raw authority bytes differ (no canonicalization allowed)"
            if expected_calls is not None:
                self.logs(self.calls(), probe_cwd or cwd, expected_calls)
        self.counts["profile_refusal" if profile else category] += 1
        return parsed

    @staticmethod
    def registration(w, repo, base, target="refs/heads/delivery"):
        return ["repository", "register", w, "--path", repo, "--base", base,
                "--delivery-target", target, "--expect-repository-revision", repos.UNREGISTERED, "--json"]

    def repositories(self):
        # Whole flow starts with the selected CLI creating its own disposable root.
        initialized = self.root / "cli-created-workspace"
        code, out, err = self.invoke(["init", initialized], self.root)
        assert code == 0 and not err and initialized.is_dir(), (code, out, err)
        repo, base = self.checkout("cli-created-checkout")
        protected = tree(repo)
        human_default = self.registration(initialized, repo, base)
        human_default.pop(2)  # Optional workspace defaults to the actual process cwd.
        human_default.pop()  # Real pretty renderer, not a JSON-only authoring shortcut.
        self.check(human_default, initialized, git=True,
                   mutation=True, category="initialized_registration")
        self.check(human_default, initialized, category="human_retry")
        self.check(["repository", "inspect", "--verify"], initialized, git=True)
        assert tree(repo) == protected
        w = self.workspace("fresh-no-git")
        self.check(self.registration(w, repo, base), w, code=1, category="fresh_no_git")
        assert tree(repo) == protected
        args = self.registration(w, repo, base)
        for flag, value in (("--base", "HEAD"), ("--delivery-target", "refs/heads/a..b"),
                            ("--path", str(repo) + "/../other"),
                            ("--expect-repository-revision", "repository-sha256:" + "0" * 64)):
            invalid = args.copy()
            invalid[invalid.index(flag) + 1] = value
            self.check(invalid, w, code=1, category="registration_admission")
        for fmt in ("sha1", "sha256"):
            repo, base = self.checkout("checkout-" + fmt, fmt)
            w = self.workspace("workspace-" + fmt)
            self.check(["repository", "inspect", "--verify", "--json"], w)
            (repo / "source").write_bytes(b"staged\n")
            self.git_run(repo, "add", "source")
            (repo / "source").write_bytes(b"unstaged\n")
            (repo / "odd\nname").write_bytes(b"untracked\n")
            protected = tree(repo)
            args = self.registration(w, repo, base)
            result = self.check(args, w, git=True, mutation=True, category="registration")
            assert result["observations"]["dirty"] and result["registration"]["object_format"] == fmt
            raw = (w / "state/repository.json").read_bytes()
            assert result["revision"] == "repository-sha256:" + hashlib.sha256(
                b"coven.repository-registration:v1\0" + raw).hexdigest()
            self.check(["repository", "inspect", "--json"], w)
            self.check(["repository", "inspect", w, "--verify", "--json"], w, git=True)
            self.check(["repository", "inspect", w], w)
            self.check(args, w, category="exact_retry")  # No Git in PATH.
            for flag, value in (("--path", repo / "different"), ("--base", "f" * len(base)),
                                ("--delivery-target", "refs/heads/other"),
                                ("--expect-repository-revision", result["revision"])):
                conflict = args.copy()
                conflict[conflict.index(flag) + 1] = value
                self.check(conflict, w, code=1, category="immutable_conflict")
            assert tree(repo) == protected, "Git bytes/modes/inodes/mtimes changed"
            # Independent raw-byte revision domain and exact 64 KiB inclusive cap.
            compact = json.dumps(json.loads(raw), separators=(",", ":")).encode()
            for size in (len(compact), 65536, 65537):
                data = compact + b" " * (size - len(compact))
                (w / "state/repository.json").write_bytes(data)
                report = self.check(["repository", "inspect", w, "--json"], w,
                                    code=1 if size > 65536 else 0, category="raw_authority")
                if size <= 65536:
                    assert report["revision"] == "repository-sha256:" + hashlib.sha256(
                        b"coven.repository-registration:v1\0" + data).hexdigest()
                else:
                    assert report["revision"] is None and "registration" not in report
            (w / "state/repository.json").write_bytes(raw)
            # Deleted checkout still replays with corrupted mutable source and no Git.
            shutil.rmtree(repo)
            self.check(args, w, category="deleted_retry")
            unavailable = self.check(["repository", "inspect", w, "--verify", "--json"], w, code=1)
            assert unavailable["registration"] == result["registration"]

        repo, base = self.checkout("linked-main")
        linked = self.root / "linked-checkout"
        self.git_run(repo, "worktree", "add", "--quiet", "--detach", linked, base)
        w = self.workspace("linked-workspace")
        protected = tree(repo), tree(linked)
        report = self.check(self.registration(w, linked, base), w, git=True, mutation=True, category="linked")
        assert report["registration"]["git_dir"] != report["registration"]["common_dir"]
        self.check(["repository", "inspect", w, "--verify", "--json"], w, git=True)
        assert (tree(repo), tree(linked)) == protected
        self.git_run(repo, "symbolic-ref", "HEAD", "refs/heads/unborn-fixture")
        w = self.workspace("unborn-workspace")
        protected = tree(repo)
        report = self.check(self.registration(w, repo, base), w, git=True, mutation=True, category="unborn")
        assert report["observations"]["head"] == {"status": "unborn", "ref": "refs/heads/unborn-fixture"}
        assert tree(repo) == protected
        alias = self.root / "checkout-alias"
        alias.symlink_to(linked)
        w = self.workspace("alias-workspace")
        args = self.registration(w, alias, base)
        first = self.check(args, w, git=True, mutation=True, category="symlink")
        alias.unlink()
        alias.symlink_to(repo)
        replay = self.check(args, w, category="symlink_retry")
        assert replay["registration"] == first["registration"]
        self.check(["repository", "inspect", w, "--verify", "--json"], w, git=True, code=1)
        # A known result survives failure to acquire the SAME workspace flock.
        lock = w / "state/.workspace.lock"
        with lock.open("r+b") as held:
            fcntl.flock(held, fcntl.LOCK_EX)
            replay = self.check(args, w, code=2, category="known_lock_uncertainty")
            assert replay["delivery"] == {"outcome": "durability_uncertain"} and replay["replayed"]

        w = self.workspace("malformed-authority")
        for raw in (b"null", b"{}", b"{", b"\xff", b'{"x":1,"x":2}'):
            (w / "state/repository.json").write_bytes(raw)
            for verify in ([], ["--verify"]):
                value = self.check(["repository", "inspect", w, *verify, "--json"], w, code=1, category="malformed")
                assert value["revision"] is None and "registration" not in value
            self.check(self.registration(w, linked, base), w, code=1, category="malformed")

        # Reuse one real repository; no repeated full Git workflows for exclusions.
        repo, base = self.checkout("excluded-checkout")
        linked = self.root / "excluded-linked"
        self.git_run(repo, "worktree", "add", "--quiet", "--detach", linked, base)
        linked_git = Path(self.git_run(linked, "rev-parse", "--absolute-git-dir").decode().strip())
        config = repo / ".git/config"
        original = config.read_bytes()
        helper = self.root / "forbidden-helper"
        marker = self.root / "helper-ran"
        helper.write_text("#!/bin/sh\nprintf ran > '" + str(marker) + "'\nexit 99\n")
        helper.chmod(0o700)
        variants = [("split-config", config, original + b"\n[core]\n splitIndex = true\n"),
                    ("partial", config, original + b"\n[extensions]\n partialClone = origin\n"),
                    ("includes", config, original + b"\n[include]\n path = /must-not-read\n"),
                    ("filters", config, original + ("\n[filter \"hostile\"]\n clean = " + str(helper) + "\n").encode()),
                    ("promisor-config", config, original + b"\n[remote \"origin\"]\n promisor = true\n"),
                    ("worktree-config", config, original + b"\n[extensions]\n worktreeConfig = true\n"),
                    ("alternates", repo / ".git/objects/info/alternates", b"/must-not-fetch\n"),
                    ("promisor-case", repo / ".git/objects/pack/dead.PROMISOR", b""),
                    ("stale-case-common", repo / ".git/ShArEdInDeX.dead", b"stale"),
                    ("stale-case-linked", linked_git / "SHAREDINDEX.dead", b"stale")]
        for name, path, data in variants:
            path.write_bytes(data)
            w = self.workspace("exclude-" + name)
            before = tree(repo), tree(linked)
            self.check(self.registration(w, linked, base), w, git=True, code=1, category="git_exclusion")
            assert (tree(repo), tree(linked)) == before and not marker.exists()
            if path == config:
                config.write_bytes(original)
            else:
                path.unlink()
        for checkout in (repo, linked):
            self.git_run(checkout, "update-index", "--split-index")
            before = tree(repo), tree(linked)
            w = self.workspace("real-split-" + checkout.name)
            self.check(self.registration(w, checkout, base), w, git=True, code=1, category="git_exclusion")
            assert (tree(repo), tree(linked)) == before
            self.git_run(checkout, "update-index", "--no-split-index")
            git_dir = Path(self.git_run(checkout, "rev-parse", "--absolute-git-dir").decode().strip())
            for shared in git_dir.glob("sharedindex.*"):
                shared.unlink()
        # Readonly verify must enforce the same exclusions, not silently ignore them.
        w = self.workspace("verify-exclusions")
        self.check(self.registration(w, repo, base), w, git=True, mutation=True, category="registration")
        config.write_bytes(original + b"\n[include]\n path = /must-not-read\n")
        before = tree(repo)
        report = self.check(["repository", "inspect", w, "--verify", "--json"], w,
                            git=True, code=1, category="verify_exclusion")
        assert report["registration"]["base_commit"] == base and tree(repo) == before
        config.write_bytes(original)

        # Local fsmonitor and inherited overrides never execute a helper. This
        # supported configuration is not an excuse to skip actual Git verification.
        config.write_bytes(original + ("\n[core]\n fsmonitor = " + str(helper) + "\n").encode())
        before = tree(repo)
        report = self.check(["repository", "inspect", w, "--verify", "--json"], w,
                            git=True, category="no_helper")
        assert report["observations"]["available"] and not marker.exists() and tree(repo) == before

    def races(self):
        repo, base = self.checkout("race-checkout")
        protected = tree(repo)
        implementations = ("python",) if self.oracle_only else ("native", "python")
        for implementation in implementations:
            for identical in (True, False):
                w = self.workspace("race-" + implementation + str(identical))
                lock = w / "state/.workspace.lock"
                lock.touch(mode=0o600)
                before = tree(w)
                prefix = ([sys.executable, "-B", str(Path(__file__).resolve()), "--oracle-child"]
                          if implementation == "python" else [self.binary])
                env = self.env | {"PATH": str(self.git.parent)}
                children = []
                with lock.open("r+b") as held:
                    fcntl.flock(held, fcntl.LOCK_EX)
                    try:
                        for target in ("refs/heads/one", "refs/heads/one" if identical else "refs/heads/two"):
                            children.append(subprocess.Popen([*prefix, *map(str, self.registration(w, repo, base, target))],
                                cwd=w, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE))
                        time.sleep(0.4)
                        assert all(child.poll() is None for child in children), "flock must block publication"
                        assert not (w / "state/repository.json").exists()
                        fcntl.flock(held, fcntl.LOCK_UN)
                        responses = [child.communicate(timeout=45) for child in children]
                        assert sorted(child.returncode for child in children) == ([0, 0] if identical else [0, 1]), responses
                        winners = [json.loads(out) for child, (out, err) in zip(children, responses) if child.returncode == 0]
                        raw = (w / "state/repository.json").read_bytes()
                        revision = "repository-sha256:" + hashlib.sha256(b"coven.repository-registration:v1\0" + raw).hexdigest()
                        assert all(value["revision"] == revision for value in winners)
                        if identical:
                            assert sorted(value["replayed"] for value in winners) == [False, True]
                        after = tree(w)
                        assert {k: after[k] for k in before} == before
                        assert set(after) - set(before) == {"state/repository.json"}
                    finally:
                        for child in children:
                            if child.poll() is None:
                                child.kill()
                            child.communicate(timeout=5)
                self.counts["same_flock_cas_" + implementation] += 1
        assert tree(repo) == protected

    def readiness(self):
        w = self.workspace("readiness-\u96ea")
        base = ["worker", "readiness", "--executable", self.exe]
        all_calls = [["--help"], ["session", "--help"], ["session", "run", "--help"],
                     ["session", "status", "--help"], ["session", "watch", "--help"],
                     ["ask", "--help"], ["agents", "--help"], ["--version"]]
        for variant in ("ansi", "wrapped", "missing", "missing-failure"):
            self.configure("help", variant)
            missing = variant.startswith("missing")
            calls = [all_calls[i] for i in ([0, 5, 6, 7] if missing else range(8))
                     if not (variant == "wrapped" and i == 7)]
            code = 1 if variant == "missing" else 2
            value = self.check([*base, "--json"], w, code=code, category="readiness", expected_calls=calls)
            assert value["assessment"] == ("blocked" if code == 1 else "unknown")
            assert len(value["capabilities"]) == 11
            assert all(v == {"state": "unknown", "evidence_source": "unverified"} for v in value["capabilities"].values())
            self.check(base, w, code=code, category="human", expected_calls=calls)
            if variant == "ansi":
                (w / "coven.json").unlink()
                shutil.rmtree(w / "state")
        self.configure("help", "ansi")
        self.check([*base, w, "--adapter", "djinn", "--profile", "research-readonly",
                    "--timeout", "1", "--max-output-bytes", "4096", "--json"], w,
                   code=2, category="explicit_flags", expected_calls=all_calls)
        odd_label = self.fixture / "producer-\u96ea\n\x1b[31m"
        shutil.copyfile(self.exe, odd_label)
        odd_label.chmod(0o700)
        odd_args = ["worker", "readiness", "--executable", odd_label]
        self.check(odd_args, w, code=2, category="visible_label", expected_calls=all_calls)
        self.check(["worker", "readiness", "--executable", self.root / "absent-worker", "--json"],
                   w, code=2, category="missing_worker", expected_calls=[])
        # Default discovery must resolve only the controlled producer, never installed Djinn.
        (self.fixture / "djinn").symlink_to(self.exe)
        saved = self.env
        self.env = self.env | {"PATH": str(self.fixture)}
        try:
            self.check(["worker", "readiness", "--json"], w, code=2,
                       category="default_executable", expected_calls=all_calls)
        finally:
            self.env = saved

    def djinn(self):
        w = self.workspace("observation-caller")
        folder = self.root / "folder-\u96ea"
        folder.mkdir()
        # No producer artifact reading or workspace-source inspection is necessary.
        (folder / "djinn.toml").write_bytes(b"not a real Djinn folder")
        fixture = native_fixture(str(folder))
        observed = [("observed", fixture, 0)]
        for status, count, size, digest in (("missing", None, None, None),
                                           ("observed", 0, 0, hashlib.sha256(b"").hexdigest())):
            v = deepcopy(fixture)
            v["observation"].update(event_count=count, last_event=None, last_lifecycle=None)
            v["observation"]["sources"]["folder_events"].update(status=status, bytes=size, sha256=digest)
            observed.append(("events-" + status, v, 0))
        v = deepcopy(fixture)
        timestamp = "2026-10-24T11:30:00.12345678901234567890+01:00"
        v["observation"]["recorded_created_at"] = timestamp
        for key in ("last_event", "last_lifecycle"):
            v["observation"][key]["recorded_at"] = timestamp
        observed.append(("precision", v, 0))
        v = deepcopy(fixture)
        for target in (v, v["observation"], v["observation"]["last_event"], v["observation"]["sources"]["manifest"]):
            target["transcript"] = SECRET
        observed.append(("extensions", v, 0))
        observed += [("unavailable", fixture | {"assessment": "unavailable", "resolved_folder": None,
                                               "observation": None, "diagnostics": ["folder_missing"]}, 1),
                     ("incompatible", fixture | {"contract": "djinn.session.observation.v2"}, 2)]
        cases = [(name, json.dumps(value).encode(), code) for name, value, code in observed]
        for name, path, value in (
            ("typed-bool", ("capabilities", "live_status"), 0),
            ("schema-bool", ("schema_version",), True),
            ("id-mismatch", ("observation", "last_event", "session_id"), "agt_other"),
            ("invalid-id", ("observation", "recorded_native_session_id"), "ui-123"),
            ("invalid-date", ("observation", "recorded_created_at"), "2026-02-30T00:00:00Z"),
            ("event-error", ("observation", "last_lifecycle"), None),
            ("event-count-bool", ("observation", "event_count"), True),
            ("wrong-folder", ("selected_folder",), "/other")):
            cases.append((name, json.dumps(change(fixture, path, value)).encode(), 1))
        cases += [("duplicates", b'{"x":1,"x":2}', 1),
                  ("extension-duplicates", json.dumps(fixture)[:-1].encode() + b',"ext":{"x":1,"x":2}}', 1),
                  ("nonfinite", b'{"extension":NaN}', 1),
                  ("overflow", json.dumps(fixture)[:-1].encode() + b',"ext":1e999}', 1),
                  ("depth", b'[' * 33 + b'0' + b']' * 33, 1),
                  ("invalid-utf8", b'\xff', 1), ("trailing-json", b'{} {}', 1)]
        args = ["observe", "djinn", "--executable", self.exe, "--folder", folder]
        calls = [["session", "observe", str(folder), "--json"]]
        for name, payload, code in cases:
            self.configure("observe", payload=payload, stderr=SECRET.encode())
            before = tree(folder)
            result = self.check([*args, "--json"], w, code=code, category="djinn_" + ("observed" if code == 0 else "refusal"),
                                expected_calls=calls, probe_cwd=Path("/"))
            assert result["authorization"] == "none" and result["dispatchable"] is False
            assert result["native_lifecycle"] == result["job_completion"] == "unestablished"
            assert result["runtime_fact_freshness"] == "unknown" and tree(folder) == before
            if name == "precision":
                assert result["observation"]["last_event"]["recorded_at"] == timestamp
        self.configure("observe", payload=json.dumps(fixture).encode())
        self.check(args, w, category="human", expected_calls=calls, probe_cwd=Path("/"))
        self.check([*args, "--timeout", "1", "--max-output-bytes", "4096", "--json"], w,
                   category="explicit_flags", expected_calls=calls, probe_cwd=Path("/"))
        missing = args.copy()
        missing[missing.index("--executable") + 1] = self.root / "missing-executable"
        self.check([*missing, "--json"], w, code=1, category="missing_executable", expected_calls=[])
        missing_folder = str(self.root / "producer-owned-folder")
        missing = args.copy()
        missing[missing.index("--folder") + 1] = missing_folder
        self.configure("observe", payload=json.dumps(fixture | {
            "selected_folder": missing_folder, "resolved_folder": missing_folder}).encode())
        self.check([*missing, "--json"], w, category="producer_owned_folder",
                   expected_calls=[["session", "observe", missing_folder, "--json"]], probe_cwd=Path("/"))
        self.configure("observe", payload=b"bad JSON", code=9)
        report = self.check([*args, "--json"], w, code=1, category="transport_precedence",
                            expected_calls=calls, probe_cwd=Path("/"))
        assert report["diagnostics"] == ["producer_error"]
        self.configure("observe", payload=b"x" * 1025)
        report = self.check([*args, "--max-output-bytes", "1024", "--json"], w, code=1,
                            category="output_limit", expected_calls=calls, probe_cwd=Path("/"))
        assert report["diagnostics"] == ["output_limit"]
        self.configure("observe", payload=b"bad JSON", delay_ms=500)
        report = self.check([*args, "--timeout", "0.1", "--json"], w, code=1,
                            category="timeout", expected_calls=calls, probe_cwd=Path("/"))
        assert report["diagnostics"] == ["timeout"]
        # Identity failure must beat both transport and malformed-payload failure.
        alias = self.fixture / "selected-alias"
        replacement = self.fixture / "replacement-producer"
        shutil.copyfile(self.exe, replacement)
        replacement.chmod(0o700)
        (self.fixture / "alias").write_text(str(alias))
        (self.fixture / "replacement").write_text(str(replacement))
        def reset():
            alias.unlink(missing_ok=True)
            alias.symlink_to(self.exe)
        reset()
        self.configure("observe", "retarget", payload=b"bad JSON", code=9)
        selected = args.copy()
        selected[3] = alias
        report = self.check([*selected, "--json"], w, code=1, category="fingerprint_precedence",
                            expected_calls=calls, probe_cwd=Path("/"), reset=reset)
        assert report["diagnostics"] == ["executable_changed"]

    def parser(self):
        w = self.workspace("parser")
        base = ["worker", "readiness", "--executable", self.exe]
        observe = ["observe", "djinn", "--executable", self.exe, "--folder", w]
        for prefix in (base, observe):
            for option, value in (("--timeout", "0"), ("--timeout", "nan"), ("--timeout", "31"),
                                  ("--max-output-bytes", "1023"), ("--max-output-bytes", "1048577")):
                self.check([*prefix, option, value, "--json"], w, code=1, category="invalid_limits", expected_calls=[])
        for args in (["observe", "djinn"], ["observe", "djinn", "--executable", self.exe],
                     ["repository", "register"], [*base, "--profile", "other"], [*base, "--adapter", "other"]):
            self.check(args, w, code=2, category="required_choices", expected_calls=[])
        for field, value in (("--folder", "relative"), ("--folder", str(w) + "/"),
                             ("--folder", str(w) + "/../other"), ("--executable", "djinn")):
            args = observe.copy()
            args[args.index(field) + 1] = value
            self.check([*args, "--json"], w, code=1, category="invalid_selector", expected_calls=[])
        for prefix in (["repository", "inspect"], ["repository", "register"], ["worker", "readiness"], ["observe", "djinn"]):
            # Help prose/layout is implementation-specific; assert every option is advertised.
            before = tree(w)
            self.clear_log()
            code, out, err = self.invoke([*prefix, "--help"], w)
            assert code == 0 and out and not err and not self.calls() and tree(w) == before
            flags = {("repository", "inspect"): ["--verify"],
                     ("repository", "register"): ["--path", "--base", "--delivery-target", "--expect-repository-revision"],
                     ("worker", "readiness"): ["--adapter", "--profile", "--executable", "--timeout", "--max-output-bytes"],
                     ("observe", "djinn"): ["--executable", "--folder", "--timeout", "--max-output-bytes"]}
            for flag in ("--help", "--json", *flags[tuple(prefix)]):
                assert flag.encode() in out
            self.counts["help"] += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    binary = str(Path(args.binary).resolve(strict=True)) if args.binary else None
    with tempfile.TemporaryDirectory(prefix="coven-process-integrations-") as temporary:
        suite = Suite(Path(temporary).resolve(), binary, args.oracle_only)
        suite.repositories()
        suite.races()
        suite.readiness()
        suite.djinn()
        suite.parser()
        print(json.dumps({"mode": "oracle-only" if args.oracle_only else "native-parity",
                          "cases": dict(sorted(suite.counts.items())), "total": sum(suite.counts.values()),
                          "routes": ROUTES, "owner_gates_not_certified_here": OWNER_GATES}, indent=2))


if __name__ == "__main__":
    if sys.argv[1:2] == ["--oracle-child"]:
        # Only the temporary parent oracle uses Python. Native child PATH never
        # contains it, and the controlled producer is always a native executable.
        raise SystemExit(coven.main(sys.argv[2:]))
    main()
