"""Optional development capture, never imported by native tests.

Requires an explicit unchanged oracle checkout and Python 3.14.8 -I -B.
Emits an apply_patch envelope on stdout with --capture; does not write files.
"""

import argparse
import ast
import difflib
import hashlib
import importlib.util
import inspect
import json
from pathlib import Path
import subprocess
import sys
import unittest


EXCLUDED = {
    "test_invalid_api_inputs_raise_checkpoint_errors": "D: Python exception inheritance and dynamically typed API contract not claimed.",
    "test_direct_script_import_has_no_cli_or_native_runtime_dependency": "D: Python import/subprocess identity contract not claimed.",
}
REVIEWS = [
    "Three approve/suggest/approve steps; unchanged scope, prior snapshot binding, action, bound status, history length/notes, exact old history blocks, four distinct snapshots, JSON-compatible full view.",
    "All eight covered-byte substitutions: purpose, custom prose case, trailing space, fenced status, human rationale, nested checkbox, indented example and Unicode; each stale scope repaired and reinspected.",
    "CRLF, mixed-case padded status, tabbed checkbox and snow survive; LF conversion invalidates approval.",
    "Timestamp and history rationale edits alter snapshot but not scope or approved status; raw altered record spellings preserved.",
    "Missing/empty/other identifier hash domains; all three accepted and six rejected revision tokens; missing creation binds missing snapshot; absent initial content refuses. False is an explicit typed-boundary exclusion.",
    "Legacy uppercase approval never grants bound approval; exact original content, null decision, empty history; all four status variants including blank.",
    "All three contradictory status/checkbox substitutions retain scope, report inconsistent/decision_invalid with reason, then repair without changing scope.",
    "Changes requested stays effective and bound after purpose edit, with stale-scope reason.",
    "All six incomplete documents preserve scope and exact counts of user-decision sections and rejection labels; no covered sections invented.",
    "Archive next heading, hidden comment, tilde-fenced unsupported machine marker and indented machine marker remain exact covered bytes.",
    "Nested header/list/fence and checklist bytes remain exact; both nested status and checkbox mutations stale approval.",
    "Unicode and fenced notes roundtrip; angle brackets escaped in exact output; human history and previous machine-history bytes survive second decision unchanged.",
    "All three history relocations into heading/prose/inline code stale approval; appended unterminated prose also stale, then rebinds to new scope.",
    "All eight notes inputs refuse: two unclosed fences, decision section, two reserved machinery strings, raw closer, None and lone surrogate. Last two tagged typed-boundary cases, never coerced.",
    "All five ambiguous/unclosed inputs reject both inspect and prepare.",
    "Both record kinds crossed with all 15 bad field values, missing source, unsupported marker, trailing marker, duplicate key and NaN; duplicate decisions/history, decision-only, unfinished block, bad closing, wrong checkpoint and reused decision identity all refuse.",
]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def compact(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"), allow_nan=False)


def capture(root):
    paths = ["tests/test_checkpoints.py", "runner/checkpoints.py", "runner/records.py", "runner/storage.py"]
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    if head != "e389a08669204d4e51a49ffd13cbe01b90b5d8ba":
        raise ValueError("unexpected oracle HEAD")
    hashes = {}
    for name in paths:
        raw = (root / name).read_bytes()
        committed = subprocess.run(["git", "-C", str(root), "show", f"{head}:{name}"], check=True, capture_output=True).stdout
        if raw != committed:
            raise ValueError(f"original file changed: {name}")
        hashes[name] = sha(raw)
    source = (root / paths[0]).read_text()
    tree = ast.parse(source)
    methods = [node for cls in tree.body if isinstance(cls, ast.ClassDef)
               for node in cls.body if isinstance(node, ast.FunctionDef) and node.name.startswith("test_")]
    selected = [node for node in methods if node.name not in EXCLUDED]
    if len(methods) != 18 or len(selected) != len(REVIEWS):
        raise ValueError("original inventory changed; re-review all assertions")
    sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location("checkpoint_original_tests", root / paths[0])
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    cp = module.cp
    if Path(cp.__file__).resolve() != (root / "runner/checkpoints.py").resolve():
        raise ValueError("oracle import escaped explicit root")
    calls, active, depth = [], None, 0
    strings, expanded, lookup = [], [], {}

    def intern(text):
        if text not in lookup:
            best = {"text": text}
            size = len(compact(best))
            # Generic lossless string compression only, not checkpoint semantics.
            for index in range(max(0, len(expanded) - 32), len(expanded)):
                old = expanded[index]
                edits = [[a, b, text[c:d]] for op, a, b, c, d in
                         difflib.SequenceMatcher(None, old, text, autojunk=False).get_opcodes() if op != "equal"]
                candidate = {"base": index, "edits": edits}
                length = len(compact(candidate))
                if length < size:
                    best, size = candidate, length
            lookup[text] = len(strings)
            strings.append(best)
            expanded.append(text)
        return {"string": lookup[text]}

    def encode(value):
        if isinstance(value, bytes):
            return {"bytes_utf8": intern(value.decode("utf-8"))}
        if isinstance(value, str) and any(0xD800 <= ord(c) <= 0xDFFF for c in value):
            return {"python_type": "surrogate", "escaped": ascii(value)}
        if isinstance(value, dict):
            return {key: encode(item) for key, item in value.items()}
        if isinstance(value, (list, tuple)):
            return [encode(item) for item in value]
        return value

    def view(value):
        return intern(json.dumps(value.to_dict(), ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False))

    originals = {name: getattr(cp, name) for name in ("inspect", "prepare", "validate_revision")}

    def intercept(name):
        original = originals[name]
        signature = inspect.signature(original)

        def wrapped(*args, **kwargs):
            nonlocal depth
            if depth:
                return original(*args, **kwargs)
            bound = signature.bind(*args, **kwargs)
            bound.apply_defaults()
            item = {"id": f"c{len(calls) + 1:03}", "method": active, "operation": name, "input": encode(dict(bound.arguments)), "native_route": "direct"}
            if name == "prepare" and (bound.arguments["notes"] is None or isinstance(item["input"]["notes"], dict)):
                item["native_route"] = "typed_notes_boundary"
            if name == "validate_revision" and type(bound.arguments["token"]) is bool:
                item["native_route"] = "typed_revision_boundary"
            calls.append(item)
            depth += 1
            try:
                output = original(*args, **kwargs)
                if name == "prepare":
                    expected = {"bytes_utf8": intern(output[0].decode("utf-8")), "canonical_view": view(output[1])}
                elif name == "inspect":
                    expected = {"canonical_view": view(output)}
                else:
                    expected = {"token": output}
                item["expected"] = {"ok": True, **expected}
                return output
            except cp.CheckpointError as error:
                item["expected"] = {"ok": False, "type": "CheckpointError", "message": str(error)}
                raise
            finally:
                depth -= 1
        return wrapped

    result = unittest.TestResult()
    try:
        for name in originals:
            setattr(cp, name, intercept(name))
        for node in selected:
            active = "python::tests/test_checkpoints.py::CheckpointTests::" + node.name
            module.CheckpointTests(node.name).run(result)
    finally:
        for name, original in originals.items():
            setattr(cp, name, original)
    if not result.wasSuccessful() or result.testsRun != 16 or result.skipped:
        raise ValueError(f"original assertions failed: {result.errors!r} {result.failures!r}")
    corpus = compact({"schema_version": 1, "strings": strings, "cases": calls}) + "\n"
    provenance = {
        "schema_version": 1, "oracle_head": head, "reference_files_match_commit": True,
        "oracle_root": str(root), "source_sha256": hashes, "capture_tool_sha256": sha(Path(__file__).read_bytes()),
        "python_version": sys.version, "isolated": bool(sys.flags.isolated), "no_bytecode": sys.dont_write_bytecode,
        "capture_command": "PYTHONDONTWRITEBYTECODE=1 python3 -I -B tools/freeze_checkpoints.py --oracle-root <explicit-root> --capture",
        "corpus_sha256": sha(corpus.encode()), "cases": len(calls), "original_methods": 18, "captured_methods": 16,
        "encoding": "Lossless generic string pool; edits use Unicode character offsets into earlier strings, applied in reverse. Raw input/output text is UTF-8 bytes, not reserialized JSON. canonical_view is the full reference to_dict encoded once as sorted compact UTF-8 JSON. No Rust-derived expectations or checkpoint formatter implementation.",
        "reference_result": "All assertions and every subTest in 16 unchanged original methods passed once; outermost inspect/prepare/validate_revision calls recorded, internal calls suppressed.",
        "methods": {},
    }
    fixture_hashes = [{"path": "tests/retirement/fixtures/checkpoints.json", "sha256": sha(corpus.encode())}]
    reviews = {}
    native = ["rust::src/checkpoints/retirement.rs::frozen_calls_exact_views_outputs_and_independence", "rust::src/checkpoints/retirement.rs::inventory_hashes_and_typed_boundaries", "rust::src/checkpoints/retirement.rs::independent_hash_domains"]
    for node, rationale in zip(selected, REVIEWS):
        identity = "python::tests/test_checkpoints.py::CheckpointTests::" + node.name
        assertions = [ast.get_source_segment(source, child) for child in ast.walk(node)
                      if isinstance(child, ast.Call) and isinstance(child.func, ast.Attribute) and child.func.attr.startswith("assert")]
        info = {"call_ids": [item["id"] for item in calls if item["method"] == identity],
                "assertion_review": rationale, "original_assertions": assertions,
                "method_source_sha256": sha(ast.get_source_segment(source, node).encode())}
        provenance["methods"][identity] = info
        reviews[identity] = {"status": "mapped", "category": "A", "native_tests": native,
                             "fixtures": fixture_hashes.copy(), "assertion_breakdown": info,
                             "assertion_review": rationale + " Full frozen byte/view equality plus native nonmutation and old-history checks; Python exception text is evidence, not a claimed Rust error string."}
    for method, gap in EXCLUDED.items():
        reviews["python::tests/test_checkpoints.py::CheckpointTests::" + method] = {
            "status": "unresolved", "category": "D", "native_tests": [], "fixtures": [], "assertion_review": gap}
    provenance_text = json.dumps(provenance, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    for entry in reviews.values():
        if entry["status"] == "mapped":
            entry["fixtures"].append({"path": "tests/retirement/fixtures/checkpoints_provenance.json", "sha256": sha(provenance_text.encode())})
    return [("tests/retirement/fixtures/checkpoints.json", corpus),
            ("tests/retirement/fixtures/checkpoints_provenance.json", provenance_text),
            ("tests/retirement/reviews/checkpoints.json", json.dumps({"schema_version": 1, "reviews": reviews}, indent=2, sort_keys=True) + "\n")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--capture", action="store_true", help="explicitly execute original tests and emit fixture/review patch")
    args = parser.parse_args()
    if not args.capture:
        parser.error("capture is optional development work; pass --capture to execute reference tests")
    if sys.version_info[:3] != (3, 14, 8) or not sys.flags.isolated or not sys.dont_write_bytecode:
        parser.error("requires Python 3.14.8 -I -B")
    files = capture(args.oracle_root.resolve(strict=True))
    print("*** Begin Patch")
    for path, content in files:
        print("*** Add File: " + path)
        for line in content.splitlines():
            print("+" + line)
    print("*** End Patch")


if __name__ == "__main__":
    main()
