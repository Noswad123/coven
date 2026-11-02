"""Real CLI init corpus: one native binary, Python oracle, no runtime calls.

Every child runs in disposable local data with empty PATH and isolated HOME/XDG.
No generated command is executed. Oracle-only exercises the same corpus without
silently falling back when native init is missing.

IntentionalNativeDifferences: embedded interactive-demo/blank only (custom and
empty example refused before effects); canonical default COVEN_COMMAND=coven;
data-quoted printf for unsafe root labels; all outputs staged before exclusive
state reservation and config published LAST, versus Python config FIRST. Mixed
bootstrap is not serialized: exclusive finals prevent overwrite but BOTH may
fail. No mixed test asserts exactly one success.

Only known JSON clock fields and validated event_id fields are normalized, never
ISO/UUID-looking literals in names, prompts, templates or other fields. Replay uses the
same absolute root. The unsafe printf policy is checked by the existing narrow
template validator, not by ignoring the manifest. Inventory, modes, JSON layout,
event encoding and every other byte/field remain exact. Filesystem mtimes are
checked for refusal/preservation, not compared across separate creations.

Private service/bootstrap units cover operation-boundary failures, actual partial
writes/file-sync failures, reservation/marker fault phases, identity swaps, cleanup
failures, stage bounds and post-link uncertainty. These are targeted fault tests,
not an exhaustive filesystem-failure or power-loss matrix. This CLI corpus uses
no native failpoint and makes no claim to replace those tests.
"""

import argparse
from collections import Counter
from datetime import datetime, timezone
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
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import coven
import _state_refresh_parity as refresh_parity
from _state_refresh_parity import files, Suite as RefreshSuite

STAMP = re.compile(rb"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?\+00:00")
FIXED_STAMP = b"2000-01-01T00:00:00+00:00"
FIXED_ID = b"00000000-0000-4000-8000-000000000000"
PYTHON = str(Path(sys.executable).resolve())
ORACLE_CODE = ("import sys, subprocess; "
               f"sys.path.insert(0,{str(ROOT)!r}); from runner import coven\n"
               "def forbidden(*a,**kw):\n"
               " raise AssertionError('init corpus attempted runtime subprocess')\n"
               "subprocess.run=forbidden; subprocess.Popen=forbidden\n"
               "raise SystemExit(coven.main(sys.argv[1:]))")
ORACLE = [PYTHON, "-c", ORACLE_CODE]
RESERVED = ("coven.json", "coven.toml", "goal.md", "spec.md", "dashboard.md",
            "agents", "prompts", "state", "logs", "checkpoints", "work", "output", "tmux")


def normalized(raw, name):
    def clocks(data, expected):
        def clock(match):
            assert STAMP.fullmatch(match[2]), (name, match[2])
            value = datetime.fromisoformat(match[2].decode())
            assert value.tzinfo == timezone.utc
            assert abs((datetime.now(timezone.utc) - value).total_seconds()) < 120
            return match[1] + FIXED_STAMP + match[3]
        # Preserve every byte outside these string values, including key order,
        # whitespace and escaping. The expected count is derived from known
        # initial records, not from whatever fields the implementation emits.
        for field, expected_count in expected.items():
            pattern = rb'((?<!\\)"' + field + rb'":\s*")([^"\\]*)(")'
            data, count = re.subn(pattern, clock, data)
            assert count == expected_count, (name, field, "clock field count", count, expected_count)
        return data

    if name == "coven.json":
        return clocks(raw, {b"created_at": 1})
    if name == "state/tasks.json":
        tasks = json.loads(raw)
        return clocks(raw, {b"created_at": len(tasks), b"updated_at": len(tasks)})
    if name == "logs/events.jsonl":
        # Validate layout BEFORE normalization; reserializing would hide whitespace,
        # escaping, key order and native numeric representation regressions.
        result = []
        assert raw.endswith(b"\n")
        for line in raw[:-1].split(b"\n"):
            event = json.loads(line)
            assert line == json.dumps(event, ensure_ascii=False, separators=(",", ":")).encode()
            identity = event["event_id"]
            parsed = uuid.UUID(identity)
            assert parsed.version == 4 and parsed.variant == uuid.RFC_4122
            assert str(parsed) == identity
            expected = {b"ts": 1}
            if event["type"] == "task.seeded":
                expected.update({b"created_at": 1, b"updated_at": 1})
            line = clocks(line, expected)
            line, count = re.subn(rb'((?<!\\)"event_id":")[^"\\]*(")',
                                  lambda match: match[1] + FIXED_ID + match[2], line)
            assert count == 1
            result.append(line)
        return b"\n".join(result) + b"\n"
    return raw


