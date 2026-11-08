"""Optional, explicit historical oracle capture. Never invoked by native tests.

Run with isolated Python -I -B and --oracle-root. Output is an apply_patch
envelope; capture does not write files or derive expectations from Rust.
"""

import argparse
from copy import deepcopy
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest


EXCLUDED = {
    "test_invalid_sources_fail_closed_and_strict_json",
    "test_direct_script_import_compatible",
    "test_pure_operations_do_not_open_files_or_dispatch",
}
OPERATIONS = ("canonical", "digest", "node_digest", "validate_document",
              "validate_manifest", "topology", "freeze", "match_source")


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def dump(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False)


def pack(raw):
    """Generic lossless repeated-character compression, including boundary padding."""
    parts, start = [], 0
    for match in re.finditer(r"(.)\1{127,}", raw, re.DOTALL):
        parts.extend([raw[start:match.start()], {"repeat": match[1], "count": len(match[0])}])
        start = match.end()
    parts.append(raw[start:])
    return parts


def capture(root):
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    # Pin the complete transitive local oracle, not only the graph entrypoint.
    names = ["tests/test_task_graphs.py"] + sorted(
        str(path.relative_to(root)) for path in (root / "runner").glob("*.py"))
    hashes = {}
    for name in names:
        raw = (root / name).read_bytes()
        if raw != subprocess.check_output(["git", "-C", str(root), "show", f"{head}:{name}"]):
            raise ValueError(f"reference differs from recorded commit: {name}")
        hashes[name] = sha(raw)
    sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location("original_task_graph_tests", root / names[0])
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    graphs = module.graphs
    if Path(graphs.__file__).resolve() != root / "runner/task_graphs.py":
        raise ValueError("wrong oracle import")
    methods = unittest.defaultTestLoader.getTestCaseNames(module.TaskGraphTests)
    if len(methods) != 18 or len(set(methods) - EXCLUDED) != 15:
        raise ValueError("original inventory changed")
    calls, documents, lookup, by_method = [], [], {}, {}
    depth, active = 0, None

    def intern(value):
        raw = dump(value)
        if raw not in lookup:
            lookup[raw] = len(documents)
            documents.append({"parts": pack(raw), "sha256": sha(raw.encode()), "bytes": len(raw.encode())})
        return lookup[raw]

    def intercept(operation, original):
        def wrapped(*args, **kwargs):
            nonlocal depth
            if depth:
                return original(*args, **kwargs)
            case = {"id": f"g{len(calls) + 1:04}", "operation": operation,
                    "args": intern(deepcopy(list(args))), "options": intern(deepcopy(kwargs))}
            depth += 1
            try:
                result = original(*args, **kwargs)
                case["ok"] = True
                case["output"] = intern(result.decode("utf-8") if isinstance(result, bytes) else result)
                return result
            except graphs.GraphError as error:
                case.update(ok=False, error_type="GraphError", message=str(error))
                raise
            finally:
                depth -= 1
                calls.append(case)
                by_method[active].append(case["id"])
        return wrapped

    originals = {name: getattr(graphs, name) for name in OPERATIONS}
    for name, original in originals.items():
        setattr(graphs, name, intercept(name, original))
    result = unittest.TestResult()
    try:
        for method in methods:
            if method in EXCLUDED:
                continue
            active = method
            by_method[active] = []
            module.TaskGraphTests(method).run(result)
    finally:
        for name, original in originals.items():
            setattr(graphs, name, original)
    if not result.wasSuccessful() or result.testsRun != 15 or result.skipped:
        raise ValueError(f"original assertions failed: {result.errors!r} {result.failures!r}")
    corpus = dump({"schema_version": 1, "documents": documents, "cases": calls}) + "\n"
    provenance = {
        "schema_version": 1, "oracle_commit": head, "oracle_version": sys.version,
        "isolated": bool(sys.flags.isolated), "no_bytecode": sys.dont_write_bytecode,
        "explicit_oracle_root": str(root), "source_hashes": hashes,
        "capture_tool_sha256": sha(Path(__file__).read_bytes()),
        "corpus_sha256": sha(corpus.encode()), "cases": len(calls), "documents": len(documents),
        "reference_result": "15 unchanged original methods passed with all assertions and subcases",
        "encoding": "Interned raw JSON strings, never serde-normalized; parts concatenate literal strings or repeat a character count times. Raw JSON preserves float 1.0, Unicode and arrays. Canonical bytes are captured as UTF-8 strings. SHA256 and byte length cover expanded raw JSON.",
        "error_policy": "Record Python GraphError messages for provenance; native refusal acceptance, not diagnostic wording, is compared.",
        "methods": {"python::tests/test_task_graphs.py::TaskGraphTests::" + method: {
            "cases": by_method.get(method, []), "status": "excluded D" if method in EXCLUDED else "captured",
        } for method in methods},
        "typed_dispositions": "Replay represents validated enrollment state, not a Python map. Outer map-key/task_id disagreement is retained but cannot be submitted directly through Replay. Keep malformed-current-enrollments unresolved pending boundary evidence; do not emulate Python validation in the adapter. Nonfinite numbers, nonstring dict keys, tuples and lone surrogates belong to excluded strict-JSON method and are not silently coerced.",
    }
    return corpus, json.dumps(provenance, ensure_ascii=True, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, help="write generated data to this existing directory instead of stdout")
    args = parser.parse_args()
    if not sys.flags.isolated or not sys.dont_write_bytecode:
        parser.error("use python -I -B")
    corpus, provenance = capture(args.oracle_root.resolve(strict=True))
    if args.output_dir:
        for name, content in [("task_graphs.json", corpus), ("task_graphs_provenance.json", provenance)]:
            (args.output_dir / name).write_text(content, encoding="utf-8")
        return
    print("*** Begin Patch")
    for name, content in [("task_graphs.json", corpus), ("task_graphs_provenance.json", provenance)]:
        print("*** Add File: tests/retirement/fixtures/" + name)
        for line in content.splitlines():
            print("+" + line)
    print("*** End Patch")


if __name__ == "__main__":
    main()
