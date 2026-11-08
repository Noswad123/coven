"""Historical action inspection only: no execution, submission, or current team.

Fixtures are created at their final roots: authority binds both path and inode.
Only named diagnostic fields admit the explicitly classified native errors below;
all other JSON values compare as canonical bytes (not Python's bool/int equality).
"""
import argparse
from contextlib import ExitStack, redirect_stderr
from copy import deepcopy
import json
import io
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import actions, checkpoints, coven, launch, storage

A = "a" * 32
B = "b" * 32
MIB = 1024 * 1024


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def snapshot(root, held_readers=None):
    result = {}

    def visit(path):
        info = path.lstat()
        identity = (info.st_mode, info.st_dev, info.st_ino, info.st_mtime_ns)
        key = os.fsencode(path.relative_to(root))
        if stat.S_ISLNK(info.st_mode):
            result[key] = (*identity, "symlink", os.fsencode(os.readlink(path)))
        elif stat.S_ISDIR(info.st_mode):
            result[key] = (*identity, "directory")
            for child in sorted(path.iterdir()):
                visit(child)
        elif stat.S_ISREG(info.st_mode):
            data = (os.pread(held_readers[path], info.st_size, 0)
                    if held_readers and path in held_readers else path.read_bytes())
            result[key] = (*identity, "file", data)
        else:
            result[key] = (*identity, "special", info.st_rdev)
    visit(root)
    return result


def diagnostic(text, kind):
    """Unknown Python diagnostics are oracle failures, never normalization."""
    patterns = {
        "bundle_missing": r": required authority file missing$",
        "unsafe_path": r"unsafe path type or symlink|symlinks are not supported|Managed path must be a regular file:|not a directory",
        "unavailable": r"cannot read authority:|Cannot read managed checkpoint|cannot inspect action path:",
        "invalid": r"^invalid |^unsupported authority version|^plan root or namespace differs|^inputs must be|^bundle filename|^Cannot edit checkpoint:",
    }
    assert kind in patterns and re.search(patterns[kind], text), (kind, text)
    return kind


class Fixture:
    def __init__(self, root, stack, backend="tmux", agents=1):
        self.root = root
        root.mkdir()
        (root / "state").mkdir()
        (root / "checkpoints").mkdir()
        self.store = actions.ActionStore(root)
        self.gate = stack.enter_context(storage.transport_gate(
            root, root / "state/.runtime-transport.lock", exclusive=True))
        self.lock = stack.enter_context(storage.workspace_lock(root, root / "state/.workspace.lock"))
        self.control = self.store.enable(self.lock, gate=self.gate)
        self.backend, self.agents = backend, agents
        self.inputs = {"not_a_producer_inventory": [True, 1, 1.0, -0.0, None,
                       {"unicode": "雪", "integer": 10**100, "tiny": 1e-200}]}

    def prepare(self, action_id=A):
        roster = [{"id": f"worker_{i}", "command": "exec worker 'quoted'", "starter": "Start\nnow"}
                  for i in range(self.agents)]
        plan = launch.build_plan(str(self.root),
            f"coven-{self.control['workspace_id']}-{action_id}", self.backend,
            roster, {"PATH": "/frozen/bin", "HOME": "/frozen/home", "LANG": "", "TERM": "xterm"})
        bundle = self.store.make_bundle(self.inputs, plan, action_id, self.lock)
        self.store.publish_bundle(bundle, self.lock)
        cp = actions.checkpoint_id(bundle)
        raw, view = checkpoints.prepare(cp, actions.checkpoint_bytes(bundle), "approved", "reviewed",
            decision_id=("c" if action_id == A else "d") * 32, decided_at="frozen time", source="review")
        self.cp(action_id).write_bytes(raw)
        return bundle, view

    def cp(self, action_id=A):
        return self.root / "checkpoints" / f"runtime-launch-{action_id}.md"

    def bundle_path(self, action_id=A):
        return self.root / actions.BUNDLES / f"{action_id}.json"

    def claim(self, action_id=A):
        bundle, view = self.prepare(action_id)
        claim, consumed = self.store.claim(action_id, actions.digest(bundle), view.revision,
            view.decision["decision_id"], self.inputs, self.lock)
        assert not consumed
        return claim

    def write_control(self, control):
        (self.root / actions.CONTROL).write_bytes(canonical(control))


