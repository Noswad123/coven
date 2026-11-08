"""Explicit development-only reference capture; emit data/patches, never write files.

Run with -I -B and --oracle-root pointing at an explicitly chosen historical
reference checkout. This is not imported by the audit or any native test.
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


OBLIGATIONS = {
    "test_valid_selector_boundaries": "Six literal selectors including 64 characters; group alone and group+member+recognized; both default and recognized-only validation return None.",
    "test_invalid_selectors_raise_value_error": "All 19 invalid values in each of group/member positions raise ValueError: empty, 65 characters, uppercase, punctuation, prefixes, slash, whitespace, LF, NUL, Unicode and five non-string Python types. Non-string types require an explicit native typed-API substitute, not string coercion.",
    "test_member_requires_group": "Member without group rejects with recognized False and True.",
    "test_recognized_only_requires_literal_bool": "All nine non-bool Python values reject. Native bool signature and real CLI value/parser refusals are an explicit interface substitute, not fabricated native bool values.",
    "test_select_report_validates_before_reading_report": "None report with bad group, member-without-group, or integer recognized flag raises ValueError before report access. First two have direct native Null-report cases; third is excluded by native bool typing and checked at the CLI parser boundary.",
    "test_no_selectors_is_exact_independent_copy": "Observed/unavailable/incompatible reports copy exactly without view metadata; every mutable descendant is independent.",
    "test_group_selects_exact_associations_without_fabricating_roles": "Alpha selects p1,p2 in original order, one+legacy associations, counts 2/1, exact full-observation view metadata; all remaining context/declaration aggregates unchanged.",
    "test_member_is_scoped_to_group_even_with_shared_member_key": "Alpha/one selects exactly alpha/one, one target and no unmatched declarations; beta/one cannot leak through the shared key.",
    "test_changed_and_legacy_bindings_remain_visible_and_qualified": "Both changed and legacy member variants retain exact filtered target and binding_check, with all report context preserved.",
    "test_recognized_only_preserves_unassociated_targets_and_all_their_hints": "Recognized p1,p3,p4 retain complete targets including unassociated p4; counts 3/3 and declarations/context unchanged.",
    "test_recognition_and_declaration_filters_intersect": "Alpha+recognized keeps one target with two alpha associations; alpha/changed+recognized gives zero targets/counts.",
    "test_unmatched_declarations_filter_by_group_and_member_not_recognition": "Both recognized flags crossed with None/lost/replaced/missing member; exact unmatched declaration filtering and full context preserved.",
    "test_zero_selected_rows_remains_observed_not_native_absence": "Alph, missing and alpha/on select no rows but remain observed with zero counts; context preserved.",
    "test_empty_observation_and_uninitialized_declarations": "Empty observed report with uninitialized declarations retains zero observed/selected counts and complete uninitialized context.",
    "test_failures_keep_null_counts_and_declaration_failure_semantics": "Unavailable/incompatible crossed with three selection forms: null observed counts, full-observation scope, otherwise exact failure report and independent descendants.",
    "test_each_call_is_independent_and_never_retains_prior_success": "Alpha/beta results and input have disjoint mutable descendants; mutate nested native reference, limitations and unmatched list in alpha, leaving input/beta unchanged; fresh beta equals prior beta; later failure clears targets and keeps null count.",
    "test_selection_matrix_preserves_input_context_and_order": "All 26 group/member/recognized combinations: original unchanged, deep independence, preserved full context/aggregates, selected counts, recognized counts, input-relative pane order and finite JSON serialization.",
    "test_inspect_request_and_full_observation_counts_are_not_reinterpreted": "Filtered inspect keeps original request/pin, full observed count 1, declaration aggregates 7/4 despite zero selected targets.",
    "test_pure_validation_and_selection_have_no_io_or_clock_reads": "Direct validation plus selection returns one target with Python file/socket/process/wall/monotonic APIs poisoned. Native source review and local filesystem no-effect checks do not prove no clock/network/process reads; remain unresolved D.",
}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    if isinstance(value, bytes):
        return {"python_type": "bytes", "hex": value.hex()}
    if type(value) is object:
        return {"python_type": "object"}
    if isinstance(value, dict):
        return {key: encode(item) for key, item in value.items()}
    if isinstance(value, list):
        return [encode(item) for item in value]
    return value


def pointer(path, key):
    return path + "/" + str(key).replace("~", "~0").replace("/", "~1")


def patches(old, new, path=""):
    """Generic JSON delta only. No session selection logic or native expectations."""
    if json.dumps(old, sort_keys=True) == json.dumps(new, sort_keys=True):
        return []
    if isinstance(old, dict) and isinstance(new, dict):
        result = [{"remove": pointer(path, key)} for key in sorted(old.keys() - new.keys())]
        for key in sorted(new):
            result.extend(patches(old[key], new[key], pointer(path, key)) if key in old
                          else [{"set": pointer(path, key), "value": new[key]}])
        return result
    if isinstance(old, list) and isinstance(new, list) and new:
        result = []
        for index, value in enumerate(new):
            result.extend(patches(old[index], value, pointer(path, index)) if index < len(old)
                          else [{"set": pointer(path, index), "value": value}])
        result.extend({"remove": pointer(path, index)} for index in range(len(old) - 1, len(new) - 1, -1))
        return result
    return [{"set": path, "value": new}]


def capture(root):
    source = root / "tests/test_session_views.py"
    reference = root / "runner/session_views.py"
    raw = source.read_bytes()
    tree = ast.parse(raw, filename=str(source))
    identities = [(cls.name, node.name) for cls in tree.body if isinstance(cls, ast.ClassDef)
                  for node in cls.body if isinstance(node, ast.FunctionDef) and node.name.startswith("test_")]
    if len(identities) != 19 or {method for _, method in identities} != set(OBLIGATIONS):
        raise ValueError("reference test inventory changed; review capture obligations before regeneration")
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], check=True,
                          capture_output=True, text=True).stdout.strip()
    for name in ["tests/test_session_views.py", "runner/session_views.py"]:
        committed = subprocess.run(["git", "-C", str(root), "show", f"{head}:{name}"],
                                   check=True, capture_output=True).stdout
        if committed != (root / name).read_bytes():
            raise ValueError(f"historical reference differs from recorded commit: {name}")
    sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location("session_views_original_tests", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    views = module.views
    if Path(views.__file__).resolve() != reference.resolve():
        raise ValueError("reference import did not come from explicit --oracle-root")
    calls, depth = [], 0
    active = None

    def intercept(operation, original):
        def wrapped(*args, **options):
            nonlocal depth
            if depth:
                return original(*args, **options)
            item = {"method": active, "operation": operation, "options": encode(deepcopy(options))}
            if operation == "select":
                item["input"] = deepcopy(args[0])
            depth += 1
            try:
                output = original(*args, **options)
                item["expected"] = {"ok": True, "output": deepcopy(output)}
                return output
            except ValueError as error:
                item["expected"] = {"ok": False, "error": "ValueError", "message": str(error)}
                raise
            finally:
                depth -= 1
                calls.append(item)
        return wrapped

    original_validate, original_select = views.validate_selection, views.select_report
    views.validate_selection = intercept("validate", original_validate)
    views.select_report = intercept("select", original_select)
    result = unittest.TestResult()
    try:
        for cls, method in identities:
            active = cls + "." + method
            getattr(module, cls)(method).run(result)
    finally:
        views.validate_selection, views.select_report = original_validate, original_select
    if not result.wasSuccessful() or result.testsRun != 19 or result.skipped:
        raise ValueError(f"reference tests did not all pass: {result.errors!r} {result.failures!r} {result.skipped!r}")

    documents, values, lookup = [], [], {}

    def intern(value):
        key = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"), allow_nan=False)
        if key not in lookup:
            choices = [{"value": value}]
            for index, previous in enumerate(values):
                choices.append({"base": index, "patch": patches(previous, value)})
            documents.append(min(choices, key=lambda item: len(json.dumps(item, separators=(",", ":")))))
            lookup[key] = len(values)
            values.append(value)
        return lookup[key]

    by_method = {cls + "." + method: [] for cls, method in identities}
    for index, item in enumerate(calls, 1):
        item["id"] = f"v{index:03}"
        by_method[item.pop("method")].append(item["id"])
        if item["operation"] == "select":
            item["input"] = intern(item["input"])
        if item["expected"]["ok"]:
            item["expected"]["output"] = intern(item["expected"]["output"])
        options = item["options"]
        if any(value is not None and not isinstance(value, str) for key, value in options.items() if key in ("group_key", "member_key")):
            item["native_route"] = "typed_selector_exclusion"
        elif "recognized_only" in options and type(options["recognized_only"]) is not bool:
            item["native_route"] = "typed_bool_and_cli_refusal"
        else:
            item["native_route"] = "direct"
    compact = lambda value: json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"), allow_nan=False)
    data = '{"schema_version":1,"documents":[\n' + ",\n".join(json.dumps(doc, ensure_ascii=True, indent=2, sort_keys=True) for doc in documents)
    data += '\n],"cases":[\n' + ",\n".join(map(compact, calls)) + "\n]}\n"
    provenance = {
        "schema_version": 1, "oracle_head": head, "oracle_root": "explicit --oracle-root; no implicit checkout",
        "reference_files_match_commit": True,
        "python_version": sys.version, "python_isolated": bool(sys.flags.isolated), "python_no_bytecode": sys.dont_write_bytecode,
        "reference_source": {"path": "runner/session_views.py", "sha256": sha(reference.read_bytes())},
        "original_tests": {"path": "tests/test_session_views.py", "sha256": sha(raw)},
        "capture_tool_sha256": sha(Path(__file__).read_bytes()),
        "corpus": {"path": "tests/retirement/fixtures/session_views.json", "sha256": sha(data.encode()), "cases": len(calls), "documents": len(documents)},
        "encoding": "documents are full JSON values or generic set/remove JSON-pointer deltas from an earlier document; not native-derived; object/bytes Python arguments are explicitly tagged",
        "reference_result": "19 original test methods passed, including every original assertion and subTest loop; only outermost selection/validation calls captured",
        "methods": {"python::tests/test_session_views.py::" + cls + "::" + method: {
            "assertion_obligations": OBLIGATIONS[method], "edge_cases": by_method[cls + "." + method],
            "remaining_gap": "No native clock/network/process poisoning seam; remain D." if method.startswith("test_pure_validation") else None,
        } for cls, method in identities},
    }
    return data, json.dumps(provenance, ensure_ascii=True, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--emit-patch", action="store_true", help="emit an apply_patch envelope instead of corpus JSON")
    args = parser.parse_args()
    if not sys.flags.isolated or not sys.dont_write_bytecode:
        parser.error("run with python -I -B")
    data, provenance = capture(args.oracle_root.resolve(strict=True))
    if args.emit_patch:
        print("*** Begin Patch")
        for name, content in [("session_views.json", data), ("session_views_provenance.json", provenance)]:
            print("*** Add File: tests/retirement/fixtures/" + name)
            for line in content.splitlines():
                print("+" + line)
        print("*** End Patch")
    else:
        print(data, end="")


if __name__ == "__main__":
    main()
