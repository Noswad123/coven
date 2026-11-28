#!/usr/bin/env python3
"""Capture SLICE C from an explicit a249 Git export, never the active runner.

Only metadata providers are controlled: uuid.uuid4 (shared by runner modules),
and coven/task_attempts/execution_v2.datetime.now. No ledger/execution logic is
patched. Original embedded caller scripts are extracted verbatim before exec.
"""
import argparse
import contextlib
import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import uuid

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
CALLERS = ["src/ledger/activation.rs", "src/ledger/confirmation.rs",
           "src/ledger/repair/tests.rs", "src/ledger/writer.rs"]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=True, indent=2) + "\n").encode()


def capture(root):
    sys.path.insert(0, str(root))
    sys.path.insert(0, str(root / "tests"))
    from runner import coven, task_attempts, execution_v2, task_ledger
    counter = 0

    def identity():
        nonlocal counter
        counter += 1
        return uuid.UUID(int=counter)

    class Clock(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return cls(2026, 1, 2, 3, 4, 5, tzinfo=datetime.timezone.utc).astimezone(tz)

    uuid.uuid4 = identity
    for module in (coven, task_attempts, execution_v2):
        module.datetime = Clock
    scripts = {}
    for path in CALLERS:
        text = (root / path).read_text()
        for script in re.findall(r'r#"(.*?)"#', text, re.S):
            if "from _authority_confirmation import execution_call" in script:
                scripts["execution"] = script
            elif "for item in (frame, raw," in script:
                scripts["plan"] = script
            elif "invoke('task', 'add', root, f'Task {i}'" in script:
                scripts["confirmation"] = script
            elif "Secret task title" in script and "from runner" in script:
                scripts["repair"] = script
            elif "mutation_id='d'*32" in script:
                scripts["writer"] = script
    assert set(scripts) == {"execution", "plan", "confirmation", "repair", "writer"}
    rows = []

    def inventory(workspace):
        files, directories = [], []
        if workspace is not None:
            for path in sorted(workspace.rglob("*")):
                relative = path.relative_to(workspace).as_posix()
                assert not path.is_symlink(), relative
                if path.is_dir():
                    directories.append(dict(path=relative, mode=path.stat().st_mode & 0o7777))
                else:
                    raw = path.read_bytes()
                    files.append(dict(path=relative, mode=path.stat().st_mode & 0o7777,
                                      hex=raw.hex(), sha256=sha(raw)))
        return dict(files=files, directories=directories)

    def invoke(kind, args, workspace=None, label=None):
        out, err = io.StringIO(), io.StringIO()
        before = inventory(workspace)
        sys.argv = ["historical-caller", *map(str, args)]
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            exec(compile(scripts[kind], f"historical-{kind}", "exec"), {})
        after = inventory(workspace)
        row = dict(id=label or kind, script=kind,
                   input_args=list(map(str, args)),
                   stdout=out.getvalue(), stderr=err.getvalue(), exit_status=0,
                   input_inventory=before, **after)
        row["stdout_sha256"] = sha(row["stdout"].encode())
        row["stderr_sha256"] = sha(row["stderr"].encode())
        row["row_sha256"] = sha(encoded(row))
        rows.append(row)
        return row

    # RAW comes from the original native declaration, including JSON escapes.
    activation = (root / CALLERS[0]).read_text()
    literal = re.search(r'const RAW: &\[u8\] = b(".*");', activation)[1]
    raw = json.loads(literal).encode()
    with tempfile.TemporaryDirectory(prefix="source-ledger-capture-") as temporary:
        base = Path(temporary)
        for index, source in enumerate((None, b" [ ] \r\n", raw)):
            invoke("plan", [(source or b"").hex(), str(source is not None).lower()], label=f"plan-{index}")
        for present in (False, True):
            for sequence in (0, 3):
                workspace = base / f"confirmation-{present}-{sequence}"
                workspace.mkdir()
                invoke("confirmation", [workspace, str(present).lower(), sequence], workspace,
                       f"confirmation-{str(present).lower()}-{sequence}")
        workspace = base / "repair"
        workspace.mkdir()
        invoke("repair", [workspace], workspace)
        for state in ("prepared", "stale", "revoked", "clear"):
            workspace = base / state
            (workspace / "state").mkdir(parents=True)
            (workspace / "coven.json").write_bytes(b"not parsed by activation")
            (workspace / "state/tasks.json").write_bytes(raw)
            invoke("execution", [workspace, state], workspace, f"execution-{state}")
        workspace = base / "writer"
        (workspace / "state").mkdir(parents=True)
        (workspace / "coven.json").write_bytes(b"marker")
        writer_raw = b' [ { "title":"Old", "id":"old", "status":"pending", "extension":{"z":1,"a":2} } ]\n'
        (workspace / "state/tasks.json").write_bytes(writer_raw)
        from runner import storage
        with storage.workspace_lock(workspace, workspace / "state/.workspace.lock") as held:
            view = task_ledger.LedgerStore(workspace).import_tasks(storage.snapshot_revision(writer_raw), held)
        binding = task_ledger.request_binding("add", dict(title="Task", owner=None, notes=""), view.revision)
        task_literal = re.search(r'r#"(\{"id":"task".*?)"#', (root / CALLERS[3]).read_text(), re.S)[1]
        invoke("writer", [workspace, json.dumps(binding), task_literal], workspace)
    return dict(schema_version=1, scripts=scripts, rows=rows)


def check(project):
    raw = (project / "tests/retirement/fixtures/source_ledger.json").read_bytes()
    corpus = json.loads(raw)
    provenance = json.loads((project / "tests/retirement/fixtures/source_ledger_provenance.json").read_bytes())
    assert sha(raw) == provenance["corpus_sha256"]
    assert sha(Path(__file__).read_bytes()) == provenance["capture_tool_sha256"]
    assert provenance["historical_commit"] == PIN
    pins = provenance["source_sha256"]
    assert sha(encoded(pins)) == provenance["source_inventory_sha256"]
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                    "runner", "tests", "src", "examples"], cwd=project).decode().splitlines()
    assert set(pins) == {p for p in paths if "__pycache__" not in p and not p.endswith(".pyc")}
    for path, digest in pins.items():
        assert sha(subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)) == digest, path
    assert sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    expected = [f"plan-{i}" for i in range(3)] + [f"confirmation-{str(p).lower()}-{s}" for p in (False, True) for s in (0, 3)] + ["repair"] + [f"execution-{s}" for s in ("prepared", "stale", "revoked", "clear")] + ["writer"]
    assert [row["id"] for row in corpus["rows"]] == expected
    files = 0
    declarations = {}
    for path in CALLERS:
        original = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project).decode()
        current = (project / path).read_text()
        pattern = r'#\[test\]\s*fn (\w+)\('
        tests = re.findall(pattern, original)
        assert tests == re.findall(pattern, current), path
        assert 'Command::new(' not in current and 'from runner' not in current, path
        if path != "src/ledger/repair/tests.rs":
            assert original.split("#[cfg(", 1)[0] == current.split("#[cfg(", 1)[0], path
        declarations[path] = len(tests)
    for row in corpus["rows"]:
        content = {key: value for key, value in row.items() if key != "row_sha256"}
        assert sha(encoded(content)) == row["row_sha256"]
        assert row["exit_status"] == 0 and row["stderr"] == ""
        for stream in ("stdout", "stderr"):
            assert sha(row[stream].encode()) == row[stream + "_sha256"]
        for inventory in (row, row["input_inventory"]):
            names = [entry["path"] for key in ("files", "directories") for entry in inventory[key]]
            assert len(set(names)) == len(names)
            assert all(not Path(p).is_absolute() and all(c not in (".", "..") for c in Path(p).parts) for p in names)
            for file in inventory["files"]:
                assert sha(bytes.fromhex(file["hex"])) == file["sha256"]
                files += 1
        if row["script"] == "plan":
            frame, manifest, marker = [bytes.fromhex(line) for line in row["stdout"].splitlines()]
            assert json.loads(manifest)["baseline_digest"] == sha(frame)
            assert json.loads(marker)["manifest_digest"] == sha(manifest)
    print(json.dumps(dict(rows=len(expected), source_files=len(pins), runner_modules=25,
                          retained_test_declarations=declarations,
                          file_stream_hashes=files, output_stream_hashes=2 * len(expected),
                          corpus_sha256=sha(raw), provenance_sha256=sha((project / "tests/retirement/fixtures/source_ledger_provenance.json").read_bytes()))))


