"""Development-only, explicit historical oracle capture; emits data, never writes.

Run with isolated Python -I -B and --oracle-root. Native tests never use this tool.
"""

import argparse
import ast
from copy import deepcopy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def capture(root):
    names = ("tests/test_records.py", "runner/records.py")
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                          check=True, capture_output=True, text=True).stdout.strip()
    sources = {}
    for name in names:
        raw = (root / name).read_bytes()
        committed = subprocess.run(["git", "-C", str(root), "show", f"{head}:{name}"],
                                   check=True, capture_output=True).stdout
        if raw != committed:
            raise ValueError(f"reference differs from recorded commit: {name}")
        sources[name] = digest(raw)
    tree = ast.parse((root / names[0]).read_bytes())
    methods = [node.name for cls in tree.body if isinstance(cls, ast.ClassDef) and cls.name == "RecordTests"
               for node in cls.body if isinstance(node, ast.FunctionDef) and node.name.startswith("test_")]
    expected_counts = [12, 18, 21, 4]
    if len(methods) != 4:
        raise ValueError("original inventory changed; review before capture")
    sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location("records_original_tests", root / names[0])
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    records = module.records
    if Path(records.__file__).resolve() != (root / names[1]).resolve():
        raise ValueError("oracle import escaped explicit root")
    calls, originals = [], {}
    active = None
    encode = lambda value: json.dumps(value, ensure_ascii=True, separators=(",", ":"), allow_nan=False)

    def intercept(operation, original):
        def wrapped(value):
            item = {"id": f"r{len(calls) + 1:03}", "method": active, "operation": operation,
                    "input_json": encode(deepcopy(value))}
            try:
                output = original(value)
                item["expected"] = {"ok": True, "output_json": encode(output), "same_object": output is value}
                return output
            except ValueError as error:
                item["expected"] = {"ok": False, "error": "ValueError", "message": str(error)}
                raise
            finally:
                item["input_unchanged"] = encode(value) == item["input_json"]
                calls.append(item)
        return wrapped

    result = unittest.TestResult()
    try:
        for operation in ("team", "tasks", "agents", "lead"):
            originals[operation] = getattr(records, operation)
            setattr(records, operation, intercept(operation, originals[operation]))
        for method in methods:
            active = method
            module.RecordTests(method).run(result)
    finally:
        for operation, original in originals.items():
            setattr(records, operation, original)
    if not result.wasSuccessful() or result.testsRun != 4 or result.skipped:
        raise ValueError(f"original tests failed: {result.errors!r} {result.failures!r} {result.skipped!r}")
    counts = [sum(item["method"] == method for item in calls) for method in methods]
    if counts != expected_counts:
        raise ValueError(f"call inventory changed: {counts}")
    data = '{"schema_version":1,"cases":[\n' + ",\n".join(encode(item) for item in calls) + "\n]}\n"
    provenance = {
        "schema_version": 1, "oracle_root": str(root), "oracle_head": head,
        "reference_files_match_commit": True, "sources_sha256": sources,
        "python_version": sys.version, "python_isolated": bool(sys.flags.isolated),
        "python_no_bytecode": sys.dont_write_bytecode,
        "capture_tool_sha256": digest(Path(__file__).read_bytes()),
        "capture_command": "env -i PATH=/usr/bin:/bin HOME=/tmp PYTHONDONTWRITEBYTECODE=1 /usr/bin/python3 -I -B tools/freeze_records.py --oracle-root " + str(root) + " --emit-patch",
        "reference_result": "4 unchanged original methods passed once; all assertions and subTests active",
        "calls": len(calls), "methods": {
            "python::tests/test_records.py::RecordTests::" + method: {
                "calls": count, "case_ids": [item["id"] for item in calls if item["method"] == method]
            } for method, count in zip(methods, counts)
        },
        "corpus": {"path": "tests/retirement/fixtures/records.json", "sha256": digest(data.encode())},
        "encoding": "input_json/output_json are ordered finite JSON text, including escaped unpaired surrogate input. Outputs/errors/identity observed only from Python; no native-derived expectations.",
        "boundary_difference": "Python can construct an unpaired surrogate str and rejects it in team text validation. Rust String/Value cannot contain it; escaped JSON is rejected by values::parse before record validation. Error text is provenance, not a native diagnostic contract.",
    }
    return data, json.dumps(provenance, ensure_ascii=True, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--emit-patch", action="store_true")
    args = parser.parse_args()
    if not sys.flags.isolated or not sys.dont_write_bytecode or sys.version_info[:3] != (3, 14, 8):
        parser.error("capture requires Python 3.14.8 with -I -B")
    data, provenance = capture(args.oracle_root.resolve(strict=True))
    if args.emit_patch:
        print("*** Begin Patch")
        for name, content in (("records.json", data), ("records_provenance.json", provenance)):
            print("*** Add File: tests/retirement/fixtures/" + name)
            for line in content.splitlines():
                print("+" + line)
        print("*** End Patch")
    else:
        print(data, end="")


if __name__ == "__main__":
    main()
