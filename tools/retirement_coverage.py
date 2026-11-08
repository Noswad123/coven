"""Read-only retirement ledger audit. Parse Python with AST; never import the runner."""

import argparse
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = "tests/retirement/coverage.json"
CATEGORIES = ("A", "B", "C", "D")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def local_file(root, name):
    path = Path(name)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe repository reference: {name}")
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
        raise ValueError(f"missing or escaping repository reference: {name}")
    return resolved


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def methods(path):
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    result = {}
    for cls in tree.body:
        if isinstance(cls, ast.ClassDef):
            for node in cls.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test_"):
                    key = f"{cls.name}.{node.name}"
                    if key in result:
                        raise ValueError(f"duplicate method: {path}:{key}")
                    result[key] = node
    return result


def python_inventory(root):
    """Conservative file-level import closure, including literal child-code imports.

    Imports in setup, helpers and methods all apply to every method in that file.
    Conditional imports are not assumed unreachable. No imported code is executed.
    """
    paths = sorted(set(root.glob("tests/*.py")) | set(root.glob("src/**/*.py")) | set(root.glob("runner/*.py")))
    aliases = {}
    for path in paths:
        name = path.relative_to(root).as_posix()
        if name.startswith("runner/"):
            aliases[f"runner.{path.stem}"] = name
            aliases[path.stem] = name  # Legacy scripts prepend runner/ to sys.path.
        elif name.startswith("tests/"):
            aliases[path.stem] = name
    graph, direct, external, dynamic = {}, {}, {}, {}
    for path in paths:
        name = path.relative_to(root).as_posix()
        imports, unknown_dynamic = set(), []
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=name)
        trees = [tree]
        for node in ast.walk(tree):
            if isinstance(node, ast.Constant) and isinstance(node.value, str) and re.search(r"\b(from|import)\s+", node.value):
                try:
                    trees.append(ast.parse(node.value))
                except SyntaxError:
                    if re.search(r"\bfrom runner\b|\bimport (?:runner|coven|storage)\b", node.value):
                        unknown_dynamic.append(node.lineno)
        for parsed in trees:
            for node in ast.walk(parsed):
                if isinstance(node, ast.Import):
                    imports.update(item.name for item in node.names)
                elif isinstance(node, ast.ImportFrom):
                    module = node.module or ""
                    if node.level and module and name.startswith("runner/"):
                        imports.add(f"runner.{module}")
                    elif module == "runner" or (node.level and name.startswith("runner/")):
                        imports.update(f"runner.{item.name}" for item in node.names)
                    else:
                        imports.add(module)
                        imports.update(f"{module}.{item.name}" for item in node.names if f"{module}.{item.name}" in aliases)
                elif isinstance(node, ast.Call):
                    function = node.func
                    if (isinstance(function, ast.Name) and function.id == "__import__") or (
                        isinstance(function, ast.Attribute) and function.attr == "import_module"
                    ):
                        if node.args and isinstance(node.args[0], ast.Constant) and isinstance(node.args[0].value, str):
                            imports.add(node.args[0].value)
                        else:
                            unknown_dynamic.append(node.lineno)
        graph[name] = sorted({aliases[item] for item in imports if item in aliases})
        direct[name] = sorted({item for item in imports if item.startswith("runner.")} | {
            f"runner.{item}" for item in imports if item in aliases and aliases[item].startswith("runner/")
        })
        external[name] = sorted(item for item in imports if item and item not in aliases and item != "runner"
                                and item.split(".")[0] not in sys.stdlib_module_names and item != "__future__")
        dynamic[name] = unknown_dynamic
    result = {}
    for name in graph:
        seen, pending = set(), [name]
        while pending:
            current = pending.pop()
            if current not in seen:
                seen.add(current)
                pending.extend(graph[current])
        result[name] = {
            "direct_runner_imports": direct[name],
            "local_imports": graph[name],
            "runner_dependencies": sorted(item for item in seen if item.startswith("runner/")),
            "external_imports": sorted({item for dep in seen for item in external[dep]}),
            "dynamic_import_files": sorted(dep for dep in seen if dynamic[dep]),
        }
    return result


