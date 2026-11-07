"""Full checkpoint inspect wrapper oracle; no runtime, writes, or live app state.

Run --oracle-only to classify fixtures before the native CLI is wired. Native
preview requires --json and may reject repeated --json at the parser boundary;
Python accepts both. Named-user/missing-HOME expansion, non-UTF8 paths, bounded
resources and finite scalar JSON are explicit unsupported profiles, not claims
that Python rejects those inputs. Pure grammar fuzzing and concurrent authority
transitions belong to the checkpoint/ledger unit owners, not this suite.
This filesystem harness targets Linux; it does not claim macOS validation.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, chdir, contextmanager, redirect_stdout, redirect_stderr
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import actions, checkpoints, coven, launch, storage, task_attempts, task_ledger

MIB = 1024 * 1024
A, B = "a" * 32, "b" * 32
TEMPLATE = (b"# Plan\n\nStatus: pending\n\n## Purpose\nShip the plan.\n\n"
            b"## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n")


def canonical(value):
    # Exact scalar types, arbitrarily large ints, floats and signed zero matter.
    return json.dumps(value, sort_keys=True, ensure_ascii=True,
                      separators=(",", ":"), allow_nan=False).encode("ascii")


def snapshot(root):
    """Include root identity and every child; never follow links/read FIFOs.

    atime is deliberately excluded: command reads may legitimately update it.
    Our own byte reads use O_NOATIME and O_NOFOLLOW on the Linux test host.
    """
    result = {}

    def visit(path):
        info = path.lstat()
        identity = (info.st_mode, info.st_dev, info.st_ino, info.st_mtime_ns)
        key = os.fsencode(path.relative_to(root))
        if stat.S_ISLNK(info.st_mode):
            result[key] = (*identity, "link", os.fsencode(os.readlink(path)))
        elif stat.S_ISDIR(info.st_mode):
            result[key] = (*identity, "directory")
            for child in sorted(path.iterdir()):
                visit(child)
        elif stat.S_ISREG(info.st_mode):
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_NOATIME", 0))
            try:
                data = os.pread(fd, info.st_size, 0)
            finally:
                os.close(fd)
            result[key] = (*identity, "file", data)
        else:
            result[key] = (*identity, "special", info.st_rdev)
    visit(root)
    return result


def invoke(args, cwd, env, *, capture_permission_error=False):
    out, err = io.StringIO(), io.StringIO()
    with ExitStack() as stack:
        stack.enter_context(patch.dict(os.environ, env, clear=True))
        stack.enter_context(chdir(cwd))
        stack.enter_context(redirect_stdout(out))
        stack.enter_context(redirect_stderr(err))
        # Fail even if an unexpected provider call would otherwise be caught.
        for module, name in ((launch, "execute"), (launch, "stop"), (launch, "_transport"),
                             (subprocess, "run"), (subprocess, "Popen"), (os, "fsync"),
                             (storage, "atomic_replace_bytes"), (storage, "sync_directory")):
            stack.enter_context(patch.object(module, name, side_effect=AssertionError(name + " called")))
        try:
            status = coven.main(list(map(str, args)))
        except SystemExit as failure:
            status = failure.code
        except PermissionError:
            if not capture_permission_error:
                raise
            # Only the deliberately denied marker probe characterizes this
            # Python-version-dependent escape; do not normalize general I/O.
            status = "uncaught_permission_error"
    return status, out.getvalue(), err.getvalue()


@contextmanager
def changed(path, value, kind="file"):
    """Fixture mutation only, outside the read-only assertion window."""
    backup = path.with_name(path.name + ".fixture-backup")
    existed = path.exists() or path.is_symlink()
    if existed:
        path.rename(backup)
    try:
        if kind == "file":
            path.write_bytes(value)
        elif kind == "directory":
            path.mkdir()
        elif kind == "fifo":
            os.mkfifo(path)
        elif kind == "link":
            path.symlink_to(value)
        elif kind != "missing":
            raise AssertionError(kind)
        yield
    finally:
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        elif path.exists() or path.is_symlink():
            path.unlink()
        if existed:
            backup.rename(path)


def main(binary=None, oracle_only=False):
    binary = str(Path(binary).resolve()) if binary else None
    counts, names = Counter(), set()
    with tempfile.TemporaryDirectory(prefix="checkpoint-workspace-") as temporary:
        root = Path(temporary).resolve()
        env = {**os.environ, "COVEN_MULTIPLEXER": "", "HOME": str(root),
               "PATH": str(root / "no-tools"), "PYTHONPATH": str(root / "no-python"),
               "PYTHONDONTWRITEBYTECODE": "1"}

        def fixture(name):
            workspace = root / name
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                assert coven.main(["init", str(workspace), "--blank", "--agents", "operator"]) == 0
            return workspace

        w = fixture("workspace with spaces 雪")
        cp = w / "checkpoints/plan-approval.md"
        cp.write_bytes(TEMPLATE)
        outside = root / "outside"
        outside.mkdir()
        (outside / "sentinel").write_bytes(b"external bytes\xff")

        def check(name, operands=None, *, cwd=None, ok=True, code=None, profile=None,
                  environment=None, command=None, python_ok=None, parser=False,
                  python_error=None):
            assert name not in names, name
            names.add(name)
            args = command if command is not None else ["checkpoint", "inspect",
                *(map(str, operands) if operands is not None else [str(w)]), "--json"]
            current_env = {**env, **(environment or {})}
            current_env = {key: value for key, value in current_env.items() if value is not None}
            before = snapshot(root)
            expected = invoke(args, cwd or w, current_env)
            expected_ok = ok if python_ok is None else python_ok
            if python_ok is not None:
                assert (expected[0] == 0) == python_ok, (name, expected)
            if python_error is not None:
                assert python_error in expected[2], (name, expected)
            if profile:
                # Profiles can be Python-valid or Python-invalid; record truth,
                # but never count a profile refusal as semantic parity.
                counts["profile_python_success" if expected[0] == 0 else "profile_python_error"] += 1
            else:
                assert (expected[0] == 0) == expected_ok, (name, expected)
            if expected[0] != 0:
                assert expected[1] == "", (name, "Python fatal stdout", expected)
            assert snapshot(root) == before, (name, "Python modified fixture namespace")
            if not oracle_only:
                result = subprocess.run([binary, *args], cwd=cwd or w, env=current_env,
                                        capture_output=True, timeout=5)
                if not ok or profile:
                    assert result.returncode != 0 and result.stdout == b"", (name, result)
                    if code or profile:
                        assert result.stderr == f"[ERROR] {code or profile}\n".encode(), (name, result)
                    elif parser:
                        assert result.returncode == 2 and b"error:" in result.stderr, (name, result)
                else:
                    assert result.returncode == 0 and result.stderr == b"", (name, result)
                    assert canonical(json.loads(result.stdout)) == canonical(json.loads(expected[1])), (name, expected, result)
                assert snapshot(root) == before, (name, "native modified fixture namespace")
            counts["profile_refusals" if profile else "parser_refusals" if parser else
                   "parity_success" if ok else "parity_fatal"] += 1

        # Marker-based selection, never positional arity or ancestor search.
        for name, operands in (("default-current", []), ("default-explicit", [w]),
                               ("id-only", ["plan-approval"]), ("both", [w, "plan-approval"]),
                               ("missing-view", [w, "missing"]), ("empty-workspace", [""]),
                               ("relative", ["."]), ("parent-relative", ["../" + w.name])):
            check(name, operands)
        alias = root / "root-alias"
        alias.symlink_to(w, target_is_directory=True)
        check("root-alias", [alias])
        check("no-ancestor-search", [], cwd=w / "work", ok=False)
        check("unmarked-one-is-id", ["unmarked"])
        check("unmarked-two-is-excess", ["unmarked", "plan-approval"], ok=False,
              code="invalid_checkpoint_operands")
        check("excess-marked", [w, "plan-approval", "extra"], ok=False,
              code="invalid_checkpoint_operands")
        nested = fixture(str(w.relative_to(root) / "plan-approval"))
        (nested / "checkpoints/plan-approval.md").write_bytes(b"Status: nested\n")
        check("id-spelling-selects-subworkspace", ["plan-approval"])
        shutil.rmtree(nested)
        selection_cwd = fixture("selection-cwd")
        for kind, target in (("directory", None), ("link", outside / "sentinel"),
                             ("link", outside / "absent"), ("fifo", None)):
            name = "marker-" + kind + ("-dangling" if target and target.name == "absent" else "")
            with changed(w / "coven.json", target, kind):
                check(name, [w, "../bad"], cwd=selection_cwd, ok=False, code="unsafe_archive_path")
        check("hyphen-id", command=["checkpoint", "inspect", "--json", "--", "-Legacy"])
        check("terminator-options-are-operands", command=["checkpoint", "inspect", "--", "--json"],
              ok=False, parser=True, python_ok=True)
        check("operand-terminator-does-not-activate-json",
              command=["checkpoint", "inspect", str(w), "--", "--json"],
              ok=False, parser=True, python_ok=True)
        check("leading-json-retains-terminator-in-operand-group",
              command=["checkpoint", "inspect", "--json", str(w), "--", "--json"])
        check("option-placement-leading", command=["checkpoint", "inspect", "--json", str(w), "plan-approval"])
        check("option-placement-between-operands", command=["checkpoint", "inspect", str(w), "--json", "plan-approval"],
              ok=False, parser=True)
        for flag in ("--j", "--js", "--jso"):
            check("abbreviated-json-leading-" + flag,
                  command=["checkpoint", "inspect", flag, str(w), "plan-approval"])
            check("abbreviated-json-trailing-" + flag,
                  command=["checkpoint", "inspect", str(w), "plan-approval", flag])
        with changed(selection_cwd / "coven.toml", b"invalid TOML!"):
            for flag in ("--json", "--j", "--js", "--jso"):
                check("trailing-terminator-parser-before-config-" + flag, cwd=selection_cwd,
                      command=["checkpoint", "inspect", str(selection_cwd), flag, "--", "--json"],
                      ok=False, parser=True, python_error="unrecognized arguments")
            for flag in ("--j", "--js", "--jso"):
                check("abbreviated-json-retains-preflight-priority-" + flag, cwd=selection_cwd,
                      command=["checkpoint", "inspect", str(selection_cwd), "../bad", flag],
                      ok=False, code="invalid_workspace_toml", python_error="Invalid workspace configuration")
        for name, args in (("unknown-option", ["checkpoint", "inspect", "--unknown", "--json"]),
                           ("hyphen-without-terminator", ["checkpoint", "inspect", "-bad", "--json"]),
                           ("required-json", ["checkpoint", "inspect", str(w)]),
                           ("repeat-json", ["checkpoint", "inspect", str(w), "--json", "--json"]),
                           ("unsupported-approve", ["checkpoint", "approve", str(w), "--json"]),
                           ("unsupported-mutation", ["approve", str(w), "--json"])):
            if name == "unsupported-mutation":
                # Do not run a Python mutation merely to verify native refusal.
                before = snapshot(root)
                if not oracle_only:
                    result = subprocess.run([binary, *args], cwd=w, env=env, capture_output=True, timeout=5)
                    assert result.returncode == 2 and result.stdout == b"", result
                assert snapshot(root) == before
                counts["parser_refusals"] += 1
            else:
                check(name, command=args, ok=False, parser=True,
                      python_ok=name in ("required-json", "repeat-json"))
        for identifier in ("", "../bad", "bad.id", "雪", "space id"):
            check("invalid-id-" + repr(identifier), [w, identifier], ok=False,
                  code="invalid_checkpoint_identifier")
        for identifier in ("_", "0", "Case_ID", "--"):
            check("legacy-id-" + identifier, command=["checkpoint", "inspect", "--json", "--", str(w), identifier])
        home = root / "home"
        home.mkdir()
        real = fixture("home/real")
        decoy = fixture(str(w.relative_to(root) / "~/real"))
        (decoy / "checkpoints/plan-approval.md").write_bytes(b"Status: wrong\n")
        check("home-expansion", ["~/real"], environment={"HOME": str(home)})
        check("bare-home", ["~"], environment={"HOME": str(real)})
        check("named-user", ["~root"], profile="unsupported_named_user_expansion")
        check("missing-home", ["~/real"], environment={"HOME": None},
              profile="home_expansion_unavailable")
        check("literal-tilde-not-leading", [decoy])

        # Every known managed path: wrong type and symlink. Special files are
        # additionally covered at marker, checkpoint, projection and lock paths.
        for relative in (*coven.MANAGED_DIRECTORIES, *coven.MANAGED_FILES):
            selected = w / relative
            for kind, value in (("file" if relative in coven.MANAGED_DIRECTORIES else "directory", b"x"),
                                ("link", outside / "absent")):
                with changed(selected, value, kind):
                    check(f"managed-{relative}-{kind}-{value!r}", ok=False, code="unsafe_archive_path")
        for relative in ("state/.workspace.lock", "state/.runtime-transport.lock"):
            with changed(w / relative, None, "fifo"):
                check("fifo-lock-" + relative, ok=False, code="unsafe_archive_path")
        for relative in ("agents/operator.md", "prompts/operator.md", "prompts/operator.start.txt"):
            for kind in ("directory", "link", "fifo"):
                with changed(w / relative, outside / "sentinel", kind):
                    check(f"roster-{relative}-{kind}", ok=False, code="unsafe_archive_path")
        for relative in ("goal.md", "spec.md", "dashboard.md", "logs/events.jsonl", "logs/messages.jsonl",
                         "tmux/coven.toml", "work/README.md", "output/README.md",
                         "agents/operator.md", "prompts/operator.md", "prompts/operator.start.txt"):
            with changed(w / relative, b"\xff\x00corrupt prose"):
                check("ignored-content-" + relative)
        for suffix in ("md", "MD", "mD"):
            with changed(w / f"checkpoints/not.valid.{suffix}", b"text"):
                check("stem-" + suffix, ok=False)
        for filename, valid in (("雪.MD", False), ("雪.ignored", True),
                                (".md", True), (".hidden.md", False)):
            with changed(w / "checkpoints" / filename, b"text" if not valid else b"\xff"):
                check("filename-edge-" + filename, ok=valid,
                      code=None if valid else "invalid_checkpoint_id")
        with changed(w / "checkpoints" / os.fsdecode(b"ignored-\xff.bin"), b"\xff"):
            check("non-utf8-unrecognized-filename", profile="non_unicode_path_unsupported", python_ok=True)
        for name, raw, valid in (("unselected-grammar", b"<!-- coven-checkpoint-bad:v99\n", True),
                                 ("unselected-encoding", b"\xff", False)):
            with changed(w / "checkpoints/Other.MD", raw):
                check(name, ok=valid, code=None if valid else "invalid_utf8")
                if valid:
                    check("uppercase-extension-not-lowercase-selected-file", [w, "Other"])
        for kind in ("directory", "link", "fifo"):
            with changed(w / "checkpoints/Other.MD", outside / "sentinel", kind):
                check("unselected-md-type-" + kind, ok=False, code="unsafe_archive_path")
        with changed(w / "checkpoints/ignored.txt", outside / "absent", "link"), \
                changed(w / "checkpoints/ignored.bin", None, "fifo"), \
                changed(w / "checkpoints/child", None, "directory"):
            (w / "checkpoints/child/invalid.name.MD").write_bytes(b"\xff")
            check("non-md-and-children-ignored")
        # Mandatory files precede sorted directory entries; sorted entries, not
        # creation order, decide the first diagnostic.
        with changed(w / "checkpoints/z-invalid.MD", b"\xff"), \
                changed(w / "checkpoints/a.bad.MD", b"ok"):
            check("sorted-stem-before-encoding", ok=False, code="invalid_checkpoint_id")
            with changed(cp, b"\xff"):
                check("mandatory-before-scanned-stem", ok=False, code="invalid_utf8")
        with changed(w / "checkpoints/z.bad.MD", b"text"), \
                changed(w / "checkpoints/a-valid.MD", b"\xff"):
            check("sorted-encoding-before-stem", ok=False, code="invalid_utf8")

        # Selected pure views: representative states only, not the 2750-case
        # grammar corpus. Inspection returns zero even for stale/inconsistent.
        bound, _ = checkpoints.prepare("plan-approval", TEMPLATE, "approved", "review 雪",
            decision_id=A, decided_at="not a timestamp", source="cli")
        rejected, _ = checkpoints.prepare("plan-approval", bound, "changes_requested", "again",
            decision_id=B, decided_at="later", source="review")
        views = {"empty": b"", "unbound": TEMPLATE.replace(b"pending", b"approved"),
                 "bound": bound, "history-rejected": rejected,
                 "stale": bound + b"\nChanged scope\n",
                 "inconsistent": bound.replace(b"Status: approved", b"Status: pending")}
        for name, raw in views.items():
            for ending in (b"\n", b"\r\n", b"\r"):
                with changed(cp, raw.replace(b"\n", ending)):
                    check(f"view-{name}-{ending.hex()}",
                          ok=ending != b"\r" or name in ("empty", "unbound"))
        with changed(cp, None, "missing"):
            check("default-missing-view")
        with changed(cp, b"<!-- coven-checkpoint-invalid:v99\n"):
            check("selected-grammar-fatal", ok=False, code="invalid_checkpoint_reserved_machinery")

        # Configuration/records are structural, not lifecycle reconciliation.
        team = json.loads((w / "coven.json").read_bytes())
        for name, value, code in (("null", None, "invalid_team_record"),
                ("schema", {**team, "version": True}, "unsupported_workspace_version"),
                ("aliases", {**team, "agents": ["A", "a"]}, "ambiguous_team_alias"),
                ("empty-roster", {**team, "agents": []}, "invalid_team_roster"),
                ("persisted-runtime", {**team, "multiplexer": "bad"}, "unsupported_multiplexer")):
            with changed(w / "coven.json", json.dumps(value).encode()):
                check("team-" + name, ok=False, code=code, environment={"COVEN_MULTIPLEXER": "tmux"})
                check("team-before-operands-" + name, [w, "bad", "excess"], ok=False, code=code)
        with changed(w / "coven.json", json.dumps({**team, "multiplexer": "bad"}).encode()), \
                changed(w / "coven.toml", b'[runtime]\nmultiplexer="tmux"\n'):
            check("persisted-invalid-cannot-be-rescued-by-toml", ok=False, code="unsupported_multiplexer")
        tomls = (("syntax", b"broken TOML!", False, "invalid_workspace_toml"),
                 ("table", b"runtime=1\n", False, "invalid_runtime_configuration"),
                 ("command-type", b"[runtime]\nopencode_command=1\n", False, "invalid_runtime_configuration"),
                 ("blank-command", b'[runtime]\nopencode_command="  "\n', False, "invalid_runtime_configuration"),
                 ("bad-mux", b'[runtime]\nmultiplexer="bad"\n', False, "unsupported_multiplexer"),
                 ("two-overrides", b'[runtime]\nmultiplexer=" TMUX "\nopencode_command=" no-such-runtime "\n', True, None),
                 ("unrelated", b"[unrelated]\nvalue=123\n", True, None))
        for name, raw, valid, code in tomls:
            with changed(w / "coven.toml", raw):
                check("toml-" + name, ok=valid, code=code)
                if not valid:
                    check("toml-before-invalid-id-" + name, [w, "../bad"], ok=False, code=code)
                if name == "syntax":
                    check("cwd-config-before-unmarked-excess", ["unmarked", "plan-approval"],
                          ok=False, code=code)
                if name == "bad-mux":
                    check("effective-team-before-environment", ok=False, code=code,
                          environment={"COVEN_MULTIPLEXER": "tmux"})
        for name, literal in (("ignored-large-int", "9223372036854775808"),
                              ("ignored-overflow-float", "1e999")):
            with changed(w / "coven.toml", f"x={literal}\n".encode()):
                check("toml-profile-" + name, profile="unsupported_checkpoint_toml_scalar_profile", python_ok=True)
        for name, literal, valid in (
                ("leap-second", "2026-10-31T23:59:60Z", False),
                ("year-zero", "0000-01-01", False),
                ("leap-day", "2024-02-29", True),
                ("non-leap-day", "2026-02-29", False),
                ("bad-calendar", "2026-04-31", False),
                ("bad-hour", "2026-10-31T24:00:00Z", False),
                ("valid-offset", "2026-10-31T23:59:59+23:59", True),
                ("bad-offset", "2026-10-31T23:59:59+24:00", False)):
            with changed(w / "coven.toml", f"x={literal}\n".encode()):
                check("toml-date-" + name, ok=valid,
                      code=None if valid else "invalid_workspace_toml")
        check("environment-invalid", ok=False, code="unsupported_multiplexer", environment={"COVEN_MULTIPLEXER": "bad"})
        check("environment-normalized", environment={"COVEN_MULTIPLEXER": " TMUX "})
        extension = {"large": 10**100, "float": 1.0, "zero": -0.0, "unicode": "雪", "nested": [True, None]}
        valid_records = {
            "state/agents.json": [{"id": "outsider", "status": "legacy", "role": "", "ext": extension}],
            "state/tasks.json": [{"id": "_", "title": "", "status": "legacy", "owner": "outsider", "ext": extension}],
            "state/lead.json": {"agent": "outsider", "until": "not a date", "ext": extension},
            "coven.json": {**team, "display_names": {"outsider": "Label"}, "ext": extension}}
        for relative, value in valid_records.items():
            with changed(w / relative, json.dumps(value).encode()):
                check("legacy-extensions-" + relative)
        for relative in ("coven.toml", "state/agents.json", "state/tasks.json", "state/lead.json"):
            with changed(w / relative, None, "missing"):
                check("optional-record-absent-" + relative)
        for relative, raw in (("coven.json", b'{"name":null,"name":"valid","agents":["operator"]}'),
                              ("state/tasks.json", b'[{"id":"old","id":"new","title":"","status":""}]'),
                              ("state/agents.json", b'[{"id":"old","id":"new","role":"","status":""}]'),
                              ("state/lead.json", b'{"agent":false,"agent":"outsider"}')):
            with changed(w / relative, raw):
                check("duplicate-last-wins-" + relative)
        for relative, raw in (("state/agents.json", b"null"), ("state/lead.json", b"{}"),
                              ("state/tasks.json", b'[ {"id":"x","title":true,"status":"x"}]')):
            with changed(w / relative, raw):
                check("invalid-record-" + relative, ok=False)
                check("invalid-record-before-excess-" + relative, [w, "bad", "extra"], ok=False)

        # Pending ledger evidence is NOT authority for this wrapper. task list
        # retains its conservative fence; active authority bypasses projection.
        with changed(w / "state/task-ledger", None, "directory"):
            (w / "state/task-ledger/arbitrary").write_bytes(b"not a ledger")
            check("pending-ledger-snapshot-allowed")
            check("task-list-pending-fence", command=["task", "list", str(w), "--json"],
                  ok=False, python_ok=True, code="unsupported_native_authority")
        ledger = fixture("active-ledger")
        store = task_ledger.LedgerStore(ledger)
        with storage.workspace_lock(ledger, ledger / "state/.workspace.lock") as lock:
            view = store.import_tasks(storage.snapshot_revision((ledger / "state/tasks.json").read_bytes()), lock)
            binding = task_ledger.request_binding("add", {"title": "Added", "notes": "opaque", "owner": None}, view.revision)
            view = store.commit(view, "wrapper-add", binding, {
                "id": "added", "title": "Added", "notes": "opaque", "owner": None,
                "status": "pending", "created_at": "frozen", "updated_at": "frozen"}, lock)
            store.publish_projection(view, lock)
        check("active-ledger-full-replay", [ledger])
        for name, kind, raw in (("fifo", "fifo", None), ("directory", "directory", None),
                                ("symlink", "link", outside / "sentinel"), ("missing", "missing", None),
                                ("bad-bytes", "file", b"\xff"), ("diverged", "file", b"[]"),
                                ("large", "file", b" " * (8 * MIB + 1))):
            with changed(ledger / "state/tasks.json", raw, kind):
                check("active-projection-ignored-" + name, [ledger])
        with changed(ledger / task_ledger.MARKER, b"{}"):
            check("corrupt-active-no-fallback", [ledger], ok=False)
        frame = next((ledger / task_ledger.LEDGER).glob("*/frames/00000000000000000001.frame"))
        with changed(frame, b"corrupt"):
            check("corrupt-active-frame-no-fallback", [ledger], ok=False)

        # Runtime controls are made at their real roots and bind real inodes.
        runtime = fixture("runtime-control")
        action_store = actions.ActionStore(runtime)
        with ExitStack() as locks:
            gate = locks.enter_context(storage.transport_gate(runtime, runtime / "state/.runtime-transport.lock", exclusive=True))
            lock = locks.enter_context(storage.workspace_lock(runtime, runtime / "state/.workspace.lock"))
            control = action_store.enable(lock, gate=gate)
            check("enabled-held-locks-not-acquired", [runtime])
            runtime_alias = root / "runtime-alias"
            runtime_alias.symlink_to(runtime, target_is_directory=True)
            check("runtime-location-through-root-alias", [runtime_alias])
            plan = launch.build_plan(str(runtime), f"coven-{control['workspace_id']}-{A}", "tmux",
                [{"id": "operator", "command": "never execute", "starter": "start"}],
                {"PATH": "/frozen", "HOME": "/frozen", "LANG": "", "TERM": "xterm"})
            inputs = {"fixture": True}
            bundle = action_store.make_bundle(inputs, plan, A, lock)
            action_store.publish_bundle(bundle, lock)
            checkpoint_id = actions.checkpoint_id(bundle)
            raw, view = checkpoints.prepare(checkpoint_id, actions.checkpoint_bytes(bundle), "approved", "review",
                decision_id=B, decided_at="frozen", source="cli")
            (runtime / f"checkpoints/{checkpoint_id}.md").write_bytes(raw)
            claim, consumed = action_store.claim(A, actions.digest(bundle), view.revision,
                view.decision["decision_id"], inputs, lock)
            assert not consumed
            check("valid-claimed-head", [runtime])
            for kind, value in (("file", b"corrupt"), ("link", outside / "absent"),
                                ("fifo", None), ("directory", None)):
                with changed(runtime / actions.BUNDLES / f"{A}.json", value, kind):
                    check("bundle-children-ignored-" + kind, [runtime])
            action_store.record_outcome(A, claim["claim_id"], {
                "operation": "runtime_launch", "status": "transport_completed", "completed_steps": 0,
                "handles": {}, "outcome_unknown": False, "diagnostics": []}, lock)
            check("valid-terminal-outcome", [runtime])
        good = action_store.read_control()
        control_path = runtime / actions.CONTROL
        for name, raw, code in (("null", b"null", "invalid_action_fields"),
                                 ("bad-json", b"{", "invalid_compat_json")):
            with changed(control_path, raw):
                check("control-" + name, [runtime], ok=False, code=code)
                if name == "null":
                    check("control-before-invalid-checkpoint", [runtime, "../bad"], ok=False, code=code)
        for name, transform, code in (
                ("location", lambda c: c["location"].update(root=str(w)), "action_workspace_location_changed"),
                ("unselected-claim", lambda c: c["claims"][A].update(status="invalid"), "invalid_action_claim_status"),
                ("head-missing", lambda c: c.update(head=B), "invalid_action_head")):
            value = deepcopy(good)
            transform(value)
            with changed(control_path, canonical(value)):
                check("control-" + name, [runtime], ok=False, code=code)
        with changed(control_path, None, "missing"):
            check("orphan-actions", [runtime], ok=False, code="action_orphan_bundles")
        with changed(control_path, json.dumps(good).encode("utf-16")):
            check("control-encoding", [runtime], profile="unsupported_action_encoding_profile")
        for relative in ("state", "state/actions", "work"):
            with changed(runtime / relative, outside, "link"):
                check("runtime-unsafe-" + relative, [runtime], ok=False, code="unsafe_archive_path")
        check("task-list-runtime-fence", command=["task", "list", str(runtime), "--json"],
              ok=False, python_ok=True, code="unsupported_native_authority")
        with changed(runtime / "coven.json", b"{}"):
            check("checkpoint-still-checks-config", [runtime], ok=False, code="missing_workspace_record_field")
            check("action-still-ignores-config", command=["action", "inspect", str(runtime), A, "--json"])
        for relative in (task_attempts.DIR,):
            for kind, raw in (("file", b"null"), ("file", b"corrupt"), ("fifo", None),
                              ("link", outside / "absent")):
                with changed(w / relative, raw, kind):
                    check(f"execution-ignored-{relative}-{kind}-{raw!r}")
        with changed(w / task_attempts.DIR, None, "directory"):
            for kind, raw in (("file", b"null"), ("file", b"corrupt"), ("fifo", None),
                              ("link", outside / "absent")):
                with changed(w / task_attempts.CONTROL, raw, kind):
                    check(f"execution-control-ignored-{kind}-{raw!r}")

        # Explicit finite/raw-UTF8/resource profiles, not Python invalidity.
        for relative in ("coven.json", "state/tasks.json"):
            raw = (json.dumps(team).encode() if relative == "coven.json" else b"[]")
            with changed(w / relative, raw + b" " * (8 * MIB + 1)):
                check("raw-size-" + relative, profile="unsupported_checkpoint_workspace_input_profile")
        for relative in ("coven.toml", "checkpoints/Other.MD", "checkpoints/plan-approval.md"):
            raw = b"#" + b" " * MIB if relative.endswith("toml") else b" " * (MIB + 1)
            with changed(w / relative, raw):
                check("text-size-" + relative, profile="unsupported_checkpoint_workspace_input_profile")
        for raw, code in ((b'[{"id":"t","title":"","status":"","x":NaN}]', "unsupported_nonfinite_json"),
                          (b'[{"id":"t","title":"","status":"","x":"\\ud800"}]', "unsupported_or_invalid_json_scalar"),
                          (b'[{"id":"t","title":"","status":"","x":1e999}]', "unsupported_nonfinite_json")):
            with changed(w / "state/tasks.json", raw):
                check("finite-profile-" + code + "-" + str(len(raw)), profile=code)
        nonunicode = root / os.fsdecode(b"workspace-\xff")
        shutil.copytree(w, nonunicode)
        check("non-unicode-workspace", [nonunicode], profile="non_unicode_path_unsupported")
        # Keep permission setup separate from check(): while the parent is mode
        # zero a whole-tree snapshot cannot traverse it as an ordinary user.
        # Compare the complete namespace after restoring exactly the old mode.
        denied_parent = root / "denied-parent"
        denied_parent.mkdir()
        candidate = fixture("denied-parent/candidate")
        permission_cwd = fixture("permission-cwd")
        with changed(permission_cwd / "coven.toml", b"invalid TOML!"):
            before = snapshot(root)
            mode = stat.S_IMODE(denied_parent.stat().st_mode)
            try:
                denied_parent.chmod(0)
                try:
                    (candidate / "coven.json").stat()
                except PermissionError:
                    args = ["checkpoint", "inspect", str(candidate), "extra", "--json"]
                    expected = invoke(args, permission_cwd, env, capture_permission_error=True)
                    assert expected[1] == "", expected
                    if expected[0] == "uncaught_permission_error":
                        assert expected[2] == "", expected
                        counts["permission_python_uncaught"] += 1
                    else:
                        assert expected[0] == 1 and "Invalid workspace configuration" in expected[2], expected
                        counts["permission_python_cwd_selected"] += 1
                    # Native's exists/is_symlink probes suppress EACCES: it
                    # must select cwd and fail its TOML before excess operands,
                    # not consume candidate or report a generic storage error.
                    if not oracle_only:
                        result = subprocess.run([binary, *args], cwd=permission_cwd, env=env,
                                                capture_output=True, timeout=5)
                        assert result.returncode == 1 and result.stdout == b"", result
                        assert result.stderr == b"[ERROR] invalid_workspace_toml\n", result
                    names.add("permission-denied-marker-cwd-selection")
                    counts["permission_probe_cases"] += 1
                else:
                    counts["permission_probe_skipped_privileged_access"] += 1
            finally:
                denied_parent.chmod(mode)
            assert snapshot(root) == before, "permission probe changed fixture namespace"
        # Mandatory fixtures already consume two entries; 4096/4097 semantics
        # and changing-authority races are protected in core owner unit tests.
    print(json.dumps({"cases": len(names) + 1, "oracle_only": oracle_only,
                      "counts": dict(sorted(counts.items()))}, sort_keys=True))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    arguments = parser.parse_args()
    if sys.platform != "linux":
        parser.error("this filesystem/permission oracle is Linux-only")
    if not arguments.oracle_only and not arguments.binary:
        parser.error("--binary is required unless --oracle-only")
    main(arguments.binary, arguments.oracle_only)