class Suite:
    def __init__(self, root, binary, oracle_only):
        self.root, self.binary, self.oracle_only = root, binary, oracle_only
        self.counts, self.failures = Counter(), []
        self.failed = Counter()
        self.serial = 0
        self.children = set()
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("COVEN_", "XDG_", "KITSUNE_", "HERDR_", "TMUX"))}
        home = root / "home"
        home.mkdir()
        self.env.update(PATH="", HOME=str(home), XDG_CONFIG_HOME=str(home / "config"),
                        XDG_DATA_HOME=str(home / "data"), XDG_CACHE_HOME=str(home / "cache"),
                        XDG_STATE_HOME=str(home / "state"), XDG_RUNTIME_DIR=str(home / "runtime"),
                        PYTHONDONTWRITEBYTECODE="1", PYTHONINTMAXSTRDIGITS="4300",
                        COVEN_MULTIPLEXER="", COVEN_COMMAND="coven")
        # The shared narrow printf validator invokes only /bin/sh -n. Its syntax
        # checker must use our isolated HOME too, never the developer's home.
        refresh_parity.ENV = self.env

    def run(self, category, label, callback):
        self.counts[category] += 1
        try:
            callback()
        except Exception as exc:
            self.failed[category] += 1
            self.failures.append(f"{category}/{label}: {type(exc).__name__}: {exc}")
        finally:
            self.cleanup()

    def cleanup(self):
        for child in tuple(self.children):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=3)
            self.children.discard(child)

    def area(self):
        self.serial += 1
        path = self.root / f"case-{self.serial}"
        path.mkdir()
        return path

    def spawn(self, args, cwd, env=None, oracle=False, code=None, mask=0o022):
        command = ([PYTHON, "-c", code] if code else ORACLE) if oracle or self.oracle_only else [self.binary]
        child = subprocess.Popen([*command, *map(str, args)], cwd=cwd, env=env or self.env,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=True, umask=mask)
        self.children.add(child)
        return child

    def finish(self, child):
        try:
            out, err = child.communicate(timeout=10)
            return child.returncode, out.decode(), err.decode()
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
                child.communicate(timeout=3)
            self.children.discard(child)

    def call(self, args, cwd, **kwargs):
        return self.finish(self.spawn(args, cwd, **kwargs))

    def validate(self, w, blank, mask):
        tree = files(w)
        team = json.loads((w / "coven.json").read_bytes())
        agents = team["agents"]
        assert len([v for v in tree.values() if v[0] == "file"]) == (14 if blank else 15) + 3 * len(agents)
        for name in ("coven.json", "state/agents.json", "state/tasks.json"):
            raw = (w / name).read_bytes()
            assert raw == (json.dumps(json.loads(raw), indent=2, ensure_ascii=True) + "\n").encode(), name
        for name, entry in tree.items():
            if entry[0] == "file":
                expected = (0o600 if name in ("dashboard.md", "state/.workspace.lock") else 0o666) & ~mask
                assert entry[1] == expected, (name, oct(entry[1]), oct(expected))
                assert entry[5] == 1, (name, "unexpected hardlink")
            elif entry[0] == "dir":
                parent = w / Path(name).parent
                assert entry[1] == (0o777 & ~mask) | (parent.stat().st_mode & stat.S_ISGID), (name, entry)
            else:
                raise AssertionError((name, entry))
        events = [json.loads(line) for line in (w / "logs/events.jsonl").read_bytes().splitlines()]
        assert events[0]["type"] == "workspace.created"
        assert events[0]["agents"] == agents and events[0]["name"] == team["name"]
        assert events[0]["example"] == ("blank" if blank else "interactive-demo")
        tasks = json.loads((w / "state/tasks.json").read_bytes())
        assert not (w / "state/task-authority.json").exists()
        assert not (w / "state/task-ledger").exists()
        if blank:
            assert tasks == [] and len(events) == 1 and not (w / "spec.md").exists()
        else:
            assert len(tasks) == 6 and len(events) == 8
            assert events[1]["type"] == "goal.seeded"
            for event, task in zip(events[2:], tasks):
                assert event["type"] == "task.seeded" and event["task"] == task
                assert task["owner"] is None or task["owner"] in agents
        identities = [event["event_id"] for event in events]
        assert len(set(identities)) == len(identities)
        for name, entry in tree.items():
            if entry[0] == "file":
                assert abs(time.time_ns() - entry[4]) < 120 * 1_000_000_000
                normalized(entry[2], name)
        return tree

    def success(self, extra=(), *, suffix="workspace", location="new", env=None, mask=0o022,
                sgid=False, sentinel=False, lifecycle=False, default_command=False):
        area = self.area()
        w = area / suffix
        cwd = area
        operand = w
        if location in ("empty", "current", "alias", "dotdot"):
            w.mkdir()
        if location == "current":
            cwd, operand = w, None
        elif location == "home":
            operand = "~/" + area.name
            w = Path(self.env["HOME"]) / area.name
        elif location == "alias":
            (area / "alias").symlink_to(w, target_is_directory=True)
            operand = area / "alias"
        elif location == "dotdot":
            (area / "hop").mkdir()
            operand = area / "hop/../workspace"
        if sgid:
            w.mkdir(exist_ok=True)
            w.chmod(0o2775)
        if sentinel:
            w.mkdir(exist_ok=True)
            (w / "unrelated.bin").write_bytes(b"user bytes\x00\xff\n")
        before = files(w) if w.exists() else {}
        args = ["init", *([] if operand is None else [operand]), *extra]
        effective = dict(self.env if env is None else env)
        if default_command:
            effective.pop("COVEN_COMMAND", None)
        result = self.call(args, cwd, env=effective, mask=mask)
        assert result[0] == 0 and result[2] == "", (args, result)
        blank = "--blank" in extra or "--bl" in extra
        # Unrelated content is excluded only from the generated-file count, and
        # independently checked with its inode, mtime, mode and arbitrary bytes.
        unrelated = None
        if sentinel:
            unrelated = files(w)["unrelated.bin"]
            assert unrelated == before["unrelated.bin"]
            (w / "unrelated.bin").rename(area / "sentinel")
        actual = self.validate(w, blank, mask)
        if sentinel:
            (area / "sentinel").rename(w / "unrelated.bin")
            actual = files(w)
        native = area / "saved-native"
        w.rename(native)
        # Recreate the same pre-init destination at the exact path. The oracle
        # therefore sees identical resolved roots, including alias and HOME cases.
        if before or location in ("empty", "current", "alias", "dotdot") or sgid:
            w.mkdir()
            if sgid:
                w.chmod(0o2775)
            if sentinel:
                (w / "unrelated.bin").write_bytes(unrelated[2])
        try:
            oracle_env = dict(effective)
            if default_command or (not self.oracle_only and not effective.get("COVEN_COMMAND")):
                oracle_env["COVEN_COMMAND"] = "coven"
            expected = self.call(args, cwd, env=oracle_env, oracle=True, mask=mask)
            if not self.oracle_only:
                # Native terminal output data-escapes roots. Keep every INFO
                # prefix, message, whitespace and line count otherwise exact.
                escaped_root = json.dumps(str(w), ensure_ascii=True)[1:-1]
                expected = (expected[0], expected[1].replace(str(w), escaped_root), expected[2])
            assert result == expected, (result, expected)
            golden = files(w)
            assert actual.keys() == golden.keys(), (sorted(actual), sorted(golden))
            for name, value in actual.items():
                assert value[:2] == golden[name][:2], (name, value[:2], golden[name][:2])
                if value[0] != "file":
                    continue
                a, b = normalized(value[2], name), normalized(golden[name][2], name)
                if name == "tmux/coven.toml" and not self.oracle_only and any(c in str(w) for c in ("'", "%", "\\")):
                    b = normalized(RefreshSuite.safe_manifest(w, value[2], golden[name][2]), name)
                assert a == b, f"bytes differ: {name}: actual={a[:800]!r} expected={b[:800]!r}"
        finally:
            if w.exists():
                shutil.rmtree(w)
            native.rename(w)
        if lifecycle:
            self.lifecycle(w)
        # Even --force never adopts a workspace or changes its lock metadata.
        before = files(w)
        refused = self.call(["init", w, "--force"], area)
        assert refused[0] == 1 and refused[1] == "" and refused[2], refused
        assert files(w) == before

    def reject(self, extra, *, parser=False, setup=None, env=None, semantic=None, oracle=True):
        area = self.area()
        w = area / "missing/child"
        if setup:
            w = area / "workspace"
            w.mkdir()
            setup(w)
        before = files(area)
        args = ["init", w, *extra]
        actual = self.call(args, area, env=env)
        assert actual[0] == (2 if parser else 1) and actual[1] == "" and actual[2], actual
        assert files(area) == before, "refusal changed inventory/inodes/bytes/modes/mtimes"
        if semantic:
            assert re.search(semantic, actual[2], re.I), actual
        if oracle:
            expected = self.call(args, area, env=env, oracle=True)
            assert expected[0] == actual[0] and expected[1] == "" and expected[2], expected
            if semantic:
                assert re.search(semantic, expected[2], re.I), expected
            assert files(area) == before, "oracle refusal had effects"

    def lifecycle(self, w):
        for oracle in (True, False):
            before = files(w)
            result = self.call(["state", "inspect", w, "--json"], w, oracle=oracle)
            assert result[0] == 0 and not result[2], result
            report = json.loads(result[1])
            assert report["authority"] == "snapshot-v1", report
            assert files(w) == before
            result = self.call(["refresh", w], w, oracle=oracle)
            assert result[0] == 0 and not result[2], result
            result = self.call(["task", "add", w, "Local interoperability", "--json"], w, oracle=oracle)
            assert result[0] == 0 and not result[2], result
        pin = coven.read_task_snapshot(w)[0]
        result = self.call(["state", "import-tasks", w, "--expect-revision", pin, "--json"], w)
        assert result[0] == 0 and not result[2], result
        reports = []
        for oracle in (True, False):
            result = self.call(["state", "inspect", w, "--json"], w, oracle=oracle)
            assert result[0] == 0 and not result[2], result
            reports.append(json.loads(result[1]))
        assert reports[0] == reports[1]

    def matrices(self):
        profiles = [([], "default"), (["--blank"], "blank"),
                    (["--example", "interactive-demo"], "explicit-example"),
                    (["--example", "../interactive-demo"], "normalized-example-alias-not-path-lookup"),
                    (["--blank", "--example", "missing-example"], "blank-wins"),
                    (["--blank", "--example", ""], "blank-empty-example"),
                    (["--force"], "force-noop"),
                    (["--name", ""], "empty-name-fallback"),
                    (["--name", " Team \u03bb \"line\"\nsecond "], "unicode-name"),
                    (["--name", "Literal 2001-02-03T04:05:06+00:00 00000000-0000-4000-8000-000000000000"], "literal-clock-and-uuid-name"),
                    (["--agents", "operator,Other Person,A_b-9"], "custom-roster"),
                    (["--agents", "Clippy,Srikanth"], "partial-owners"),
                    (["--agents", " \u03bb Alice , B\u00e9ta ", "--blank"], "unicode-labels"),
                    (["--agents", "solo"], "omitted-owners"),
                    (["--opencode-command", " oc --model 'x y' "], "command-whitespace"),
                    (["--opencode-command", "false; touch NEVER_EXECUTED"], "command-is-data"),
                    (["--multiplexer", "tmux"], "tmux"),
                    (["--multiplexer", "herdr"], "herdr"),
                    (["--m", "tmux"], "short-multiplexer-prefix"),
                    (["--name", "old", "--name", "new", "--agents", "bad,bad", "--agents", "Solo"], "last-value"),
                    (["--example", "missing", "--example", "interactive-demo",
                      "--opencode-command", " ", "--opencode-command", "oc",
                      "--multiplexer", "herdr", "--multiplexer", "tmux"], "repeated-value-options"),
                    (["--blank", "--blank", "--force", "--force"], "repeat-switch"),
                    (["--na", "prefix", "--ag", "Solo", "--op", "oc", "--mul", "tmux", "--bl", "--fo"], "abbreviations"),
                    (["--ex", "interactive-demo"], "example-prefix"),
                    (["--name=-1", "--opencode-command=-9"], "negative-equals")]
        for extra, label in profiles:
            self.run("seed-bytes", label, lambda extra=extra: self.success(extra, lifecycle=True))
        for location in ("new", "empty", "current", "home", "alias", "dotdot"):
            self.run("resolution", location, lambda location=location: self.success(location=location))
        for suffix in ("parent/new/workspace", "space \u03bb\nworkspace", "apostrophe'root", "percent%s%nroot", r"backslash\n\croot"):
            self.run("root-labels", suffix, lambda suffix=suffix: self.success(suffix=suffix))
        self.run("preservation", "unrelated", lambda: self.success(["--blank"], sentinel=True))
        for mask in (0o002, 0o027, 0o077):
            self.run("modes", oct(mask), lambda mask=mask: self.success(mask=mask))
        self.run("modes", "sgid", lambda: self.success(mask=0o027, sgid=True))
        self.run("offline", "PATH empty full seed canonical command", lambda: self.success(default_command=not self.oracle_only))
        for key, values in (("COVEN_MULTIPLEXER", ("tmux", "herdr", " TMUX ", "")),
                            ("COVEN_COMMAND", ("/fixed path/'coven'", "", " false; touch NEVER_EXECUTED "))):
            for value in values:
                env = dict(self.env, **{key: value})
                self.run("environment", f"{key}={value!r}", lambda env=env: self.success(env=env))
        for name in RESERVED:
            for kind in ("file", "dir", "sym", "dangling"):
                for force in (False, True):
                    def case(name=name, kind=kind, force=force):
                        def setup(w):
                            target = w / name
                            if kind == "file":
                                target.write_bytes(b"authored\x00\xff\n")
                                target.chmod(0o640)
                            elif kind == "dir":
                                target.mkdir()
                                (target / "keep").write_bytes(b"keep")
                            else:
                                target.symlink_to("keep" if kind == "sym" else "absent")
                                if kind == "sym":
                                    (w / "keep").write_bytes(b"external target")
                        # Collision must win over invalid roster, example and env.
                        self.reject([*(["--force"] if force else []), "--agents", "a,A", "--example", "missing"],
                                    setup=setup, env=dict(self.env, COVEN_MULTIPLEXER="invalid"),
                                    semantic=r"existing|refus|reserved")
                    self.run("reserved", f"{name}/{kind}/force={force}", case)
        semantics = [(["--agents", ""], r"letter|number|agent|slug"),
                     (["--agents", "a,A"], r"duplicate"),
                     (["--agents", "a,"], r"letter|number|agent|slug"),
                     (["--agents", "\u03bb"], r"letter|number|agent|slug"),
                     (["--opencode-command", " "], r"empty|command"),
                     (["--example", "missing-example"], r"example"),
                     (["--agents", "a,A", "--example", "missing"], r"duplicate")]
        for extra, pattern in semantics:
            self.run("semantic-no-effects", repr(extra), lambda extra=extra, pattern=pattern: self.reject(extra, semantic=pattern))
        for value in ("invalid", "TMUX-invalid"):
            env = dict(self.env, COVEN_MULTIPLEXER=value)
            self.run("semantic-no-effects", "invalid env " + value, lambda env=env: self.reject([], env=env, semantic=r"multiplexer"))
        parser_cases = [["--json"], ["--unknown"], ["--name"], ["--agents"], ["--opencode-command"],
                        ["--multiplexer"], ["--example"], ["--multiplexer", "TMUX"],
                        ["--multiplexer", "invalid", "--multiplexer", "tmux"],
                        ["--name", "-x"], ["--agents", "-x"], ["--opencode-command", "-x"],
                        ["--example", "-x"], ["--blank=yes"], ["--force=no"],
                        ["--", "extra"], ["extra"]]
        for extra in parser_cases:
            self.run("parser-no-effects", repr(extra), lambda extra=extra: self.reject(extra, parser=True))
        for extra in (["--name", "-1"], ["--opencode-command", "-9"]):
            self.run("parser-negative", repr(extra), lambda extra=extra: self.success(extra))
        def terminator():
            area = self.area()
            for name, separator in (("-workspace", ["--"]), ("--blank", ["--"]), ("-1", [])):
                for oracle in (False, True):
                    w = area / name
                    result = self.call(["init", "--blank", *separator, name], area, oracle=oracle)
                    assert result[0] == 0 and not result[2], result
                    self.validate(w, True, 0o022)
                    shutil.rmtree(w)
        self.run("parser-terminator", "dash workspace", terminator)
        for kind in ("file", "dangling-alias", "alias-new-child", "symlink-dotdot"):
            def root_case(kind=kind):
                area = self.area()
                for oracle in (False, True):
                    slot = area / ("oracle" if oracle else "candidate")
                    slot.mkdir()
                    operand = slot / "workspace"
                    if kind == "file":
                        operand.write_bytes(b"not a directory\x00\xff")
                    elif kind == "dangling-alias":
                        operand.symlink_to("target")
                    else:
                        target = slot / "real/inside"
                        target.mkdir(parents=True)
                        (slot / "alias").symlink_to(target, target_is_directory=True)
                        operand = slot / ("alias/new/child" if kind == "alias-new-child" else "alias/../workspace")
                    resolved = operand.resolve()
                    before = files(slot)
                    result = self.call(["init", operand, "--blank"], slot, oracle=oracle)
                    if kind == "file":
                        assert result[0] == 1 and not result[1] and result[2], result
                        assert files(slot) == before
                    else:
                        assert result[0] == 0 and not result[2], result
                        self.validate(resolved, True, 0o022)
                        assert f"Coven workspace: {resolved}\n" in result[1], result
                        for name, value in before.items():
                            if value[0] == "link":
                                assert files(slot)[name] == value
            self.run("resolution-edges", kind, root_case)
        # All reserved names reported in the fixed preflight inventory order,
        # regardless of the order directory entries were created.
        def ordered_conflicts():
            def setup(w):
                for name in reversed(RESERVED):
                    (w / name).write_bytes(b"keep")
            area = self.area()
            w = area / "workspace"
            w.mkdir()
            setup(w)
            before = files(w)
            for oracle in (False, True):
                result = self.call(["init", w, "--force"], area, oracle=oracle)
                assert result[0] == 1 and not result[1], result
                if oracle or self.oracle_only:
                    assert ", ".join(RESERVED) + ". Use `coven refresh`" in result[2], result
                else:
                    assert re.search(r"reserved|existing|refus", result[2], re.I), result
                assert files(w) == before
        self.run("reserved-order", "all names", ordered_conflicts)
        self.run("semantic-no-effects", "roster before invalid environment", lambda: self.reject(
            ["--agents", "a,A"], env=dict(self.env, COVEN_MULTIPLEXER="invalid"), semantic=r"duplicate"))
        self.run("semantic-no-effects", "example before invalid environment", lambda: self.reject(
            ["--example", "missing-example"], env=dict(self.env, COVEN_MULTIPLEXER="invalid"), semantic=r"example"))
        self.run("semantic-no-effects", "environment before empty command", lambda: self.reject(
            ["--opencode-command", " "], env=dict(self.env, COVEN_MULTIPLEXER="invalid"), semantic=r"multiplexer"))
        self.run("example-policy", "local custom example", self.custom_example)
        self.normalization_checks()
        if not self.oracle_only:
            for example in ("", "custom", "../another-example"):
                self.run("IntentionalNativeDifferences", "unsupported example " + repr(example),
                         lambda example=example: self.reject(["--example", example], oracle=False, semantic=r"example"))
            self.run("bootstrap-race", "native/native gated start", self.native_race)
            self.run("bootstrap-race", "Python config-first paused collision", self.python_first)
            self.run("bootstrap-race", "stale Python preflight exclusive final collision", self.stale_python)

    def normalization_checks(self):
        stamp = datetime.now(timezone.utc).isoformat()
        identity = str(uuid.uuid4())
        def literal_fields():
            raw = json.dumps({"created_at": stamp, "name": stamp, "literal_id": identity}, indent=2).encode()
            expected = raw.replace(('"created_at": "' + stamp + '"').encode(),
                                   b'"created_at": "' + FIXED_STAMP + b'"', 1)
            assert normalized(raw, "coven.json") == expected
            for name in ("goal.md", "dashboard.md", "coven.toml", "prompts/solo.md", "unrelated.bin"):
                assert normalized(raw, name) == raw, name
        self.run("normalization", "known fields only; literal clocks and IDs preserved", literal_fields)
        def event_fields():
            raw = json.dumps({"event_id": identity, "ts": stamp, "type": "workspace.created",
                              "name": stamp + " " + identity}, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
            actual = json.loads(normalized(raw, "logs/events.jsonl"))
            assert actual == {"event_id": FIXED_ID.decode(), "ts": FIXED_STAMP.decode(),
                              "type": "workspace.created", "name": stamp + " " + identity}
        self.run("normalization", "only event ID field, not repeated literal UUID", event_fields)
        def malformed():
            for value in (stamp + "tail", "2001-02-03T04:05:06+00:00", stamp.replace("+00:00", "Z"), "bad", 123, None):
                raw = json.dumps({"created_at": value}).encode()
                try:
                    normalized(raw, "coven.json")
                except AssertionError:
                    continue
                raise AssertionError(f"invalid clock was normalized: {value!r}")
        self.run("normalization", "malformed and stale clocks refused", malformed)

    def custom_example(self):
        area = self.area()
        examples = area / "examples"
        examples.mkdir()
        custom = examples / "custom"
        shutil.copytree(ROOT / "examples/interactive-demo", custom)
        manifest = json.loads((custom / "example.json").read_bytes())
        manifest["id"] = "custom"
        (custom / "example.json").write_text(json.dumps(manifest))
        (custom / "goal.md").write_bytes(b"# Custom local goal\n")
        w = area / "workspace"
        before = files(area)
        if not self.oracle_only:
            result = self.call(["init", w, "--example", "custom"], area)
            assert result[0] == 1 and not result[1] and "example" in result[2], result
            assert files(area) == before
        # Only this oracle child sees a private example directory. Neither the
        # checkout nor Python production code is modified to create the fixture.
        code = ORACLE_CODE.replace("raise SystemExit(coven.main(sys.argv[1:]))",
                                   f"from pathlib import Path; coven.EXAMPLES_DIR=Path({str(examples)!r})\n"
                                   "raise SystemExit(coven.main(sys.argv[1:]))")
        result = self.call(["init", w, "--example", "custom"], area, oracle=True, code=code)
        assert result[0] == 0 and not result[2], result
        assert json.loads((w / "coven.json").read_bytes())["example"] == "custom"
        assert (w / "goal.md").read_bytes() == b"# Custom local goal\n"

    def stale_python(self):
        area = self.area()
        w = area / "workspace"
        code = ("import sys; "
                f"sys.path.insert(0,{str(ROOT)!r}); from runner import coven\n"
                "original=coven.preflight_init\n"
                "def paused(path):\n"
                " original(path); print('PAUSED',flush=True); sys.stdin.readline()\n"
                "coven.preflight_init=paused\n"
                "raise SystemExit(coven.main(sys.argv[1:]))")
        child = self.spawn(["init", w], area, oracle=True, code=code)
        ready, _, _ = select.select([child.stdout], [], [], 5)
        assert ready and child.stdout.readline() == b"PAUSED\n", "missing oracle preflight barrier"
        assert not w.exists()
        native = self.call(["init", w], area)
        assert native[0] == 0 and not native[2], native
        before = files(w)
        child.stdin.write(b"continue\n")
        child.stdin.flush()
        oracle = self.finish(child)
        assert oracle[0] == 1 and not oracle[1] and "partial" in oracle[2], oracle
        assert files(w) == before, "stale unlocked Python writer overwrote native finals"

    def native_race(self):
        area = self.area()
        w = area / "workspace"
        # An external exec gate removes Python/process-startup skew. It does not
        # claim to pause native reservation ownership: that exact interleaving
        # remains a private bootstrap unit test, not a CLI scheduling assertion.
        code = ("import os,sys; print('READY',flush=True); sys.stdin.readline(); "
                "os.execve(sys.argv[1],sys.argv[1:],dict(os.environ))")
        children = [self.spawn([self.binary, "init", w], area, oracle=True, code=code) for _ in range(2)]
        for child in children:
            ready, _, _ = select.select([child.stdout], [], [], 5)
            assert ready and child.stdout.readline() == b"READY\n", "missing native exec start barrier"
        assert not w.exists(), "gated child initialized before release"
        for child in children:
            child.stdin.write(b"continue\n")
            child.stdin.flush()
        results = [self.finish(child) for child in children]
        assert sorted(result[0] for result in results) == [0, 1], results
        for status, out, err in results:
            assert (out.startswith("[INFO] Coven workspace:") and not err) if status == 0 else (not out and err), results
        self.validate(w, False, 0o022)
        before = files(w)
        result = self.call(["init", w, "--force"], area)
        assert result[0] == 1 and files(w) == before, result

    def python_first(self):
        area = self.area()
        w = area / "workspace"
        # Test-subprocess-only oracle hook: pause after config close, before the
        # first state file. No production change or native failpoint is involved.
        code = ("import sys; from pathlib import Path; "
                f"sys.path.insert(0,{str(ROOT)!r}); from runner import coven; "
                "original=Path.open\n"
                "def opened(path,*a,**kw):\n"
                " if path.name=='agents.json' and a and a[0]=='x':\n"
                "  print('PAUSED',flush=True); sys.stdin.readline()\n"
                " return original(path,*a,**kw)\n"
                "Path.open=opened\n"
                "raise SystemExit(coven.main(sys.argv[1:]))")
        child = self.spawn(["init", w], area, oracle=True, code=code)
        ready, _, _ = select.select([child.stdout], [], [], 5)
        assert ready and child.stdout.readline() == b"PAUSED\n", "Python oracle did not reach publication barrier"
        assert (w / "coven.json").exists() and not (w / "state/agents.json").exists()
        assert not (w / "state/.workspace.lock").exists(), "oracle initial writes unexpectedly locked"
        before = files(area)
        native = self.call(["init", w], area)
        assert native[0] == 1 and not native[1] and native[2] and files(area) == before, native
        child.stdin.write(b"continue\n")
        child.stdin.flush()
        result = self.finish(child)
        assert result[0] == 0 and not result[2], result
        self.validate(w, False, 0o022)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    binary = str(Path(args.binary).resolve()) if args.binary else None
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="coven-init-") as directory:
        suite = Suite(Path(directory), binary, args.oracle_only)
        try:
            suite.matrices()
        finally:
            suite.cleanup()
        print(json.dumps({"cases": sum(suite.counts.values()), "categories": suite.counts,
                          "failures": len(suite.failures), "failure_categories": suite.failed,
                          "oracle_only": args.oracle_only, "seconds": round(time.monotonic() - started, 3),
                          "gaps": ["private service/bootstrap fault matrix owned by module units",
                                   "native-held reservation barrier requires private bootstrap unit",
                                   "mixed simultaneous unlocked Python writes may leave both failing"]}))
        for failure in suite.failures:
            print(failure, file=sys.stderr)
        return bool(suite.failures)


if __name__ == "__main__":
    raise SystemExit(main())