def report(status="transport_completed"):
    # These are deliberately not constrained to the frozen plan's step count,
    # actual runtime output paths, or a fixed diagnostic vocabulary.
    return {"operation": "runtime_launch", "status": status, "completed_steps": 10**100,
            "handles": {"Unexpected_1": "native-handle"}, "outcome_unknown": True,
            "diagnostics": [{"step": None, "error": "", "message": "\n雪"},
                            {"step": 10**90, "error": "arbitrary", "message": "anything"}]}


def main(binary=None, oracle_only=False):
    counts = {"parity": 0, "control_refusals": 0, "profile_refusals": 0,
              "resource_refusals": 0, "cli_refusals": 0}
    binary = str(Path(binary).resolve()) if binary else None
    names = set()
    with tempfile.TemporaryDirectory(prefix="native-action-") as temporary, ExitStack() as stack:
        root = Path(temporary).resolve()
        held_readers = {}
        # Storage helpers remain real, including held locks and the transport
        # gate; only runtime entry points are forbidden.
        for function in ("execute", "stop", "_transport"):
            stack.enter_context(patch.object(launch, function, side_effect=AssertionError("runtime called")))
        for function in ("load_team", "read_team_config", "load_workspace_state"):
            stack.enter_context(patch.object(coven, function,
                                            side_effect=AssertionError("current state read")))

        def fixture(name, **kwargs):
            return Fixture(root / name, stack, **kwargs)

        def run(workspace, action_id=A, operands=None):
            env = {**os.environ, "HOME": str(root), "PATH": str(root / "no-programs"),
                   "PYTHONPATH": str(root / "no-python"), "COVEN_MULTIPLEXER": "poison"}
            return subprocess.run([binary, "action", "inspect",
                *(operands if operands is not None else [str(workspace), action_id]), "--json"],
                cwd=workspace if workspace.is_dir() else root, env=env,
                capture_output=True, timeout=15)

        def check(name, workspace, action_id=A, *, fatal=None, bundle_error=None,
                  checkpoint_error=None, profile=None, resource=None, operands=None,
                  native_error=None, retained_claim=None):
            assert name not in names, name
            names.add(name)
            before = snapshot(root, held_readers)
            try:
                expected = actions.ActionStore(workspace).inspect(action_id)
            except storage.StorageError as error:
                assert fatal is not None and profile is None, (name, str(error))
                assert re.search(fatal, str(error)), (name, str(error), fatal)
                expected = None
            else:
                assert fatal is None, (name, "invalid control accepted by Python")
                checkpoint_diagnostic = None
                captured = io.StringIO()
                try:
                    with redirect_stderr(captured):
                        expected["checkpoint"] = coven.checkpoint_view(workspace, expected["checkpoint_id"]).to_dict()
                except (storage.StorageError, SystemExit) as error:
                    expected["checkpoint_error"] = str(error)
                    checkpoint_diagnostic = captured.getvalue() if isinstance(error, SystemExit) else str(error)
                expected["outcome"] = "inspected"
                if retained_claim is not None:
                    assert canonical(expected["claim"]) == canonical(retained_claim), name
                for field, kind in (("bundle_error", bundle_error), ("checkpoint_error", checkpoint_error)):
                    if expected.get(field) is not None:
                        assert kind is not None, (name, field, expected[field])
                        diagnostic(checkpoint_diagnostic if field == "checkpoint_error" else expected[field], kind)
                    else:
                        assert kind is None, (name, field, "expected diagnostic absent")
            if not oracle_only:
                actual = run(workspace, action_id, operands)
                if fatal is not None or profile is not None or resource is not None:
                    assert actual.returncode != 0 and actual.stdout == b"", (name, actual)
                    code = profile or resource or native_error or {
                        "version-bool": "invalid_action_version", "extra-field": "invalid_action_fields",
                        "root": "action_workspace_location_changed", "device": "action_workspace_location_changed",
                        "inode": "action_workspace_location_changed", "head-missing": "invalid_action_head",
                        "head-null": "invalid_action_head", "unselected-status": "invalid_action_claim_status",
                        "unselected-report": "invalid_action_integer", "duplicate-claim": "invalid_action_claim_identity",
                        "terminal-report-mismatch": "invalid_action_claim_report", "claimed-with-report": "invalid_action_claim_report",
                        "unknown-without-ack": "invalid_action_claim_status", "null-control": "invalid_action_fields",
                        "malformed-control": "invalid_compat_json", "duplicate-control-key": "duplicate_json_key",
                        "orphan": "action_orphan_bundles", "copy-refused": "action_workspace_location_changed",
                        "move-refused": "action_workspace_location_changed",
                    }[name]
                    assert re.fullmatch(rb"\[ERROR\] " + re.escape(code.encode()) + rb"\n", actual.stderr), (name, code, actual.stderr)
                else:
                    assert actual.returncode == 0, (name, actual.stderr)
                    assert actual.stderr == b"", (name, actual.stderr)
                    assert actual.stdout.isascii(), (name, "report must use safe ASCII JSON")
                    observed = json.loads(actual.stdout)
                    for field, kind in (("bundle_error", bundle_error), ("checkpoint_error", checkpoint_error)):
                        if kind is not None:
                            code = "action_bundle_missing" if kind == "bundle_missing" else (
                                "action_bundle_" if field == "bundle_error" else "action_checkpoint_") + kind
                            assert observed.get(field) == code, (name, field, observed.get(field))
                            expected[field] = code
                    assert canonical(observed) == canonical(expected), (name, observed, expected)
            assert snapshot(root, held_readers) == before, (name, "inspection changed filesystem")
            counts["resource_refusals" if resource else "profile_refusals" if profile else
                   "control_refusals" if fatal else "parity"] += 1

        empty = root / "empty"
        empty.mkdir()
        check("empty-no-config", empty, bundle_error="bundle_missing")
        check("current-empty", empty, bundle_error="bundle_missing", operands=[A])
        for backend in ("tmux", "herdr"):
            for size in (1, 32):
                f = fixture(f"{backend}-{size}", backend=backend, agents=size)
                f.prepare()
                check(f"{backend}-{size}-frozen", f.root)
                check(f"{backend}-{size}-current", f.root, operands=[A])

        f = fixture("history")
        claim = f.claim()
        check("claimed", f.root)
        f.store.acknowledge_unknown(A, claim["claim_id"], f.lock)
        check("unknown-acknowledged", f.root)
        newer = f.claim(B)
        check("old-unknown-new-claimed", f.root)
        f.store.record_outcome(A, claim["claim_id"], report(), f.lock)
        check("late-terminal-preserves-head", f.root)
        f.store.record_outcome(B, newer["claim_id"], report("transport_failed_or_partial"), f.lock)
        check("terminal-failure-successful-inspection", f.root, B)
        check("older-terminal", f.root)
        check("unknown-selected-id", f.root, "e" * 32, bundle_error="bundle_missing")
        valid_control = f.store.read_control()
        many = fixture("many-diagnostics")
        many_claim = many.claim()
        many_report = report()
        many_report["diagnostics"] = [{"step": i * 1000000, "error": "", "message": "free text"}
                                      for i in range(2049)]
        many.store.record_outcome(A, many_claim["claim_id"], many_report, many.lock)
        check("no-invented-report-count-limit", many.root)
        for label, change, message in [
            ("version-bool", lambda c: c.update(version=True), "version"),
            ("extra-field", lambda c: c.update(extra=0), "authority fields"),
            ("root", lambda c: c["location"].update(root="/different"), "location changed"),
            ("device", lambda c: c["location"].update(device=10**100), "location changed"),
            ("inode", lambda c: c["location"].update(inode=10**100), "location changed"),
            ("head-missing", lambda c: c.update(head="f" * 32), "head is missing"),
            ("head-null", lambda c: c.update(head=None), "require a head"),
            ("unselected-status", lambda c: c["claims"][B].update(status="invented"), "status"),
            ("unselected-report", lambda c: c["claims"][B]["report"].update(completed_steps=True), "step count"),
            ("duplicate-claim", lambda c: c["claims"][B].update(claim_id=c["claims"][A]["claim_id"]), "claim identity"),
            ("terminal-report-mismatch", lambda c: c["claims"][B]["report"].update(status="transport_completed"), "differs from report"),
            ("claimed-with-report", lambda c: c["claims"][B].update(status="claimed"), "nonterminal"),
            ("unknown-without-ack", lambda c: c["claims"][B].update(status="outcome_unknown", report=None), "acknowledgement"),
        ]:
            bad = deepcopy(valid_control)
            change(bad)
            f.write_control(bad)
            check(label, f.root, fatal=message)
        f.write_control(valid_control)
        for field in ("device", "inode"):
            bad = deepcopy(valid_control)
            bad["location"][field] += 2**64
            f.write_control(bad)
            check("location-u64-wrap-" + field, f.root, fatal="location changed",
                  native_error="action_workspace_location_changed")
        f.write_control(valid_control)
        for label, change, message, code in [
            ("workspace-id", lambda c: c.update(workspace_id="A" * 32), "action ID", "invalid_action_id"),
            ("epoch-id", lambda c: c.update(epoch_id="short"), "action ID", "invalid_action_id"),
            ("location-integer-type", lambda c: c["location"].update(inode=True), "location identity", "invalid_action_integer"),
            ("location-canonical", lambda c: c["location"].update(root=str(f.root) + "/."), "canonical workspace root", "invalid_action_location"),
            ("claim-action-id", lambda c: c["claims"][B].update(action_id=A), "claim identity", "invalid_action_claim_identity"),
            ("claim-manifest", lambda c: c["claims"][B].update(manifest_digest="bad"), "manifest digest", "invalid_action_reference"),
            ("claim-checkpoint", lambda c: c["claims"][B].update(checkpoint_revision="bad"), "checkpoint revision", "invalid_action_reference"),
            ("claim-decision", lambda c: c["claims"][B].update(decision_id="bad"), "action ID", "invalid_action_id"),
            ("claim-scope", lambda c: c["claims"][B].update(scope_revision="bad"), "scope revision", "invalid_action_reference"),
            ("ack-integer", lambda c: c["claims"][B].update(operator_acknowledged=1), "acknowledgement", "invalid_action_claim_status"),
            ("report-handle", lambda c: c["claims"][B]["report"].update(handles={"unsafe/": "x"}), "handle name", "invalid_action_report"),
            ("report-diagnostic-step", lambda c: c["claims"][B]["report"]["diagnostics"][0].update(step=-1), "diagnostic step", "invalid_action_integer"),
        ]:
            bad = deepcopy(valid_control)
            change(bad)
            f.write_control(bad)
            check(label, f.root, fatal=message, native_error=code)
        f.write_control(valid_control)
        control_path = f.root / actions.CONTROL
        for name, raw, message in [("null-control", b"null", "null control"),
                                   ("malformed-control", b"{", "strict authority JSON"),
                                   ("duplicate-control-key", b'{"version":1,"version":1}', "duplicate JSON key")]:
            control_path.write_bytes(raw)
            check(name, f.root, fatal=message)
        f.write_control(valid_control)
        saved_bundle = f.bundle_path().read_bytes()
        f.bundle_path().unlink()
        bad = deepcopy(valid_control)
        bad["claims"][B]["status"] = "corrupt-unselected"
        f.write_control(bad)
        check("missing-bundle-cannot-hide-corrupt-control", f.root, fatal="status",
              native_error="invalid_action_claim_status")
        f.bundle_path().write_bytes(saved_bundle)
        f.write_control(valid_control)
        for kind in ("symlink", "fifo", "directory"):
            control_path.unlink()
            if kind == "symlink":
                control_path.symlink_to(root / "absent-control")
            elif kind == "fifo":
                os.mkfifo(control_path)
            else:
                control_path.mkdir()
            check("unsafe-control-" + kind, f.root, fatal="unsafe path type or symlink",
                  native_error="unsafe_archive_path")
            if kind == "directory":
                control_path.rmdir()
            else:
                control_path.unlink()
            f.write_control(valid_control)
        orphan = root / "orphan"
        (orphan / actions.BUNDLES).mkdir(parents=True)
        check("orphan", orphan, fatal="orphan")

        # Copies/moves are intentional invalid authority, never semantic fixtures.
        copied = root / "copied"
        shutil.copytree(f.root, copied)
        check("copy-refused", copied, fatal="location changed")
        moved = root / "moved"
        copied.rename(moved)
        check("move-refused", moved, fatal="location changed")
        rebound = fixture("same-path-new-inode")
        rebound.prepare()
        original = root / "same-path-original-inode"
        rebound.root.rename(original)
        shutil.copytree(original, rebound.root)
        check("same-path-copy-inode-refused", rebound.root, fatal="location changed",
              native_error="action_workspace_location_changed")
        alias = root / "alias"
        alias.symlink_to(f.root, target_is_directory=True)
        check("root-symlink-alias", alias)
        check("root-dot-alias", f.root, operands=[str(f.root / "state" / ".."), A])
        check("tilde-existing-workspace", f.root, operands=["~/history", A])
        for invalid_id in ("A" * 32, "a" * 31, "../escape"):
            check("invalid-id-" + invalid_id, f.root, invalid_id, fatal="action ID", native_error="invalid_action_id")

        # A selected artifact error cannot suppress the independently valid claim.
        for artifact in ("bundle", "checkpoint"):
            path = f.bundle_path() if artifact == "bundle" else f.cp()
            saved = path.read_bytes()
            for kind in ("missing", "malformed", "symlink", "fifo", "directory"):
                path.unlink()
                if kind == "malformed":
                    path.write_bytes(b"{" if artifact == "bundle" else b"<!-- coven-checkpoint-garbage\n")
                elif kind == "symlink":
                    path.symlink_to(root / "missing-target")
                elif kind == "fifo":
                    os.mkfifo(path)
                elif kind == "directory":
                    path.mkdir()
                error = ("bundle_missing" if kind == "missing" else "invalid" if kind == "malformed" else "unsafe_path")
                options = {"bundle_error": error} if artifact == "bundle" else (
                    {} if kind == "missing" else {"checkpoint_error": error})
                check(f"claimed-{artifact}-{kind}", f.root, **options)
                if path.is_dir() and not path.is_symlink():
                    path.rmdir()
                elif path.exists() or path.is_symlink():
                    path.unlink()
                path.write_bytes(saved)
            if os.geteuid() != 0:
                mode = stat.S_IMODE(path.stat().st_mode)
                with path.open("rb") as reader:
                    held_readers[path] = reader.fileno()
                    path.chmod(0)
                    try:
                        check(f"claimed-{artifact}-unavailable", f.root,
                              **({"bundle_error": "unavailable"} if artifact == "bundle" else {"checkpoint_error": "unavailable"}))
                    finally:
                        path.chmod(mode)
                        del held_readers[path]

        # Frozen steps must be exactly rebuildable, including JSON numeric types.
        p = fixture("mutations", backend="herdr")
        bundle, _ = p.prepare()
        mutations = [
            ("extra", lambda x: x.update(extra=None)),
            ("argv", lambda x: x["steps"][0]["argv"].append("--surprise")),
            ("outputs", lambda x: x["steps"][1]["outputs"].update(workspace=["wrong"])),
            ("delay-float", lambda x: next(s for s in x["steps"] if s["operation"] == "sleep").update(seconds=5.0)),
            ("delay-bool", lambda x: next(s for s in x["steps"] if s["operation"] == "sleep").update(seconds=True)),
            ("namespace", lambda x: x.update(namespace="coven-aa-bb")),
            ("root", lambda x: x.update(root="/elsewhere")),
            ("cwd", lambda x: x.update(cwd="/elsewhere")),
            ("env", lambda x: x["env"].update(UNSUPPORTED="x")),
            ("roster", lambda x: x["agents"].append(deepcopy(x["agents"][0]))),
            ("agent-id", lambda x: x["agents"][0].update(id="../unsafe")),
            ("steps-missing", lambda x: x["steps"].pop()),
        ]
        for name, mutate in mutations:
            bad = deepcopy(bundle)
            mutate(bad["plan"])
            p.bundle_path().write_bytes(canonical(bad))
            check("bundle-plan-" + name, p.root, bundle_error="invalid")
        p.bundle_path().write_bytes(canonical(bundle))
        for label, raw in [
            ("duplicate-bundle-key", b'{"version":1,"version":1}'),
            ("null-bundle", b"null"),
            ("nonfinite-bundle", canonical(bundle).replace(b'"version":1', b'"version":NaN')),
        ]:
            p.bundle_path().write_bytes(raw)
            check(label, p.root, bundle_error="invalid")
        p.bundle_path().write_bytes(canonical(bundle))
        pclaim, _ = p.store.claim(A, actions.digest(bundle),
            checkpoints.inspect("runtime-launch-" + A, p.cp().read_bytes()).revision,
            "c" * 32, p.inputs, p.lock)
        pc = p.store.read_control()
        for name, mutate in mutations:
            bad = deepcopy(pc)
            mutate(bad["claims"][A]["plan"])
            p.write_control(bad)
            check("control-plan-" + name, p.root, fatal="invalid frozen launch plan|plan root or namespace",
                  native_error="invalid_runtime_launch_plan")
        p.write_control(pc)

        # Reportability does not mean a stale bundle can be submitted again.
        stale = deepcopy(bundle)
        stale.update(epoch_id="f" * 32, control_revision="runtime-launch-control-v1:" + "0" * 64)
        p.bundle_path().write_bytes(canonical(stale))
        check("stale-activation-bundle-reportable", p.root)
        p.bundle_path().write_bytes(canonical(bundle))

        for relative in ("coven.json", "covenant.json", "tasks.json", "state/tasks.json",
                         "coven.toml", "goal.md", "spec.md", "state/task-authority.json",
                         "state/task-ledger", "state/task-ledger.jsonl", "state/execution/control.json",
                         "state/execution-control.json", "state/execution-history.jsonl",
                         "prompts", "providers", "state/agents.json", "state/lead.json"):
            path = p.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            os.mkfifo(path)
        check("ignore-current-state-fifos-held-locks", p.root)

        # Native value/profile errors are fatal even inside optional artifacts.
        for name, value, code in [
            ("string", "x" * (MIB + 1), "unsupported_json_string_profile"),
            ("array", [0] * 65537, "unsupported_json_array_profile"),
            ("object", {str(i): 0 for i in range(16385)}, "unsupported_json_object_profile"),
            ("depth", json.loads("[" * 65 + "0" + "]" * 65), "unsupported_json_depth_profile"),
            ("nodes", [[0] * 60000 for _ in range(5)], "unsupported_json_node_profile"),
        ]:
            large = deepcopy(bundle)
            large["inputs"] = {"opaque": value}
            p.bundle_path().write_bytes(canonical(large))
            check("optional-bundle-profile-" + name, p.root, profile=code)
        p.bundle_path().write_bytes(b" " * (8 * MIB) + canonical(bundle))
        check("native-input-eight-mib-python-sixteen", p.root, profile="unsupported_action_input_profile")
        p.bundle_path().write_bytes(canonical(bundle))
        p.write_control(pc)
        (p.root / actions.CONTROL).write_bytes(b" " * (8 * MIB) + canonical(pc))
        check("control-input-eight-mib", p.root, profile="unsupported_action_input_profile")
        p.write_control(pc)
        cp_raw = p.cp().read_bytes()
        p.cp().write_bytes(b"x" * (MIB + 1))
        check("optional-checkpoint-one-mib", p.root, profile="unsupported_checkpoint_input_profile")
        p.cp().write_bytes(cp_raw)

        # Surrogate-escaped POSIX paths have an explicit native profile refusal.
        nonunicode = root / os.fsdecode(b"nonunicode-\xff")
        nonunicode.mkdir()
        check("non-utf8-root-profile", nonunicode, bundle_error="bundle_missing",
              profile="non_unicode_path_unsupported")

        oversized_plan = deepcopy(bundle)
        roster = deepcopy(bundle["plan"]["agents"])
        roster[0]["command"] = "x" * (MIB + 1)
        oversized_plan["plan"] = launch.build_plan(str(p.root), bundle["plan"]["namespace"],
            "herdr", roster, bundle["plan"]["env"])
        p.bundle_path().write_bytes(canonical(oversized_plan))
        check("optional-nested-plan-value-profile", p.root, profile="unsupported_json_string_profile")
        p.bundle_path().write_bytes(canonical(bundle))
        oversized_control = deepcopy(pc)
        oversized_control["claims"][A]["plan"] = oversized_plan["plan"]
        p.write_control(oversized_control)
        check("control-nested-plan-value-profile", p.root, profile="unsupported_json_string_profile")
        p.write_control(pc)

        expanded = deepcopy(bundle)
        expanded["inputs"] = {str(i): "\x7f" * 900000 for i in range(7)}
        assert len(canonical(expanded)) < 8 * MIB
        p.bundle_path().write_bytes(canonical(expanded))
        check("safe-ascii-output-expansion", p.root, profile="unsupported_json_output_profile")
        p.bundle_path().write_bytes(canonical(bundle))

        # JSON encodings accepted by Python's bytes decoder are deliberately
        # outside the native preview profile, including optional bundle reads.
        encoding = fixture("encoding-profile")
        encoding.claim()
        for artifact in ("control", "bundle"):
            path = encoding.root / actions.CONTROL if artifact == "control" else encoding.bundle_path()
            original = path.read_bytes()
            for codec in ("utf-8-sig", "utf-16-le"):
                encoded = original.decode("utf-8").encode(codec)
                assert canonical(json.loads(encoded)) == canonical(json.loads(original))
                path.write_bytes(encoded)
                check(f"{artifact}-{codec}-encoding-profile", encoding.root,
                      profile="unsupported_action_encoding_profile")
                path.write_bytes(original)

        # A bounded dedicated checkpoint still applies the source value profile
        # before interpreting its closed decision-record schema. Python rejects
        # these extra record fields (or the long integer); that does not license
        # native code to erase a resource error into checkpoint_error.
        source = fixture("checkpoint-source-resource")
        source_claim = source.claim()
        original = source.cp().read_bytes()
        decision = checkpoints.inspect("runtime-launch-" + A, original).decision
        prefix = canonical(decision)[:-1] + b',"payload":'
        for name, payload, code in (
            ("depth", b"[" * 65 + b"0" + b"]" * 65, "unsupported_json_depth_profile"),
            ("integer", b"1" * 4301, "unsupported_integer_profile"),
            ("array", b"[" + b"0," * 65536 + b"0]", "unsupported_json_array_profile"),
        ):
            raw = b"<!-- coven-checkpoint-decision:v1\n" + prefix + payload + b"}\n-->\n"
            assert len(raw) < MIB
            source.cp().write_bytes(raw)
            check("checkpoint-source-resource-" + name, source.root,
                  checkpoint_error="invalid", resource=code, retained_claim=source_claim)
        source.cp().write_bytes(b"<!-- coven-checkpoint-decision:v1\n{\n-->\n")
        check("checkpoint-source-ordinary-malformed-json", source.root,
              checkpoint_error="invalid", retained_claim=source_claim)
        source.cp().write_bytes(original)

        # Neither authority nor an individual value is oversized. Selecting the
        # terminal claim duplicates its report in the envelope, and only that
        # duplication pushes safe ASCII presentation beyond 32 MiB.
        duplicate = fixture("selected-claim-ascii-duplication")
        duplicate_claim = duplicate.claim()
        large_report = report()
        large_report["diagnostics"] = [
            {"step": None, "error": "", "message": "\x7f" * 900000} for _ in range(4)]
        terminal_claim = duplicate.store.record_outcome(A, duplicate_claim["claim_id"], large_report, duplicate.lock)
        control = duplicate.store.read_control()
        assert len((duplicate.root / actions.CONTROL).read_bytes()) < 8 * MIB
        assert len(json.dumps(control, ensure_ascii=True).encode("ascii")) < 32 * MIB
        envelope = duplicate.store.inspect(A)
        envelope["checkpoint"] = coven.checkpoint_view(duplicate.root, envelope["checkpoint_id"]).to_dict()
        envelope["outcome"] = "inspected"
        assert len(canonical(envelope)) < 32 * MIB
        assert len(json.dumps(envelope, ensure_ascii=True).encode("ascii")) > 32 * MIB
        unselected = deepcopy(envelope)
        unselected["claim"] = None
        assert len(json.dumps(unselected, ensure_ascii=True).encode("ascii")) < 32 * MIB
        check("unselected-terminal-ascii-fits", duplicate.root, B, bundle_error="bundle_missing")
        check("selected-terminal-ascii-duplication", duplicate.root,
              resource="unsupported_json_output_profile", retained_claim=terminal_claim)

        # Replace lock names only after the last storage operation in this
        # fixture. Inspection must not open either FIFO or acquire a lock.
        fifo_locks = fixture("ignored-lock-fifos")
        fifo_claim = fifo_locks.claim()
        check("before-lock-fifos", fifo_locks.root, retained_claim=fifo_claim)
        for relative in ("state/.workspace.lock", "state/.runtime-transport.lock"):
            path = fifo_locks.root / relative
            path.unlink()
            os.mkfifo(path)
        check("ignore-both-lock-fifos", fifo_locks.root, retained_claim=fifo_claim)

        if not oracle_only:
            # Writers are native now. Keep these five parser/no-effect guards
            # with an invalid flag instead of treating supported routes as gaps.
            for command in ("submit", "stop", "acknowledge-unknown", "prepare-runtime-launch", "enable-runtime-launch"):
                before = snapshot(root)
                result = subprocess.run([binary, "action", command, "--invalid-action-option", str(p.root), A, "--json"],
                    capture_output=True, timeout=15)
                assert result.returncode != 0 and result.stdout == b"", (command, result)
                assert b"unexpected argument" in result.stderr, (command, result.stderr)
                assert snapshot(root) == before, command
                counts["cli_refusals"] += 1
    print("action authority cases: " + json.dumps(counts, sort_keys=True))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    args = parser.parse_args()
    if not args.oracle_only and not args.binary:
        parser.error("--binary is required unless --oracle-only")
    main(args.binary, args.oracle_only)
