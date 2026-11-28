#!/usr/bin/env python3
"""Capture only static historical source-contender evidence from a verified export."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
CALLERS = ["src/task_mutation/tests.rs", "src/task_mutation/native_first.rs",
           "src/storage/maintenance/tests.rs", "src/transport_gate/tests.rs"]
CODES = dict(zip(["snapshot-key", "pending", "active-no-key", "active-bad-key", "corrupt", "snapshot-revision"],
                 ["snapshot_request_id_unsupported", "task_import_pending", "ledger_request_id_required",
                  "invalid_request_key", "invalid_compat_json", "invalid_snapshot_revision"], strict=True))


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode()


BOOTSTRAP = r'''
import base64, contextlib, datetime, io, itertools, json, sys, uuid
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from runner import coven, storage, task_attempts, task_ledger
root, mode, script_path = Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4])
counter = itertools.count(1)
uuid.uuid4 = lambda: uuid.UUID(int=next(counter))
class FixedDatetime(datetime.datetime):
    @classmethod
    def now(cls, tz=None):
        return cls(2026, 1, 2, 3, 4, 5, tzinfo=datetime.timezone.utc)
coven.datetime = task_attempts.datetime = FixedDatetime
(root / 'state').mkdir()
(root / 'logs').mkdir()
(root / 'checkpoints').mkdir()
(root / 'coven.json').write_bytes(b'{"name":"test","agents":["Worker_ID","other","___","---"],"display_names":{"other":"Display Name"}}')
(root / 'state/tasks.json').write_bytes(b'[]\n')
active = mode.startswith('active-') or mode == 'setup-ledger'
if active:
    with storage.workspace_lock(root, root / 'state/.workspace.lock') as held:
        task_ledger.LedgerStore(root).import_tasks(coven.read_task_snapshot(root)[0], held)
if mode.startswith('setup-'):
    coven.add_task(root, ' Test Title ', owner=' worker_id ', notes='note', request_id='key' if active else None)
else:
    (root / 'coven.json').unlink()
    if sys.argv[5] == 'directory': (root / 'coven.json').mkdir()
    if mode == 'pending': (root / 'state/task-ledger').mkdir()
    if mode == 'corrupt': (root / 'state/task-authority.json').write_bytes(b'bad')
inputs = {str(p.relative_to(root)): base64.b64encode(p.read_bytes()).decode()
          for p in sorted(root.rglob('*')) if p.is_file()}
directories = [str(p.relative_to(root)) for p in sorted(root.rglob('*')) if p.is_dir()]
stdout, stderr = io.StringIO(), io.StringIO()
sys.argv = [str(script_path), str(root), 'bad-revision' if mode == 'snapshot-revision' else '',
            'key' if mode == 'snapshot-key' else 'bad key' if mode == 'active-bad-key' else '']
with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
    exec(compile(script_path.read_bytes(), 'historical-inline-script', 'exec'))
artifacts = {str(p.relative_to(root)): base64.b64encode(p.read_bytes()).decode()
             for p in sorted(root.rglob('*')) if p.is_file() and str(p.relative_to(root)).startswith('state/')}
print(json.dumps(dict(stdout=stdout.getvalue(), stderr=stderr.getvalue(), inputs_base64=inputs,
                     input_directories=directories, artifacts_base64=artifacts), sort_keys=True))
'''


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"capture interrupted by signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    assert not list(root.rglob("*.pyc"))
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, "runner", "src", "tests"], cwd=project).decode().splitlines()
    paths = [p for p in paths if "__pycache__" not in p and (p.startswith(("runner/", "src/")) or p.endswith(".py"))]
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = sha(raw)
    assert sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    scripts = {}
    for caller in CALLERS:
        source = (root / caller).read_text()
        matches = [s for s in re.findall(r'r#"(.*?)"#', source, re.S)
                   if "from runner import" in s or "import storage" in s]
        assert len(matches) == (2 if caller == CALLERS[0] else 1), caller
        scripts[caller] = matches
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    rows = []
    # No race script is run here. Its exact original bytes are identified below.
    with tempfile.TemporaryDirectory(prefix="source-contenders-") as temporary:
        temporary = Path(temporary)
        for mode in [*CODES, "setup-snapshot", "setup-ledger"]:
            for config in (["missing", "directory"] if mode in CODES else ["valid"]):
                script = scripts[CALLERS[0]][0 if mode in CODES else 1]
                script_path = temporary / "historical-script.txt"
                script_path.write_text(script)
                workspace = temporary / "workspace"
                command = [sys.executable, "-I", "-B", "-c", BOOTSTRAP, str(root), str(workspace), mode, str(script_path), config]
                results = []
                for _ in range(2):
                    workspace.mkdir()
                    try:
                        child = subprocess.Popen(command, cwd=root, env=env, stdout=subprocess.PIPE,
                                                 stderr=subprocess.PIPE, start_new_session=True)
                        try:
                            out, err = child.communicate(timeout=15)
                            assert child.returncode == 0, (mode, out, err)
                        finally:
                            if child.poll() is None:
                                os.killpg(child.pid, signal.SIGKILL)
                            child.wait()
                        results.append(json.loads(out))
                    finally:
                        shutil.rmtree(workspace)
                assert results[0] == results[1], mode
                row = dict(id=f"{mode}-{config}", **results[0], script_sha256=sha(script.encode()),
                           repeated_equal=True, native_error_code=CODES.get(mode))
                row["input_bytes"] = sum(len(base64.b64decode(v)) for v in row["inputs_base64"].values())
                row["input_sha256"] = {p: sha(base64.b64decode(v)) for p, v in row["inputs_base64"].items()}
                row["stdout_bytes"] = len(row["stdout"].encode())
                row["stdout_sha256"] = sha(row["stdout"].encode())
                row["stderr_sha256"] = sha(row["stderr"].encode())
                if mode in CODES:
                    assert row["artifacts_base64"] == {p: v for p, v in row["inputs_base64"].items() if p.startswith("state/")}
                row["row_sha256"] = sha(encoded(row))
                rows.append(row)
    dispositions = [
        dict(caller=CALLERS[0], script_sha256=sha(scripts[CALLERS[0]][0].encode()),
             disposition="12 full static historical diagnostics captured. Native literal codes, configuration precedence and no-effects assertions retained.",
             parent_test="authority_admission_precedes_config_errors_like_python_but_still_requires_config", cases=12),
        dict(caller=CALLERS[0], script_sha256=sha(scripts[CALLERS[0]][1].encode()),
             disposition="2 coherent historical setup input/artifact records captured; not native output goldens. Live setup now calls native enable/enroll/prepare APIs. Fresh same-owner rejection and active known-receipt replay retained.",
             parent_test="fresh_same_owner_assign_is_not_exempt_from_prepared_execution_guard", cases=2),
        dict(caller=CALLERS[1], script_sha256=sha(scripts[CALLERS[1]][0].encode()),
             disposition="Python-specific cross-language race qualification retained as historical source evidence only, not captured or currently tested. Native independent libc contender uses a real barrier and unchanged inode, then calls native prepare API with stale task CAS; no attempt, receipt or control change on either backend.",
             parent_test="native_first_assignment_makes_queued_python_preparation_stale_on_both_backends",
             child_entry="queued_native_preparation_child", cases=2),
        dict(caller=CALLERS[2], script_sha256=sha(scripts[CALLERS[2]][0].encode()),
             disposition="Python-specific cross-language qualification retained as historical source evidence only, not captured or currently tested. Independent libc child proves busy, held, native exclusion, owned-child kill/reap and same-inode reacquisition.",
             parent_test="python_and_maintenance_exclude_each_other_and_process_death_releases",
             child_entry="independent_maintenance_child", cases=1),
        dict(caller=CALLERS[3], script_sha256=sha(scripts[CALLERS[3]][0].encode()),
             disposition="Bare-storage Python-specific cross-language qualification retained as historical source evidence only, not captured or currently tested. Six independent libc child launches preserve shared/exclusive compatibility in both directions, held barriers, release/reap and unchanged inode.",
             parent_test="python_flock_interoperability_in_both_directions",
             child_entry="independent_transport_child", cases=6),
    ]
    corpus = dict(schema_version=1, cases=rows, historical_dispositions=dispositions,
                  capture_kind="12 static diagnostics plus 2 coherent historical setup inputs; no race outcome capture; no native output golden")
    destination = project / "tests/retirement/fixtures/source_contenders.json"
    provenance_path = destination.with_name("source_contenders_provenance.json")
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      original_scripts={caller: [sha(s.encode()) for s in values] for caller, values in scripts.items()},
                      interpreter=dict(version=sys.version, sha256=sha(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      capture_tool_sha256=sha(Path(__file__).read_bytes()), bootstrap_sha256=sha(BOOTSTRAP.encode()),
                      environment={**env, "TMPDIR": "caller-owned disk-backed run"},
                      corpus_sha256=sha(encoded(corpus)), cases=len(rows), diagnostic_cases=12, setup_cases=2,
                      repeated_captures=28, dispositions=dispositions,
                      row_sha256={r["id"]: r["row_sha256"] for r in rows},
                      metadata_fix="UUID sequence and operation time only; locks, CAS, bytes and errors unmodified")
    for path, value in [(destination, corpus), (provenance_path, provenance)]:
        raw = encoded(value)
        if args.check:
            assert path.read_bytes() == raw, path
        else:
            path.write_bytes(raw)
        print(json.dumps(dict(path=str(path.relative_to(project)), bytes=len(raw), sha256=sha(raw))))


if __name__ == "__main__":
    main()
