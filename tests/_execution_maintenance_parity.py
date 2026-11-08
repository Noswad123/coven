"""Real maintenance CLI acceptance; setup uses Python Store, never providers.

Run --oracle-only first. Native execution requires the parent's complete routes.
Only operation entropy is supplied to the oracle, never normalized in receipts,
hashes, extensions or operator text. Native fault injection belongs in private
owner units, not environment variables or this external-process acceptance gate.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, chdir, redirect_stderr, redirect_stdout
from copy import deepcopy
from datetime import datetime
import fcntl
import io
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time
import traceback
import uuid
from unittest.mock import patch

sys.dont_write_bytecode = True
from _execution_inspection_parity import Suite as Fixtures, ENV, exact, intent, document
from _execution_parity import snapshot, resign
from runner import coven, execution_v2 as v2, storage, task_attempts as v1


def forbidden(*args, **kwargs):
    raise AssertionError(("maintenance touched runtime/source", args, kwargs))


def python_cli(args, cwd, entropy=None):
    out, err = io.StringIO(), io.StringIO()
    with ExitStack() as stack:
        stack.enter_context(chdir(cwd))
        stack.enter_context(redirect_stdout(out))
        stack.enter_context(redirect_stderr(err))
        stack.enter_context(patch.dict(os.environ, ENV, clear=True))
        for target, name in ((coven, "execution_source"), (coven, "load_team"),
                             (coven, "append_event"), (subprocess, "Popen")):
            stack.enter_context(patch.object(target, name, forbidden))
        if entropy:
            # Patch ONLY entropy providers during the operation, not setup/replay.
            uid, stamp = entropy
            stack.enter_context(patch.object(v1.uuid, "uuid4", return_value=type("UUID", (), {"hex": uid})()))
            if stamp is not None:
                class Clock(datetime):
                    @classmethod
                    def now(cls, tz=None):
                        return datetime.fromisoformat(stamp)
                for module in (v1, v2):
                    stack.enter_context(patch.object(module, "datetime", Clock))
        try:
            code = coven.main(list(map(str, args)))
        except SystemExit as exc:
            code = exc.code
    return code, out.getvalue().encode("ascii"), err.getvalue().encode("utf-8")


def presentation(raw, args):
    raw.decode("ascii")
    value = v1._json(raw)
    compact = any(arg in ("--json", "--js", "--j") for arg in args)
    assert raw.endswith(b"\n") and not raw.endswith(b"\n\n")
    if compact:
        assert b"\n" not in raw[:-1]
    else:
        assert raw == (json.dumps(value, ensure_ascii=True, indent=2) + "\n").encode("ascii")
    return value


def cancel_args(workspace, attempt, revision, key="cancel", reason="  exact \u03bb \U0001f98a  "):
    return ["task", "attempt", "cancel", workspace, attempt, "--reason", reason,
            "--expect-execution-revision", revision, "--request-id", key]


def confirm_args(workspace, revision):
    return ["execution", "recover", workspace, "--confirm-authority", "--expect-execution-revision", revision]


class Suite(Fixtures):
    def __init__(self, *args):
        super().__init__(*args)
        self.counts = Counter()

    def fresh(self, name, template):
        return self.fixture(name, template)

    def check(self, name, args, cwd, code=0, category="cancel", effect="none",
              contains=None, profile=None, verify=None, creates_lock=False, native_contains=None):
        assert name not in self.names, name
        self.names.add(name)
        control = cwd / v1.CONTROL
        initial = control.read_bytes() if control.is_file() and not control.is_symlink() else None
        initial_lock = (cwd / "state/.workspace.lock").exists()

        def restore(value):
            if effect == "cancel" and initial is not None:
                control.write_bytes(initial)
            if effect == "archive" and value is not None:
                Path(value["archive_path"]).rename(cwd / v1.DIR)
            if not initial_lock:
                (cwd / "state/.workspace.lock").unlink(missing_ok=True)

        def effects(before, value, success):
            after = snapshot(cwd)
            if not success or effect == "none":
                if creates_lock:
                    assert "state/.workspace.lock" in after
                    assert after["state/.workspace.lock"][3:] == ("file", b"")
                    for key in ("", "state", "state/.workspace.lock"):
                        before.pop(key, None)
                        after.pop(key, None)
                assert after == before, (name, "unexpected tree change")
                return
            lock = "state/.workspace.lock"
            assert before[lock] == after[lock], (name, "lock inode changed")
            if effect == "cancel":
                key = v1.CONTROL
                assert after[key][0] == before[key][0], (name, "control mode changed")
                assert after[key][1] != before[key][1], (name, "control was not atomically replaced")
                raw = control.read_bytes()
                assert raw == v1.canonical(v1._json(raw)) + b"\n"
                view = v1.Store(cwd).read()  # Complete replay of exact published bytes.
                assert view.generation == v1._json(initial)["generation"] + 1
                assert value["result"] == view.receipts[v1._json(raw)["history"][-1]["request_id"]]["result"]
                for key in (v1.CONTROL, v1.DIR):
                    before.pop(key)
                    after.pop(key)
            else:
                destination = Path(value["archive_path"])
                assert destination.parent == cwd / "state"
                assert re.fullmatch(r"execution-pending-archive-[0-9a-f]{32}", destination.name)
                assert not (cwd / v1.DIR).exists()
                prefix = str(destination.relative_to(cwd))
                moved = {v1.DIR + key[len(prefix):]: item for key, item in after.items()
                         if key == prefix or key.startswith(prefix + "/")}
                old = {key: item for key, item in before.items() if key == v1.DIR or key.startswith(v1.DIR + "/")}
                assert moved == old, (name, "archive changed evidence")
                before = {k: v for k, v in before.items() if k not in old and k != "state"}
                after = {k: v for k, v in after.items() if not (k == prefix or k.startswith(prefix + "/")) and k != "state"}
            assert after == before, (name, "secondary write")

        try:
            before = snapshot(cwd)
            expected_code, out, err = python_cli(args, cwd)
            assert expected_code == code, (name, expected_code, out[:300], err)
            expected = presentation(out, args) if out.startswith(b"{") else None
            if code:
                assert not out, (name, out)
                if contains:
                    assert contains.encode() in err, (name, err)
            else:
                assert not err
                assert expected is not None or ("--help" in args and b"usage:" in out.lower())
                if verify:
                    verify(expected)
            effects(before, expected, code == 0)
            oracle_raw = control.read_bytes() if effect == "cancel" and code == 0 else None
            if self.oracle_only:
                self.counts["native_profile_refusals" if profile else category] += 1
                return expected
            restore(expected)
            before = snapshot(cwd)
            result = subprocess.run([self.binary, *map(str, args)], cwd=cwd, env=ENV,
                                    capture_output=True, timeout=12)
            assert result.returncode == (1 if profile else code), (name, result.returncode, result.stdout[:400], result.stderr)
            if profile or code:
                assert result.stdout == b"" and result.stderr, (name, result)
                assert len(result.stderr) <= 65536, (name, "unbounded error delivery")
                if profile:
                    assert profile.encode() in result.stderr, (name, result.stderr)
                if native_contains:
                    assert native_contains.encode() in result.stderr, (name, result.stderr)
                effects(before, None, False)
            else:
                assert not result.stderr, (name, result.stderr)
                if "--help" in args:
                    assert b"usage:" in result.stdout.lower() and result.stdout.decode("ascii")
                    effects(before, None, True)
                    self.counts[category] += 1
                    return None
                actual = presentation(result.stdout, args)
                effects(before, actual, True)
                native_raw = control.read_bytes() if effect == "cancel" else None
                entropy = None
                if effect == "cancel":
                    record = v1._json(native_raw)["history"][-1]
                    entropy = record["effect"]["cancel_id"], record["effect"]["cancelled_at"]
                elif effect == "archive":
                    entropy = Path(actual["archive_path"]).name.removeprefix("execution-pending-archive-"), None
                if entropy:
                    restore(actual)
                    golden_code, golden_out, golden_err = python_cli(args, cwd, entropy)
                    assert golden_code == 0 and not golden_err
                    expected = presentation(golden_out, args)
                    if effect == "cancel":
                        assert control.read_bytes() == native_raw, (name, "exact persisted control mismatch")
                elif oracle_raw is not None:
                    assert native_raw == oracle_raw
                exact(actual, expected)
                if verify:
                    verify(actual)
            self.counts["native_profile_refusals" if profile else category] += 1
            return expected
        except Exception as exc:
            self.failures.append({"case": name, "category": category, "error": str(exc)[:4000]})


def prepared(s, name, schema=1, graph=False):
    root = s.fixture(name)
    (root / "logs").mkdir()
    (root / "logs/events.jsonl").write_bytes(b"")
    (root / "logs/messages.jsonl").write_bytes(b"")
    (root / "dashboard.md").write_bytes(b"Operator-owned dashboard sentinel\n")
    (root / "checkpoints").mkdir()
    (root / "checkpoints/plan-approval.md").write_bytes(b"Operator-owned checkpoint\n")
    s.mutate(root, "enable", {})
    control = v1._json((root / v1.CONTROL).read_bytes())
    control["extensions"] = {"legacy": {"float": 1.0, "zero": -0.0, "bool": True,
        "operator": "00000000000000000000000000000000 \u03bb", "large": 2**90 + 1}}
    (root / v1.CONTROL).write_bytes(v1.canonical(control))
    for key in ("zeta", "alpha", "unrelated"):
        s.mutate(root, "enroll", {"task_id": key, "rebind_source": False})
    if schema == 2:
        s.upgrade(root)
    if graph:
        proposal = s.mutate(root, "graph_propose", {"document": document()})
        s.decide(root)
        attempt = s.mutate(root, "prepare_graph", {"task_id": "zeta", "graph_revision": proposal["graph_revision"]})
    else:
        attempt = s.mutate(root, "prepare", {"task_id": "zeta", "intent": intent("zeta")})
    # Preserve noncanonical initial bytes and legacy extension types through write.
    control = v1._json((root / v1.CONTROL).read_bytes())
    (root / v1.CONTROL).write_bytes(json.dumps(control, ensure_ascii=True, indent=3).encode() + b"\n\n")
    (root / v1.CONTROL).chmod(0o640)
    return root, attempt["attempt_id"]


def run(s):
    templates = [(*prepared(s, "v1"), "v1"), (*prepared(s, "v2", 2), "v2"),
                 (*prepared(s, "graph", 2, True), "graph")]
    for template, attempt, label in templates:
        for compact in (False, True):
            root = s.fresh(f"{label}-cancel-{compact}", template)
            revision = v1.Store(root).read().revision
            args = cancel_args(root, attempt, revision) + (["--json"] if compact else [])
            first = s.check(f"{label}/cancel/{compact}", args, root, effect="cancel")
            replay = s.check(f"{label}/retry/{compact}", args, root, category="receipt")
            if first is not None and replay is not None:
                exact(first["result"], replay["result"])
                assert replay["delivery"]["replayed"] is True
            current = v1.Store(root).read().revision
            s.check(f"{label}/second/{compact}", cancel_args(root, attempt, current, "new-key"), root, code=1, category="receipt")
            for field, replacement in (("reason", "changed"), ("expect-execution-revision", current),
                                       ("request-id", "new-key")):
                changed = list(args)
                changed[changed.index("--" + field) + 1] = replacement
                s.check(f"{label}/conflict-{field}/{compact}", changed, root, code=1, category="receipt")
            changed = list(args)
            changed[4] = "f" * 32
            s.check(f"{label}/conflict-attempt/{compact}", changed, root, code=1, category="receipt")
            if label == "v1":
                s.upgrade(root)
                upgraded = s.check(f"legacy-retry/{compact}", args, root, category="legacy_receipt")
                if upgraded is not None:
                    exact(upgraded["result"], first["result"])
            s.mutate(root, "prepare", {"task_id": "unrelated", "intent": intent("unrelated")})
            control = v1._json((root / v1.CONTROL).read_bytes())
            control["history"][-1]["result"]["corrupt_tail"] = True
            (root / v1.CONTROL).write_bytes(v1.canonical(control))
            s.check(f"{label}/corrupt-tail/{compact}", args, root, code=1, category="complete_replay")

        root = s.fresh(label + "-confirm", template)
        s.check(label + "/confirm-prepared", confirm_args(root, v1.Store(root).read().revision), root, category="confirmation")
        s.check(label + "/confirm-prepared-compact", confirm_args(root, v1.Store(root).read().revision) + ["--json"], root, category="confirmation")
        stale = v1.Store(root).read().revision
        s.mutate(root, "cancel", {"attempt_id": attempt, "reason": "setup"})
        s.check(label + "/confirm-stale", confirm_args(root, stale), root, code=1, category="confirmation")

    template, attempt, _ = templates[0]
    source_cases(s, templates)
    parser_cases(s, template, attempt)
    archive_cases(s, template)
    oracle_faults(s, template, attempt)
    profile_cases(s, template, attempt)
    cross_workflow(s, template, attempt)
    for baseline, aid, label in templates[:2]:
        races(s, baseline, aid, label)
    process_death(s, template)


def source_cases(s, templates):
    poisons = [("marker-free", "coven.json", None), ("marker", "coven.json", b"{\xff"),
        ("config", "coven.toml", b"[runtime"), ("tasks", "state/tasks.json", b"{}"),
        ("roster", "state/agents.json", b"{}"), ("lead", "state/lead.json", b"[]"),
        ("checkpoint", "checkpoints/plan-approval.md", b"\xff"),
        ("runtime", "state/runtime-launch-control.json", b"{"),
        ("runtime-null", "state/runtime-launch-control.json", b"null")]
    for template, attempt, label in templates:
        for name, relative, raw in poisons:
            root = s.fresh(f"source-{label}-{name}", template)
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if raw is None:
                target.unlink()
            else:
                target.write_bytes(raw)
            revision = v1.Store(root).read().revision
            s.check(f"source/{label}/{name}/confirm", confirm_args(root, revision), root, category="source_independence")
            args = cancel_args(root, attempt, revision)
            s.check(f"source/{label}/{name}/cancel", args, root, effect="cancel", category="source_independence")
            s.check(f"source/{label}/{name}/retry", args, root, category="source_independence")


def parser_cases(s, template, attempt):
    revision = v1.Store(template).read().revision
    # Every parser call gets a disposable authority: successful parsing can write.
    cases = [("default-workspace", lambda a: a[:3] + a[4:], 0),
        ("prefixes", lambda a: ["--rea" if x == "--reason" else "--expect-e" if x == "--expect-execution-revision" else "--request" if x == "--request-id" else x for x in a] + ["--j"], 0),
        ("repeat-last", lambda a: a + ["--reason", "last", "--request-id", "last-key", "--expect-execution-revision", revision, "--json", "--json"], 0),
        ("equals", lambda a: a[:5] + ["--reason=equals", "--expect-execution-revision=" + revision, "--request-id=equals"], 0),
        ("negative-reason", lambda a: a[:6] + ["-12"] + a[7:], 0),
        ("unicode-negative", lambda a: a[:6] + ["-\u0661\u0662"] + a[7:], 0),
        ("nondecimal-negative", lambda a: a[:6] + ["-\u00b2"] + a[7:], 2),
        ("double-dash", lambda a: a[:3] + a[5:] + ["--", a[3], a[4]], 0),
        ("between-positionals", lambda a: a[:4] + a[5:] + [a[4]], 2),
        ("unknown", lambda a: a + ["--invalid"], 2),
        ("bool-value", lambda a: a + ["--json=true"], 2),
        ("missing-reason", lambda a: a[:5] + a[7:], 2),
        ("missing-revision", lambda a: a[:7] + a[9:], 2),
        ("missing-key", lambda a: a[:9], 2),
        ("no-selector", lambda a: a[:3] + a[5:], 2),
        ("empty-reason", lambda a: a[:6] + [""] + a[7:], 1),
        ("invalid-id", lambda a: a[:4] + ["A" * 32] + a[5:], 1),
        ("invalid-key", lambda a: a[:10] + ["bad/key"], 1),
        ("invalid-revision", lambda a: a[:8] + ["bad"] + a[9:], 1),
        ("ambiguous-prefix", lambda a: a + ["--re", "value"], 2),
        ("overlong-reason", lambda a: a[:6] + ["x" * 16385] + a[7:], 1),
        ("whitespace-reason", lambda a: a[:6] + [" \t\r\n"] + a[7:], 1),
        ("negative-key", lambda a: a[:10] + ["-12"], 1),
        ("negative-revision", lambda a: a[:8] + ["-12"] + a[9:], 1),
        ("negative-id", lambda a: a[:4] + ["-12"] + a[5:], 1),
        ("option-before-command", lambda a: a[:2] + ["--json"] + a[2:], 2),
        ("help", lambda a: a[:3] + ["--help"], 0)]
    for name, transform, code in cases:
        root = s.fresh("parser-cancel-" + name, template)
        args = transform(cancel_args(root, attempt, revision))
        s.check("parser/cancel/" + name, args, root, code=code, category="parser",
                effect="cancel" if code == 0 and name != "help" else "none")
    for name, flags, code in (("missing-mode", [], 2),
        ("both-modes", ["--confirm-authority", "--archive-pending"], 2),
        ("same-mode-repeat", ["--confirm-authority", "--confirm-authority", "--expect-execution-revision", revision], 0),
        ("prefix", ["--confirm-a", "--expect-e", revision, "--j"], 0),
        ("no-pin", ["--confirm-authority"], 1),
        ("disabled-pin", ["--confirm-authority", "--expect-execution-revision", v1.DISABLED], 1),
        ("bad-pin", ["--confirm-authority", "--expect-execution-revision", "bad"], 1),
        ("negative-pin", ["--confirm-authority", "--expect-execution-revision", "-12"], 1),
        ("repeat-pin-last-valid", ["--confirm-authority", "--expect-execution-revision", "bad", "--expect-execution-revision", revision, "--json", "--json"], 0),
        ("repeat-pin-last-invalid", ["--confirm-authority", "--expect-execution-revision", revision, "--expect-execution-revision", "bad"], 1),
        ("equals-pin", ["--confirm-authority", "--expect-execution-revision=" + revision], 0),
        ("group-error-before-pin", ["--confirm-authority", "--archive-pending", "--expect-execution-revision", "bad"], 2),
        ("option-before-command", ["--confirm-authority", "--json=true"], 2),
        ("unknown", ["--confirm-authority", "--invalid"], 2),
        ("bool-value", ["--archive-pending=true"], 2), ("help", ["--help"], 0)):
        root = s.fresh("parser-recover-" + name, template)
        (root / "state/.workspace.lock").unlink()
        # Successful confirmation may create the lock, but never changes control.
        s.check("parser/recover/" + name, ["execution", "recover", root, *flags], root,
                code=code, category="parser", creates_lock=code == 0 and name != "help")
    for name, args in (("default", ["--confirm-authority", "--expect-execution-revision", revision]),
                       ("double-dash", ["--confirm-authority", "--expect-execution-revision", revision, "--", "."]),
                       ("flags-first", ["--confirm-authority", "--expect-execution-revision", revision, "."])):
        root = s.fresh("recover-position-" + name, template)
        s.check("parser/recover/" + name, ["execution", "recover", *args], root, category="parser")
    for name, updates, corrupt, contains, native_error in (("reason", {6: ""}, False, "cancellation reason", "invalid_execution_bounded_text"),
        ("key", {10: "bad/key"}, False, "request key", "invalid_execution_request_or_criterion_id"),
        ("authority-before-reason", {6: ""}, True, "present execution control must be an object", "invalid_execution_fields")):
        root = s.fresh("precedence-" + name, template)
        (root / "state/.workspace.lock").unlink()
        if corrupt:
            (root / v1.CONTROL).write_bytes(b"null")
        args = cancel_args(root, attempt, revision)
        for index, value in updates.items():
            args[index] = value
        s.check("precedence/" + name, args, root, code=1, category="precedence", contains=contains, native_contains=native_error)
    root = s.fixture("disabled-no-lock")
    s.check("disabled/no-lock", cancel_args(root, attempt, revision), root, code=1, category="precedence", contains="disabled")
    bare = s.root / "bare-existing-directory"
    bare.mkdir()
    s.check("disabled/bare-no-state", cancel_args(bare, attempt, revision), bare, code=1,
            category="precedence", contains="disabled", native_contains="execution_disabled")
    s.check("confirm/bare-bad-pin-no-state", confirm_args(bare, "bad"), bare, code=1, category="precedence")


def archive_cases(s, template):
    for compact in (False, True):
        root = s.fixture("pending-" + str(compact))
        (root / "coven.json").unlink()
        pending = root / v1.DIR
        pending.mkdir()
        (pending / "raw-orphan").write_bytes(b"\x00\xffnot JSON\n")
        (pending / "nested").mkdir()
        (pending / "nested/symlink").symlink_to("../../outside-must-not-follow")
        (pending / "nested/evidence").write_bytes(b"exact evidence")
        (pending / "nested/evidence").chmod(0o640)
        # Establish persistent lock so rename verification is strict.
        with storage.workspace_lock(root, root / "state/.workspace.lock"):
            pass
        s.check(f"archive/whole/{compact}", ["execution", "recover", root, "--archive-pending", *(["--json"] if compact else [])], root,
                effect="archive", category="archive")
    for kind in ("null", "bad", "symlink", "fifo", "directory", "active"):
        root = s.fresh("archive-control-" + kind, template)
        control = root / v1.CONTROL
        if kind != "active":
            control.unlink()
            if kind in ("null", "bad"):
                control.write_bytes(b"null" if kind == "null" else b"{")
            elif kind == "symlink":
                control.symlink_to(s.root / "absent")
            elif kind == "fifo":
                os.mkfifo(control)
            else:
                control.mkdir()
        s.check("archive/present/" + kind, ["execution", "recover", root, "--archive-pending"], root, code=1, category="archive")
    root = s.fixture("archive-pin-lock-timing")
    (root / v1.DIR).mkdir()
    s.check("archive/pin-under-lock", ["execution", "recover", root, "--archive-pending", "--expect-execution-revision", "bad"], root,
            code=1, category="precedence", creates_lock=True, contains="Archive does not accept",
            native_contains="archive_does_not_accept_execution_revision")
    for kind in ("missing", "symlink", "fifo", "file"):
        root = s.fixture("archive-unsafe-pending-" + kind)
        with storage.workspace_lock(root, root / "state/.workspace.lock"):
            pass
        target = root / v1.DIR
        if kind == "symlink":
            target.symlink_to(template / v1.DIR)
        elif kind == "fifo":
            os.mkfifo(target)
        elif kind == "file":
            target.write_bytes(b"not directory")
        s.check("archive/unsafe-pending/" + kind, ["execution", "recover", root, "--archive-pending"], root,
                code=1, category="archive")


def oracle_faults(s, template, attempt):
    """Python characterization only; never advertised as native fault coverage."""
    def check(name, callback):
        try:
            callback()
            s.counts["oracle_private_fault_contracts"] += 1
        except Exception as exc:
            s.failures.append({"case": name, "error": traceback.format_exc()[-4000:]})

    def archive_sync():
        root = s.fixture("oracle-archive-sync")
        (root / v1.DIR).mkdir()
        (root / v1.DIR / "orphan").write_bytes(b"raw\xff")
        sync = storage.sync_directory
        def fail(path):
            if Path(path) == root / "state":
                raise OSError("private archive sync failure")
            return sync(path)
        with patch.object(storage, "sync_directory", fail):
            code, out, err = python_cli(["execution", "recover", root, "--archive-pending", "--json"], root)
        assert code == 1 and out == b"" and b"durability uncertain" in err
        assert not (root / v1.DIR).exists()
        archived, = (root / "state").glob("execution-pending-archive-*")
        assert (archived / "orphan").read_bytes() == b"raw\xff"
    check("oracle/archive-postrename-sync-exit1", archive_sync)

    for schema in (1, 2):
        def uncertain(schema=schema):
            root, aid = prepared(s, "oracle-uncertain-" + str(schema), schema)
            args = cancel_args(root, aid, v1.Store(root).read().revision)
            with patch.object(v1.Store, "confirm_durability", side_effect=storage.StorageError("private confirmation failure")), \
                    patch.object(v2, "_confirm", side_effect=storage.StorageError("private confirmation failure")):
                code, out, err = python_cli(args + ["--json"], root)
                assert code == 2 and not err
                value = presentation(out, args + ["--json"])
                assert value["delivery"]["current_revision"] is None
                assert value["delivery"]["attempt_state"] is None
                assert value["delivery"]["known_control_revision"] == v1.Store(root).read().revision
                code, out, err = python_cli(args + ["--json"], root)
                retry = presentation(out, args + ["--json"])
                assert code == 2 and retry["delivery"]["replayed"] and not err
                exact(value["result"], retry["result"])
                with storage.workspace_lock(root, root / "state/.workspace.lock") as lock:
                    try:
                        v1.guard_task_mutation(root, "zeta", lock)
                    except storage.StorageError:
                        pass
                    else:
                        raise AssertionError("later task guard skipped independent confirmation")
            code, out, err = python_cli(args + ["--json"], root)
            assert code == 0 and not err
            exact(value["result"], presentation(out, args + ["--json"])["result"])
        check("oracle/uncertainty/" + str(schema), uncertain)


def profile_cases(s, template, attempt):
    # Python-valid control exceeds native input cap; refusal must not prune it.
    root = s.fresh("profile-8MiB", template)
    control = v1._json((root / v1.CONTROL).read_bytes())
    control["extensions"]["padding"] = ["x" * (1024**2)] * 8
    resign(control)
    (root / v1.CONTROL).write_bytes(v1.canonical(control))
    revision = v1.Store(root).read().revision
    s.check("profile/input-8MiB", cancel_args(root, attempt, revision), root, effect="cancel",
            category="profile", profile="handoff_size_limit")

    root = s.fresh("profile-successor-utf8", template)
    control = v1._json((root / v1.CONTROL).read_bytes())
    control["extensions"]["padding"] = ["x" * (1024**2)] * 7 + [""]
    overhead = len(v1.canonical(control)) + 1
    control["extensions"]["padding"][-1] = "x" * (8 * 1024**2 - 32768 - overhead)
    resign(control)
    (root / v1.CONTROL).write_bytes(v1.canonical(control) + b"\n")
    assert len((root / v1.CONTROL).read_bytes()) < 8 * 1024**2
    revision = v1.Store(root).read().revision
    def oversized_successor(value):
        raw = (root / v1.CONTROL).read_bytes()
        assert raw == v1.canonical(v1._json(raw)) + b"\n"
        assert len(raw) > 8 * 1024**2, "UTF-8 successor must exceed the native control cap"

    s.check("profile/successor-UTF8-headroom", cancel_args(root, attempt, revision, reason="\U0001f98a" * 4096), root,
            effect="cancel", category="profile", profile="unsupported_json_size_profile", verify=oversized_successor)

    # Use only the checked pure reducer to grow valid history, never patched
    # replay or a native limit increase. Each prefix and receipt stays intact.
    control = v1._json((template / v1.CONTROL).read_bytes())
    view = v1.Store(template).read()
    enrollments, attempts = deepcopy(view.enrollments), deepcopy(view.attempts)
    prepare_entry = deepcopy(control["history"][-1])

    def extend(length):
        while len(control["history"]) < length:
            previous = v1._revision(control)
            generation = control["generation"] + 1
            active = next((a for a in attempts.values() if a["state"] == "prepared"), None)
            if active:
                binding = v1.request_binding("cancel", {"attempt_id": active["prepared"]["attempt_id"], "reason": "profile"}, previous, None)
                effect = {"cancel_id": uuid.uuid4().hex, **binding["parameters"], "cancelled_at": "2026-01-01T00:00:00+00:00"}
            else:
                binding = deepcopy(prepare_entry["binding"])
                binding["expected_execution_revision"] = previous
                effect = deepcopy(prepare_entry["effect"])
                older = [a["prepared"] for a in attempts.values()]
                effect["prepared"].update(attempt_id=uuid.uuid4().hex,
                    attempt_number=len(older) + 1, previous_attempt_id=older[-1]["attempt_id"])
            result = v1._apply(control, enrollments, attempts, binding, effect, generation)
            control["generation"] = generation
            control["history"].append({"generation": generation, "request_id": f"profile-{generation}",
                "binding": binding, "effect": effect, "result": result})

    extend(39)  # Odd histories end with a live preparation.
    work = deepcopy(control)
    work["extensions"]["padding"] = ["w" * (1024**2)] * 7
    resign(work)
    root = s.fresh("profile-work", template)
    (root / v1.CONTROL).write_bytes(v1.canonical(work))
    view = v1.Store(root).read()
    aid = next(aid for aid, a in view.attempts.items() if a["state"] == "prepared")
    s.check("profile/replay-256MiB", cancel_args(root, aid, view.revision), root,
            effect="cancel", category="profile", profile="unsupported_execution_work_profile")
    extend(1025)
    root = s.fresh("profile-history", template)
    (root / v1.CONTROL).write_bytes(v1.canonical(control))
    view = v1.Store(root).read()
    aid = next(aid for aid, a in view.attempts.items() if a["state"] == "prepared")
    s.check("profile/history-1025", cancel_args(root, aid, view.revision), root,
            effect="cancel", category="profile", profile="unsupported_execution_history_profile")


def cross_workflow(s, template, attempt):
    root = s.fresh("cross-workflow", template)
    # This hand-built fixture must pass Python's real source loader too, not
    # merely the pure Store's setup loader.
    with patch.dict(os.environ, ENV, clear=True):
        coven.load_workspace_state(root)
    args = cancel_args(root, attempt, v1.Store(root).read().revision) + ["--json"]
    first = s.check("workflow/python-prepare-native-cancel", args, root, effect="cancel", category="cross_workflow")
    original_control = (root / v1.CONTROL).read_bytes()
    for name, command in (("add", ["task", "add", root, "Unrelated offline task", "--json"]),
                           ("assign", ["task", "assign", root, "zeta", "other", "--json"]),
                           ("import", ["state", "import-tasks", root, "--expect-revision",
                                       storage.snapshot_revision((root / "state/tasks.json").read_bytes()), "--json"])):
        if name == "import":
            command[command.index("--expect-revision") + 1] = storage.snapshot_revision((root / "state/tasks.json").read_bytes())
        if s.oracle_only:
            out, err = io.StringIO(), io.StringIO()
            with chdir(root), redirect_stdout(out), redirect_stderr(err), patch.dict(os.environ, ENV, clear=True), \
                    patch.object(subprocess, "Popen", forbidden):
                code = coven.main(list(map(str, command)))
            assert code == 0 and not err.getvalue(), (name, err.getvalue())
        else:
            result = subprocess.run([s.binary, *map(str, command)], cwd=root, env=ENV, capture_output=True, timeout=12)
            assert result.returncode == 0 and not result.stderr, (name, result)
        assert (root / v1.CONTROL).read_bytes() == original_control
        code, out, err = python_cli(args, root)
        assert code == 0 and not err
        exact(presentation(out, args)["result"], first["result"])
        s.counts["cross_workflow"] += 1


def cli_command(s, args, native):
    if native and not s.oracle_only:
        return [s.binary, *map(str, args)]
    return [sys.executable, "-B", "-c",
            "from runner import coven; import sys; raise SystemExit(coven.main(sys.argv[1:]))", *map(str, args)]


def races(s, template, attempt, label):
    """Both real CLIs compete behind the exact same existing flock inode."""
    for scenario in ("CAS", "same-key", "key-conflict", "add", "assign", "import"):
        root = s.fresh(f"race-{label}-{scenario}", template)
        revision = v1.Store(root).read().revision
        inode = (root / "state/.workspace.lock").stat().st_ino
        key = "shared" if scenario in ("same-key", "key-conflict") else "python"
        args = cancel_args(root, attempt, revision, key) + ["--json"]
        competing = cancel_args(root, attempt, revision, "native" if scenario == "CAS" else key,
                                reason="changed" if scenario == "key-conflict" else "  exact \u03bb \U0001f98a  ") + ["--json"]
        if scenario == "add":
            competing = ["task", "add", root, "Unrelated race task", "--json"]
        elif scenario == "assign":
            competing = ["task", "assign", root, "zeta", "other", "--json"]
        elif scenario == "import":
            competing = ["state", "import-tasks", root, "--expect-revision",
                         storage.snapshot_revision((root / "state/tasks.json").read_bytes()), "--json"]
        tasks_before = (root / "state/tasks.json").read_bytes()
        with ExitStack() as stack:
            held = stack.enter_context((root / "state/.workspace.lock").open("r+b"))
            fcntl.flock(held, fcntl.LOCK_EX)
            children = []
            for command in (cli_command(s, args, False), cli_command(s, competing, True)):
                child = subprocess.Popen(command, cwd=Path(__file__).resolve().parents[1], env=ENV,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                children.append(child)
                stack.callback(lambda child=child: child.kill() if child.poll() is None else None)
            if Path("/proc/self/fd").is_dir():
                # Observe both descriptors on the actual lock inode before
                # releasing the barrier, not just children that haven't started.
                deadline = time.monotonic() + 4
                def opened(child):
                    for fd in Path(f"/proc/{child.pid}/fd").iterdir():
                        try:
                            if fd.stat().st_ino == inode:
                                return True
                        except FileNotFoundError:
                            pass
                    return False
                while not all(opened(child) for child in children):
                    assert all(child.poll() is None for child in children), scenario
                    assert time.monotonic() < deadline, (scenario, "child did not open shared lock")
                    time.sleep(0.02)
            # Holding the lock makes completion impossible for either writer.
            assert not select.select([child.stdout for child in children], [], [], 0.2)[0]
            assert all(child.poll() is None for child in children)
            fcntl.flock(held, fcntl.LOCK_UN)
            outputs = [child.communicate(timeout=12) for child in children]
            if scenario in ("CAS", "same-key", "key-conflict"):
                assert sorted(child.returncode for child in children) == ([0, 0] if scenario == "same-key" else [0, 1]), outputs
            else:
                assert children[0].returncode == 0, outputs
                assert children[1].returncode in (0, 1), outputs
                if scenario == "add":
                    assert children[1].returncode == 0, outputs
                elif children[1].returncode == 1:
                    assert outputs[1][0] == b"" and b"prepared" in outputs[1][1].lower(), outputs
                    assert (root / "state/tasks.json").read_bytes() == tasks_before
                    retry = subprocess.run(cli_command(s, competing, True), cwd=Path(__file__).resolve().parents[1],
                                           env=ENV, capture_output=True, timeout=12)
                    assert retry.returncode == 0, retry
            if scenario == "same-key":
                values = [presentation(out, args) for out, err in outputs]
                exact(values[0]["result"], values[1]["result"])
                assert sorted(v["delivery"]["replayed"] for v in values) == [False, True]
            assert v1.Store(root).read().generation == v1.Store(template).read().generation + 1
            assert (root / "state/.workspace.lock").stat().st_ino == inode
            s.counts["oracle_cli_races" if s.oracle_only else "mixed_cli_races"] += 1


def process_death(s, template):
    root = s.fresh("lock-owner-death", template)
    lock = root / "state/.workspace.lock"
    before = snapshot(root)
    holder_code = """import fcntl,sys
