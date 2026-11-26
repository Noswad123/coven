#!/usr/bin/env python3
"""Freeze the execution-source oracle from an explicit verified Git export only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

PIN = "ca3b19d87c955d6415231b1c4ebbbc591195a105"
HELPER = "src/execution/source/oracle.py"
CASES = ["extensions", "empty", "whitespace", "fresh", "ledger"]
RAW = [
    br' [ {"id":"task","title":"Title","status":"custom","owner":"outside","extra":{"z":-0.0,"big":123456789012345678901234567890,"unicode":"\u03bb\ud83e\udd8a","float":1e-7,"nested":[null,true]}} ] ',
    b'[]', b' \n[]\t',
    b'[{"id":"fresh","title":"Fresh","status":"pending"}]',
    b'[{"id":"new","title":"New","status":"pending"}]',
]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    paths = subprocess.check_output([
        "git", "ls-tree", "-r", "--name-only", PIN, "runner", "examples",
        "src/execution.rs", "src/execution/source.rs", "src/execution/source",
    ], cwd=project).decode().splitlines()
    paths = [p for p in paths if "__pycache__" not in p]
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert HELPER in pins and sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    bootstrap = '''import sys
from pathlib import Path
sys.path.insert(0, sys.argv.pop(1))
root = Path(sys.argv[1])
if sys.argv.pop() == "ledger":
    from runner import storage, task_ledger
    with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
        task_ledger.LedgerStore(root).import_tasks(storage.snapshot_revision((root / "state/tasks.json").read_bytes()), held)
exec(compile(Path(sys.path[0], "src/execution/source/oracle.py").read_bytes(), "historical-oracle", "exec"))
'''
    captures = []
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    for case, raw in zip(CASES, RAW, strict=True):
        with tempfile.TemporaryDirectory(prefix="execution-source-") as temporary:
            workspace = Path(temporary)
            (workspace / "state").mkdir()
            (workspace / "coven.json").write_bytes(b'{"name":"test","agents":["b","a"],"unknown":{"preserved":true}}')
            (workspace / "state/tasks.json").write_bytes(raw)
            command = [sys.executable, "-I", "-B", "-c", bootstrap, str(root), str(workspace), case]
            result = subprocess.run(command, cwd=root, env=env, capture_output=True, check=True)
            stdout = result.stdout
            record = dict(id=case, tasks_raw=raw.decode(), tasks_raw_sha256=digest(raw),
                          stdout=stdout.decode(), stdout_sha256=digest(stdout), stdout_bytes=len(stdout),
                          exit_status=result.returncode, stderr=result.stderr.decode(), slots=[])
            if case == "ledger":
                identity = json.loads(stdout)["identity"]
                record["captured_ids"] = {k: identity[k] for k in ["workspace_id", "epoch_id"]}
                contexts = [(b'"epoch_id":"', "epoch_id"), (b'"workspace_id":"', "workspace_id"),
                            (b'"revision":"tasks-ledger:v1:', "workspace_id")]
                for context, binding in contexts:
                    assert stdout.count(context) == 1
                    offset = stdout.index(context) + len(context)
                    record["slots"].append(dict(offset=offset, binding=binding, before=context.decode(), after=chr(stdout[offset + 32])))
                offset = record["slots"][2]["offset"] + 33
                record["slots"].append(dict(offset=offset, binding="epoch_id", before=":", after=":"))
                template = bytearray(stdout)
                for slot in record["slots"]:
                    offset = slot["offset"]
                    value = identity[slot["binding"]]
                    assert re.fullmatch("[0-9a-f]{32}", value)
                    assert stdout[offset:offset + 32] == value.encode()
                    template[offset:offset + 32] = b"0" * 32
                proof = bytearray(template)
                for slot in record["slots"]:
                    proof[slot["offset"]:slot["offset"] + 32] = identity[slot["binding"]].encode()
                assert bytes(proof) == stdout
                record.update(template=template.decode(), roundtrip_equal=True)
            else:
                assert json.loads(stdout)["revision"] == "tasks-sha256:" + digest(raw)
            captures.append(record)
    corpus = dict(schema_version=1, case_ids=CASES, cases=captures)
    destination = project / "tests/retirement/fixtures/execution_source.json"
    destination.write_bytes(encoded(corpus))
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      interpreter=dict(version=sys.version, executable=sys.executable,
                                       sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      capture_tool_sha256=digest(Path(__file__).read_bytes()), bootstrap=bootstrap,
                      environment=env, case_ids=CASES, cases=5, full_byte_comparisons=5,
                      corpus_sha256=digest(destination.read_bytes()),
                      proof="Historical helper stdout retained verbatim as opaque UTF-8. Four fixed 32-byte slots only; captured IDs restore stdout exactly. No native output used.")
    provenance_path = project / "tests/retirement/fixtures/execution_source_provenance.json"
    provenance_path.write_bytes(encoded(provenance))
    fixtures = [dict(path=str(p.relative_to(project)), bytes=p.stat().st_size, sha256=digest(p.read_bytes())) for p in [destination, provenance_path]]
    review = dict(schema_version=1, approval_status="pending_parent_review", mapping_delta=0,
                  historical_commit=PIN, fixtures=fixtures,
                  retired_helpers={HELPER: dict(sha256=pins[HELPER], kind="runner_oracle")},
                  original_tests=18, caller_tests=4, executed_comparisons=5,
                  case_ids=CASES, binding_slots=4,
                  scope="Execution source unit only. Original 941 total / 90 mapped / 851 unresolved and registry 4 active + 1 retired = 5 unchanged. Zero new mapping claims; no global retirement approval.",
                  validation="Pending normal and Python-filesystem-absent 18-test runs.")
    (project / "tests/retirement/reviews/execution_source_oracle.json").write_bytes(encoded(review))
    print(json.dumps(dict(fixtures=fixtures, source_pins=len(pins), cases=[dict(id=c['id'], bytes=c['stdout_bytes'], sha256=c['stdout_sha256']) for c in captures])))


if __name__ == "__main__":
    main()