def main():
    def interrupted(signum, _frame):
        raise SystemExit(128 + signum)
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(signum, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", type=Path)
    parser.add_argument("--check", action="store_true", help="Verify frozen hashes and strict inventory without executing historical Python")
    parser.add_argument("--capture", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    if args.check:
        check(project)
        return
    assert args.historical_root is not None, "--historical-root is required for capture"
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    if args.capture:
        print(json.dumps(capture(root)))
        return
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                    "runner", "tests", "src", "examples"], cwd=project).decode().splitlines()
    pins = {}
    for path in paths:
        if "__pycache__" in path or path.endswith(".pyc"):
            continue
        original = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == original, path
        pins[path] = sha(original)
    assert sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    assert all(p in pins for p in CALLERS + ["tests/_authority_confirmation.py"])
    assert not list(root.rglob("*.pyc"))
    for directory in ("runner", "tests", "src", "examples"):
        actual = {p.relative_to(root).as_posix() for p in (root / directory).rglob("*") if p.is_file()}
        assert actual == {p for p in pins if p.startswith(directory + "/")}, directory
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "COVEN_MULTIPLEXER": "", "TMPDIR": os.environ["TMPDIR"]}
    child = subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()),
                              "--historical-root", str(root), "--capture"], cwd=root,
                             env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    try:
        out, err = child.communicate(timeout=120)
        assert child.returncode == 0, err.decode()
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
        child.wait()
    corpus = json.loads(out)
    destination = project / "tests/retirement/fixtures/source_ledger.json"
    destination.write_bytes(encoded(corpus))
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      source_inventory_sha256=sha(encoded(pins)), corpus_sha256=sha(destination.read_bytes()),
                      capture_tool_sha256=sha(Path(__file__).read_bytes()),
                      interpreter=dict(version=sys.version, sha256=sha(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      environment=env, child_stdout_sha256=sha(out), child_stderr_hex=err.hex(),
                      metadata_patches=["uuid.uuid4: monotonic UUID(int=n)",
                                        "runner.coven.datetime", "runner.task_attempts.datetime", "runner.execution_v2.datetime"],
                      clock="2026-01-02T03:04:05+00:00", rows=len(corpus["rows"]),
                      proof="Full original scripts, input arguments, raw file bytes/modes, stdout/stderr, per-stream/file/row SHA256. No native outputs or encoders used as expectations. Execution overlay selects only captured state/execution paths; stale source edit remains native. No path/UUID normalization of expected bytes.")
    (destination.parent / "source_ledger_provenance.json").write_bytes(encoded(provenance))
    print(json.dumps(dict(rows=len(corpus["rows"]), source_files=len(pins), corpus_sha256=provenance["corpus_sha256"])))


if __name__ == "__main__":
    main()