def rust_tokens(source, mask_strings=False):
    """Small lexical inventory, NOT a Rust parser or execution/security proof.

    Keep string tokens, blank comments, and handle nested block comments/raw
    strings so comments cannot manufacture mod declarations or Python findings.
    """
    output, strings = [], []
    index = 0
    while index < len(source):
        if source.startswith("//", index):
            end = source.find("\n", index)
            end = len(source) if end < 0 else end
            output.append(" " * (end - index))
        elif source.startswith("/*", index):
            end, depth = index + 2, 1
            while end < len(source) and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                raise ValueError("unterminated Rust block comment")
            output.append(" " * (end - index))
        else:
            raw = re.match(r'(?:br|r)(#*)"', source[index:])
            character = re.match(r"(?:b)?'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'", source[index:])
            if character:
                end = index + character.end()
                output.append(source[index:end])
            elif raw:
                start = index + raw.end()
                end = source.find('"' + raw[1], start)
                if end < 0:
                    raise ValueError("unterminated Rust raw string")
                strings.append(source[start:end])
                end += 1 + len(raw[1])
                output.append(" " * (end - index) if mask_strings else source[index:end])
            elif source[index] == '"':
                end = index + 1
                while end < len(source):
                    if source[end] == "\\":
                        end += 2
                    elif source[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
                strings.append(source[index + 1:end - 1])
                output.append(" " * (end - index) if mask_strings else source[index:end])
            else:
                output.append(source[index])
                end = index + 1
        index = end
    return "".join(output), strings


def rust_tests(root, name):
    code, _ = rust_tokens(local_file(root, name).read_text(encoding="utf-8"), mask_strings=True)
    return sorted(re.findall(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+(\w+)\s*\(", code))


def native_inventory(root, entries):
    result = []
    for entry in entries:
        name = entry["path"]
        if not re.fullmatch(r"tests/[a-z_]+\.rs", name):
            raise ValueError(f"not an exact integration target: {name}")
        seen, pending, findings = set(), [name], []
        while pending:
            current = pending.pop()
            if current in seen:
                continue
            seen.add(current)
            code, strings = rust_tokens(local_file(root, current).read_text(encoding="utf-8"))
            for value in strings:
                if re.fullmatch(r"(?:.*/)?python(?:\d+(?:\.\d+)*)?", value) or value.endswith(".py") or re.search(r"\bfrom runner\b|\bimport (?:runner|coven|storage)\b", value):
                    findings.append(f"{current}: Python/helper string {value!r}")
            masked = re.sub(r'#\[path\s*=\s*"([^"]+)"\]\s*(?:pub\s+)?mod\s+\w+\s*;',
                            lambda match: " " * len(match[0]), code)
            references = re.findall(r'#\[path\s*=\s*"([^"]+)"\]\s*(?:pub\s+)?mod\s+\w+\s*;', code)
            references += re.findall(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)', code)
            for module in re.findall(r"\b(?:pub\s+)?mod\s+(\w+)\s*;", masked):
                parent = Path(current).parent
                candidates = [parent / f"{module}.rs", parent / module / "mod.rs"]
                existing = [item for item in candidates if (root / item).is_file()]
                if len(existing) != 1:
                    findings.append(f"{current}: unresolved module {module}")
                else:
                    references.append(str(existing[0].relative_to(parent)))
            for reference in references:
                resolved = (root / current).parent.joinpath(reference).resolve()
                if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
                    findings.append(f"{current}: missing/escaping dependency {reference}")
                elif resolved.suffix == ".rs":
                    pending.append(resolved.relative_to(root.resolve()).as_posix())
                elif resolved.suffix == ".py":
                    findings.append(f"{current}: Python dependency {reference}")
            if re.search(r"include!\s*\(|include_(?:str|bytes)!\(\s*[^\s\"]", code):
                findings.append(f"{current}: dynamic include requires manual inventory")
        actual = rust_tests(root, name)
        if actual != sorted(entry["tests"]):
            findings.append(f"{name}: native test ID drift")
        result.append({"path": name, "tests": actual, "rust_dependencies": sorted(seen), "findings": sorted(set(findings))})
    return result


def audit(root, manifest):
    if manifest["schema_version"] != 1:
        raise ValueError("unsupported ledger schema")
    errors, units, expected_ids = [], [], set()
    transitions = {}
    accepted_paths = set()
    for reference in manifest["provenance"].get("accepted_source_transitions", []):
        name = reference["path"]
        path = local_file(root, name)
        if name in accepted_paths or digest(path) != reference["sha256"]:
            raise ValueError(f"duplicate or changed accepted source transition: {name}")
        accepted_paths.add(name)
        record = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
        if record["schema_version"] != 1 or record["approval_status"] != "parent_approved":
            raise ValueError(f"source transition is not parent approved: {name}")
        for change in record["files"]:
            source = change["path"]
            if source in transitions:
                raise ValueError(f"duplicate source transition: {source}")
            transitions[source] = (name, change)
    imports = python_inventory(root)
    active_oracle = manifest["provenance"].get("active_oracle")
    if active_oracle and digest(local_file(root, active_oracle["path"])) != active_oracle["sha256"]:
        errors.append("active oracle source drift")
    file_counts, categories = {}, Counter({key: 0 for key in CATEGORIES})
    for entry in manifest["files"]:
        name = entry["path"]
        if name in file_counts or not re.fullmatch(r"tests/test_[a-z0-9_]+\.py", name):
            raise ValueError(f"duplicate/invalid baseline file: {name}")
        path = local_file(root, name)
        actual = methods(path)
        baseline = [f"{cls}.{method}" for cls, names in entry["classes"].items() for method in names]
        if len(set(baseline)) != len(baseline):
            raise ValueError(f"duplicate baseline method: {name}")
        if name in transitions:
            reference, change = transitions[name]
            method_ids = [f"{Path(name).stem}.{key}" for key in baseline]
            changed_ids = change["method_ids"]
            if (entry.get("source_transition") != reference
                    or entry.get("original_sha256") != change["old_sha256"]
                    or entry["sha256"] != change["new_sha256"]
                    or change["old_sha256"] == change["new_sha256"]
                    or any(not re.fullmatch(r"[0-9a-f]{64}", change[key])
                           for key in ("old_sha256", "new_sha256"))
                    or not changed_ids or len(set(changed_ids)) != len(changed_ids)
                    or not set(changed_ids) <= set(method_ids)):
                errors.append(f"{name}: accepted source transition provenance mismatch")
        elif "original_sha256" in entry or "source_transition" in entry:
            errors.append(f"{name}: unaccepted source transition")
        if set(actual) != set(baseline):
            errors.append(f"{name}: method drift; missing={sorted(set(baseline) - set(actual))}; added={sorted(set(actual) - set(baseline))}")
        if digest(path) != entry["sha256"]:
            errors.append(f"{name}: assertion source drift; review before updating baseline hash")
        selected = {key: "D" for key in baseline}
        overridden = set()
        for override in entry.get("overrides", []):
            cls = override["class"]
            candidates = {key for key in baseline if key.startswith(cls + ".")}
            if "methods" in override:
                chosen = {f"{cls}.{method}" for method in override["methods"]}
            else:
                exclusions = {f"{cls}.{method}" for method in override["except"]}
                if not exclusions <= candidates:
                    raise ValueError(f"unknown exclusion in {name}")
                chosen = candidates - exclusions
            if not chosen or not chosen <= candidates or chosen & overridden or override["category"] not in ("A", "B"):
                raise ValueError(f"invalid/overlapping method selector: {name}")
            overridden.update(chosen)
            selected.update(dict.fromkeys(chosen, override["category"]))
        counts = Counter({key: 0 for key in CATEGORIES})
        for key in sorted(baseline):
            cls, method = key.split(".", 1)
            identity = f"python::{name}::{cls}::{method}"
            expected_ids.add(identity)
            category = selected[key]
            review = manifest.get("reviews", {}).get(identity, {})
            status = review.get("status", "unresolved")
            native = manifest.get("candidate_mappings", {}).get(identity, [])
            references = review.get("native_tests", [])
            fixtures = review.get("fixtures", [])
            if status not in ("unresolved", "mapped"):
                errors.append(f"{identity}: invalid review status")
            note = review.get("assertion_review", "")
            if status == "mapped" and (category == "D" or not isinstance(note, str) or not note.strip() or not references or (category == "A" and not fixtures)):
                errors.append(f"{identity}: mapping lacks assertion review/native tests/required A fixtures")
            for reference in native + references:
                parts = reference.split("::")
                if len(parts) != 3 or parts[0] != "rust" or parts[2] not in rust_tests(root, parts[1]):
                    errors.append(f"{identity}: absent native test {reference}")
            for fixture in fixtures:
                fixture_path = local_file(root, fixture["path"])
                if digest(fixture_path) != fixture["sha256"]:
                    errors.append(f"{identity}: fixture digest mismatch {fixture['path']}")
            counts[category] += 1
            units.append({"id": identity, "category": category, "status": status,
                          "obligation": manifest["default_obligation"], "candidate_native_tests": native,
                          "review": review, **imports[name]})
        if dict(counts) != entry["counts"]:
            errors.append(f"{name}: category count drift")
        file_counts[name] = dict(counts)
        categories.update(counts)
    if set(transitions) - set(file_counts):
        errors.append("accepted source transition references absent baseline files")
    supplemental_ids, supplemental_files = set(), {}
    for entry in manifest.get("supplemental_python", []):
        name = entry["path"]
        if name in file_counts or name in supplemental_files or not re.fullmatch(r"tests/test_[a-z0-9_]+\.py", name):
            raise ValueError(f"duplicate/invalid/overlapping supplemental file: {name}")
        if (not re.fullmatch(r"[0-9a-f]{7,40}", entry["source_main_commit"])
                or any(not re.fullmatch(r"[0-9a-f]{64}", entry[key])
                       for key in ("sha256", "original_main_sha256"))
                or not isinstance(entry["policy_transition_notes"], str)
                or not entry["policy_transition_notes"].strip()):
            raise ValueError(f"invalid supplemental provenance: {name}")
        baseline = [f"{cls}.{method}" for cls, names in entry["classes"].items() for method in names]
        if not baseline or len(set(baseline)) != len(baseline):
            raise ValueError(f"empty/duplicate supplemental method: {name}")
        identities = {f"python::{name}::{key.replace('.', '::', 1)}" for key in baseline}
        if identities & (expected_ids | supplemental_ids):
            raise ValueError(f"overlapping supplemental IDs: {name}")
        path = local_file(root, name)
        if set(methods(path)) != set(baseline):
            errors.append(f"{name}: supplemental method drift")
        if digest(path) != entry["sha256"]:
            errors.append(f"{name}: supplemental assertion source drift")
        supplemental_ids.update(identities)
        supplemental_files[name] = len(baseline)
    discovered = {f"python::{path.relative_to(root).as_posix()}::{key.replace('.', '::', 1)}"
                  for path in sorted(root.glob("tests/test_*.py")) for key in methods(path)}
    approved_ids = expected_ids | supplemental_ids
    if discovered != approved_ids:
        errors.append(f"original ID drift / supplemental inventory drift: missing={sorted(approved_ids - discovered)}, added={sorted(discovered - approved_ids)}")
    if len(expected_ids) != manifest["original_method_count"] or dict(categories) != manifest["category_counts"]:
        errors.append("original method/category totals drift")
    for name in ("reviews", "candidate_mappings"):
        if set(manifest.get(name, {})) - expected_ids:
            errors.append(f"unknown original IDs in {name}")
    b_ids = {unit["id"] for unit in units if unit["category"] == "B"}
    if set(manifest["candidate_mappings"]) != b_ids:
        errors.append("every B unit must have exactly its explicit candidate mapping entry")
    for name in manifest["independent_python"]:
        local_file(root, name)
        item = imports[name]
        if item["runner_dependencies"] or item["external_imports"] or item["dynamic_import_files"]:
            errors.append(f"C file is not independently stdlib-only: {name}")
    native = native_inventory(root, manifest["native_integration_inventory"])
    errors.extend(finding for item in native for finding in item["findings"])
    return {"schema_version": 1, "provenance": manifest["provenance"], "categories": dict(categories),
            "statuses": dict(sorted(Counter(unit["status"] for unit in units).items())),
            "file_counts": file_counts, "units": units, "independent_python": manifest["independent_python"],
            "supplemental_python": {"ids": sorted(supplemental_ids), "file_counts": supplemental_files,
                                    "method_count": len(supplemental_ids)},
            "full_python_method_count": len(discovered),
            "python_inventory": imports, "native_integration_inventory": native,
            "native_inventory_scope": "static integration-source inventory only; not execution or library-dependency proof",
            "errors": sorted(set(errors))}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--check", action="store_true", help="validate the frozen baseline and references")
    parser.add_argument("--summary", action="store_true", help="print human-readable counts")
    parser.add_argument("--json", action="store_true", help="print deterministic expanded obligation/import inventory")
    parser.add_argument("--require-complete", action="store_true", help="also reject every unresolved original obligation")
    args = parser.parse_args(argv)
    try:
        manifest = json.loads(local_file(args.root, MANIFEST).read_text(encoding="utf-8"), object_pairs_hook=unique_object)
        report = audit(args.root.resolve(), manifest)
    except (ValueError, OSError, KeyError, TypeError, SyntaxError) as error:
        print(f"retirement audit: {error}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(report, sort_keys=True, indent=2, ensure_ascii=True))
    if args.summary or not args.json:
        print(f"Original methods: {len(report['units'])} in {len(report['file_counts'])} files")
        supplemental = report["supplemental_python"]
        print(f"Supplemental Python methods: {supplemental['method_count']} in {len(supplemental['file_counts'])} files (not original mapping units)")
        print(f"Full Python suite methods: {report['full_python_method_count']}")
        print("Categories: " + " ".join(f"{key}={report['categories'][key]}" for key in CATEGORIES))
        print("Statuses: " + " ".join(f"{key}={value}" for key, value in report["statuses"].items()))
        print(f"Independent stdlib Python files (C): {len(report['independent_python'])}")
        native = report["native_integration_inventory"]
        print(f"Candidate native integration inventory: {len(native)} targets, {sum(len(item['tests']) for item in native)} test IDs")
        print(report["native_inventory_scope"])
    for error in report["errors"]:
        print(error, file=sys.stderr)
    if args.require_complete and any(unit["status"] != "mapped" or unit["category"] == "D" for unit in report["units"]):
        print("retirement incomplete: unresolved A/B/D obligations remain; candidate mappings are not assertion reviews", file=sys.stderr)
        return 1
    return int(bool(report["errors"]))


if __name__ == "__main__":
    raise SystemExit(main())
