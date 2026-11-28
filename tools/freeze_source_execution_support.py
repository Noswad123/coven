#!/usr/bin/env python3
"""Capture B2 from an explicit unmodified a249 export, never the active runner.

Export with git archive a249a5dfe8d32fd75022f8ed2ad524264b7436a0 |
tar -x --exclude='*.pyc' --exclude='*/__pycache__' -C ROOT.
Run with Python 3.14.8 -I -B and TMPDIR on verified disk-backed storage.
This is regeneration closure, not live-constructor retirement credit.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
CAPACITY = "src/execution/mutation/capacity/oracle.py"
UPGRADE = "src/execution/mutation/upgrade/oracle.py"
GUARDS = "src/execution/guards.rs"
PROJECTION = "src/execution/inspection/projection/oracle.py"
VECTORS = "src/execution/inspection/projection/vectors.json"
DEST = "tests/retirement/fixtures/source_execution_support"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", type=Path, required=True)
    parser.add_argument("--projection-output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    assert os.environ.get("TMPDIR"), "explicit disk-backed TMPDIR required"
    paths = subprocess.check_output([
        "git", "ls-tree", "-r", "--name-only", PIN, "runner", "src/execution.rs",
        "src/execution", "tests/_execution_v2_parity.py", "src/values", "src/canonical.rs",
        "src/platform.rs", "src/platform", "src/read_fs.rs", "src/storage.rs", "src/storage", "src/output.rs",
    ], cwd=project).decode().splitlines()
    paths = [p for p in paths if "__pycache__" not in p]
    pins = {}
    # Verify the entire caller/native subtree and every runner module before execution.
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = sha(raw)
    assert sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    assert not list(root.rglob("*.pyc"))
    retained = project / VECTORS
    assert sha(retained.read_bytes()) == pins[VECTORS]
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    bootstrap = '''import sys, runpy
from pathlib import Path
sys.path.insert(0, sys.argv.pop(1))
from datetime import datetime
from types import SimpleNamespace
from unittest.mock import patch
from runner import task_attempts as v1, execution_v2 as v2
import itertools
ids = itertools.count(1)
now = datetime.fromisoformat("2026-11-02T01:02:03.123456+00:00")
with patch.object(v1.uuid, "uuid4", side_effect=lambda: SimpleNamespace(hex=f"{next(ids):032x}")), patch.object(v1, "datetime") as c1, patch.object(v2, "datetime") as c2:
    c1.now.return_value = c2.now.return_value = now
    helper = sys.argv.pop(1)
    if helper == "inline":
        exec(compile(sys.argv.pop(1), "historical-guards-inline", "exec"))
    else:
        sys.argv[0] = helper
        runpy.run_path(helper, run_name="__main__")
'''

    def run(helper, arguments=(), stdin=b""):
        command = [sys.executable, "-I", "-B", "-c", bootstrap, str(root), helper, *arguments]
        child = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            stdout, stderr = child.communicate(stdin, timeout=120)
            assert child.returncode == 0 and not stderr, (child.returncode, stderr)
            return stdout
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()

    guard_source = (root / GUARDS).read_text().split('.args(["-B", "-c", r#"', 1)[1].split('"#])', 1)[0]
    native = (root / "src/execution.rs").read_text()
    literal = native.split('pub(super) fn enabled() -> Value {', 1)[1].split('r#"', 1)[1].split('"#', 1)[0]
    # This predecessor is fixed historical input, not reconstructed from native actual output.
    control = json.loads(literal)
    raw = " \n\t" + json.dumps(control, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + " \r\n\n"
    upgrade_input = json.dumps(dict(raw=raw, mode=0o640), ensure_ascii=True).encode()
    specifications = [
        ("capacity-reviewed-3-6", str(root / CAPACITY), (), b"", {"task_counts": [3, 6], "argv": []}, 2),
        ("upgrade-raw-0640", str(root / UPGRADE), (), upgrade_input, json.loads(upgrade_input), 1),
        ("guards-12-states", "inline", (guard_source,), b"", {"inline_sha256": sha(guard_source.encode()), "states": [
            "v1-enabled", "v2-upgraded", "enrolled", "standalone-prepared", "standalone-cancelled",
            "graph-proposed", "graph-approved", "graph-prepared", "stale-prepared",
            "revoked-prepared", "revoked-cancelled", "approved-no-attempt-history"]}, 12),
    ]
    rows = []
    for name, helper, arguments, stdin, inputs, count in specifications:
        first = run(helper, arguments, stdin)
        assert first == run(helper, arguments, stdin), name
        value = json.loads(first)
        assert (len(value) if isinstance(value, list) else 1) == count
        row = dict(id=name, input=inputs, stdout=first.decode(), stdout_bytes=len(first),
                   stdout_sha256=sha(first), output_count=count)
        row["row_sha256"] = sha(encoded(row))
        rows.append(row)
    near = run(str(root / CAPACITY), ("--near-nodes",))
    assert near == run(str(root / CAPACITY), ("--near-nodes",))
    near_value = json.loads(near)
    def nodes(value):
        if isinstance(value, dict):
            return 1 + sum(map(nodes, value.values()))
        if isinstance(value, list):
            return 1 + sum(map(nodes, value))
        return 1
    assert nodes(near_value) == 262048
    near_canonical = json.dumps(near_value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    near_proof = dict(id="capacity-near-nodes", input={"argv": ["--near-nodes"], "task_count": 3},
                      output_count=1, target=262144 - 96, copies=2, arrays=3,
                      max_items=65536, stdout_bytes=len(near), stdout_sha256=sha(near),
                      canonical_sha256=sha(near_canonical), canonical_bytes=len(near_canonical),
                      padding_lengths=[len(a) for a in near_value["extensions"]["padding"]])
    # Independently verify the small recipe consumed by the private Rust helper.
    # Only extension padding and cryptographic prefix links change, not effects/results.
    recipe = json.loads(rows[0]["stdout"])[0]
    extension = {"padding": [[None] * n for n in [65536, 65019, 0]]}
    recipe["extensions"] = extension
    legacy = recipe["legacy_v1"]
    legacy["extensions"] = extension
    def canonical(value):
        return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    def revision(value, schema):
        return f"execution-control-v{schema}:{value['execution_authority_id']}:{value['generation']}:{sha(canonical(value))}"
    for index in range(1, len(legacy["history"])):
        prefix = {**legacy, "history": legacy["history"][:index], "generation": index}
        legacy["history"][index]["binding"]["expected_execution_revision"] = revision(prefix, 1)
    previous = revision(legacy, 1)
    recipe["upgrade"]["source_revision"] = previous
    for key in ["effect", "result"]:
        recipe["history"][0][key]["source_revision"] = previous
    for index, entry in enumerate(recipe["history"]):
        entry["binding"]["expected_execution_revision"] = previous
        prefix = {**recipe, "history": recipe["history"][:index + 1], "generation": entry["generation"]}
        previous = revision(prefix, 2)
    assert canonical(recipe) == near_canonical
    assert (json.dumps(recipe, ensure_ascii=True) + "\n").encode() == near
    near_proof["independent_recipe_complete_byte_equal"] = True
    near_proof["row_sha256"] = sha(encoded(near_proof))
    vectors = json.loads(retained.read_bytes())
    assert sum(len(v["cases"]) for v in vectors) == 42
    if args.projection_output:
        # Old generator owns output. No active implementation or vector-copy fallback.
        output = args.projection_output.resolve()
        assert not output.is_relative_to(project) and not output.is_relative_to(root)
        run(str(root / PROJECTION), (str(output),))
        first = output.read_bytes()
        run(str(root / PROJECTION), (str(output),))
        assert output.read_bytes() == first
        generated = json.loads(first)
        assert sum(len(v["cases"]) for v in generated) == 42
        print(json.dumps(dict(projection_regenerated_sha256=sha(first), retained_vectors_sha256=pins[VECTORS],
                              note="Metadata mocks make regeneration repeatable; existing vectors remain byte-unchanged. No claim of constructor credit or regenerated-byte identity to retained vectors.")))
    corpus = dict(schema_version=1, case_ids=[r["id"] for r in rows], cases=rows, near_nodes=near_proof)
    payload = encoded(corpus)
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      corpus_sha256=sha(payload), retained_vectors_sha256=pins[VECTORS],
                      interpreter=dict(version=sys.version, executable=sys.executable,
                                       sha256=sha(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      capture_tool_sha256=sha(Path(__file__).read_bytes()), bootstrap=bootstrap,
                      environment=env, repeats=2, case_inputs=[s[4] for s in specifications],
                      output_counts=[2, 1, 12], near_nodes=near_proof,
                      mocks="Only UUID and datetime metadata fixed; upgrade helper retains its original UUID mock. No business logic patched.",
                      proof="Complete opaque stdout bytes and strict fixed inputs. Near-node output represented by independent padding specification and complete-byte hash, not a giant JSON blob. Projection retains existing 42 cases unchanged; regeneration closure only.")
    for suffix, data in [(".json", payload), ("_provenance.json", encoded(provenance))]:
        destination = project / (DEST + suffix)
        if args.check:
            if suffix == ".json":
                assert destination.read_bytes() == data
            else:
                previous = json.loads(destination.read_bytes())
                for key in ["source_sha256", "corpus_sha256", "retained_vectors_sha256", "bootstrap", "output_counts", "near_nodes", "capture_tool_sha256", "case_inputs", "interpreter"]:
                    assert previous[key] == provenance[key], key
        else:
            destination.write_bytes(data)
    print(json.dumps(dict(corpus_sha256=sha(payload), rows=[{k: r[k] for k in ["id", "stdout_bytes", "stdout_sha256", "output_count"]} for r in rows], near_nodes=near_proof, source_pins=len(pins), runner_modules=25, projection_cases=42)))


if __name__ == "__main__":
    def interrupted(signum, _frame):
        raise KeyboardInterrupt(signum)
    signal.signal(signal.SIGTERM, interrupted)
    main()