with open(sys.argv[1], 'r+b') as lock:
 fcntl.flock(lock, fcntl.LOCK_EX)
 print('held', flush=True)
 sys.stdin.read()
"""
    with ExitStack() as stack:
        holder = subprocess.Popen([sys.executable, "-B", "-c", holder_code, str(lock)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=ENV)
        stack.callback(lambda: holder.kill() if holder.poll() is None else None)
        assert select.select([holder.stdout], [], [], 4)[0]
        assert holder.stdout.readline() == b"held\n"
        command = cli_command(s, confirm_args(root, v1.Store(root).read().revision) + ["--json"], True)
        writer = subprocess.Popen(command, cwd=Path(__file__).resolve().parents[1], env=ENV,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stack.callback(lambda: writer.kill() if writer.poll() is None else None)
        assert not select.select([writer.stdout], [], [], 0.2)[0] and writer.poll() is None
        holder.kill()
        holder.communicate(timeout=4)
        out, err = writer.communicate(timeout=12)
        assert writer.returncode == 0 and not err and v1._json(out)["outcome"] == "confirmed", (out, err)
        assert snapshot(root) == before, "process death replaced lock inode or modified authority"
        s.counts["lock_process_death"] += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="coven-execution-maintenance-") as temporary:
        suite = Suite(Path(temporary), str(Path(args.binary).resolve()), args.oracle_only)
        run(suite)
        print(json.dumps({"cases": sum(suite.counts.values()), "cli_cases": len(suite.names),
            "categories": dict(suite.counts), "failures": suite.failures,
            "oracle_only": args.oracle_only, "native_executed": not args.oracle_only,
            "required_main_routes": ["task attempt cancel [workspace] ID --reason TEXT --expect-execution-revision TOKEN --request-id KEY [--json]",
                "execution recover [workspace] (--confirm-authority --expect-execution-revision TOKEN | --archive-pending) [--json]"],
            "owner_unit_gates": ["confirmation prepared allowed, full reread, UUID/revision CAS, control/execution/state/root fsync order",
                "archive collision, unsafe paths, rename failure, postrename SyncError exit1 with known path and no false rollback",
                "cancel every publication boundary and post-known read/profile/sync failure retains exact receipt exit2",
                "candidate 8MiB/1024 combined history/256MiB work and escaped-output admission before replacement"],
            "gaps": ["native private fault coverage must be supplied by owners; Python characterization is not native injection",
                "successor profile headroom boundaries require private owner units"],
            "claims_process_stop_or_dispatch": False}, ensure_ascii=True))
        return bool(suite.failures)


if __name__ == "__main__":
    raise SystemExit(main())
