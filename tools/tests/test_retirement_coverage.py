"""Independent audit tests, intentionally outside tests/ discovery."""

import ast
import contextlib
import copy
import functools
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import retirement_coverage as coverage


class RetirementCoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = json.loads((coverage.ROOT / coverage.MANIFEST).read_text(), object_pairs_hook=coverage.unique_object)
        cls.report = coverage.audit(coverage.ROOT, cls.manifest)

    def test_original_ids_categories_and_unresolved_status_are_frozen(self):
        self.assertEqual(self.report["errors"], [])
        self.assertEqual(len(self.report["units"]), 941)
        self.assertEqual(len({unit["id"] for unit in self.report["units"]}), 941)
        self.assertEqual(len(self.report["file_counts"]), 44)
        self.assertEqual(self.report["categories"], {"A": 53, "B": 15, "C": 0, "D": 873})
        self.assertEqual(self.report["statuses"], {"mapped": 67, "unresolved": 874})
        self.assertEqual(len(self.report["independent_python"]), 5)
        views = [unit for unit in self.report["units"] if "::tests/test_session_views.py::" in unit["id"]]
        self.assertEqual(len(views), 19)
        self.assertEqual(sum(unit["status"] == "mapped" for unit in views), 18)
        self.assertEqual([unit["category"] for unit in views if unit["status"] == "unresolved"], ["D"])
        batches = self.manifest["provenance"]["accepted_batches"]
        self.assertEqual([batch["path"] for batch in batches], [
            f"tests/retirement/reviews/{name}.json"
            for name in ["records", "checkpoints", "task_graphs", "session_prompts"]
        ])
        expected = {
            "tests/test_records.py": (4, 0, {"A": 4, "B": 0, "C": 0, "D": 0}),
            "tests/test_checkpoints.py": (16, 2, {"A": 16, "B": 0, "C": 0, "D": 2}),
            "tests/test_task_graphs.py": (14, 4, {"A": 15, "B": 0, "C": 0, "D": 3}),
            "tests/test_session_prompts.py": (15, 6, {"A": 0, "B": 15, "C": 0, "D": 6}),
        }
        for name, (mapped, unresolved, categories) in expected.items():
            units = [unit for unit in self.report["units"] if unit["id"].split("::")[1] == name]
            self.assertEqual(sum(unit["status"] == "mapped" for unit in units), mapped)
            self.assertEqual(sum(unit["status"] == "unresolved" for unit in units), unresolved)
            self.assertEqual(self.report["file_counts"][name], categories)
        reviewed_files = set(expected) | {"tests/test_session_views.py"}
        untouched = [unit for unit in self.report["units"] if unit["id"].split("::")[1] not in reviewed_files]
        self.assertEqual(len(untouched), 861)
        self.assertTrue(all(unit["status"] == "unresolved" and not unit["review"]
                            and not unit["candidate_native_tests"] for unit in untouched))
        self.assertEqual(len(self.manifest["reviews"]), 80)
        for batch in batches:
            path = coverage.ROOT / batch["path"]
            self.assertEqual(coverage.digest(path), batch["sha256"])
            fragment = json.loads(path.read_text(), object_pairs_hook=coverage.unique_object)
            self.assertEqual(batch["metadata"], {key: value for key, value in fragment.items() if key != "reviews"})
            for identity, original in fragment["reviews"].items():
                normalized = copy.deepcopy(original)
                for index, reference in enumerate(original.get("native_tests", [])):
                    parts = reference.split("::")
                    self.assertEqual(coverage.rust_tests(coverage.ROOT, parts[1]).count(parts[-1]), 1)
                    if len(parts) != 3:
                        self.assertEqual(parts[2:-1], ["unix"])
                        normalized["native_tests"][index] = "::".join([parts[0], parts[1], parts[-1]])
                self.assertEqual(self.manifest["reviews"][identity], normalized)
        graph_gap = next(unit for unit in self.report["units"]
                         if unit["id"].endswith("::test_malformed_current_enrollments_fail_closed"))
        self.assertEqual((graph_gap["category"], graph_gap["status"]), ("A", "unresolved"))
        self.assertTrue({"g0076", "g0077"} <= set(graph_gap["review"]["cases"]))
        provenance = json.loads((coverage.ROOT / "tests/retirement/fixtures/session_views_provenance.json").read_text())
        corpus_path = coverage.ROOT / provenance["corpus"]["path"]
        corpus = json.loads(corpus_path.read_text())
        self.assertEqual(coverage.digest(corpus_path), provenance["corpus"]["sha256"])
        self.assertEqual(set(provenance["methods"]), {unit["id"] for unit in views})
        cases = [case for method in provenance["methods"].values() for case in method["edge_cases"]]
        self.assertEqual(len(cases), 127)
        self.assertEqual(set(cases), {case["id"] for case in corpus["cases"]})
        self.assertEqual(len(set(cases)), len(cases))

    def test_fifteen_b_candidates_have_explicit_reviewed_native_test_ids(self):
        units = [unit for unit in self.report["units"] if unit["category"] == "B"]
        self.assertEqual(len(units), 15)
        self.assertEqual(set(self.manifest["candidate_mappings"]), {unit["id"] for unit in units})
        prior = {
            "test_capabilities_query_is_metadata_only": "capabilities_are_workspace_independent",
            "test_prompt_file_bounds_utf8_and_symlink_rejection": "strict_permissions_and_prompt_bounds_prevent_contact",
            "test_orphaned_authority_blocks_before_io": "enabled_invalid_and_orphan_launch_authority_block_before_contact",
            "test_transport_gate_held_but_global_workspace_lock_free_during_native_io": "transport_gate_is_held_and_workspace_lock_is_free_during_io",
        }
        for unit in units:
            self.assertEqual(unit["status"], "mapped")
            self.assertTrue(unit["review"]["assertion_review"].strip())
            self.assertEqual(unit["candidate_native_tests"], unit["review"]["native_tests"])
            method = unit["id"].split("::")[-1]
            if method in prior:
                self.assertIn(f"rust::tests/session_prompts_workflow.rs::{prior[method]}",
                              unit["candidate_native_tests"])
            for reference in unit["candidate_native_tests"]:
                kind, path, function = reference.split("::")
                self.assertEqual(kind, "rust")
                self.assertEqual(coverage.rust_tests(coverage.ROOT, path).count(function), 1)

    def test_indirect_runner_imports_are_not_independent(self):
        inventory = self.report["python_inventory"]
        for name in ["tests/_legacy_lifecycle_parity.py", "tests/_checkpoint_guidance_parity.py"]:
            self.assertFalse(inventory[name]["direct_runner_imports"])
            self.assertIn("runner/coven.py", inventory[name]["runner_dependencies"])
        for name in self.manifest["independent_python"]:
            self.assertEqual(inventory[name]["runner_dependencies"], [])
            self.assertEqual(inventory[name]["external_imports"], [])

    def test_native_inventory_is_explicit_and_has_no_static_findings(self):
        native = self.report["native_integration_inventory"]
        self.assertEqual(len(native), 7)
        self.assertEqual(sum(len(item["tests"]) for item in native), 54)
        prompts = next(item for item in native if item["path"] == "tests/session_prompts_workflow.rs")
        self.assertEqual(len(prompts["tests"]), 19)
        self.assertTrue({
            "all_original_malformed_ack_cases_are_uncertain_without_retry",
            "leaf_socket_replacement_after_capabilities_contacts_neither_submit_endpoint",
            "original_invalid_argument_cases_refuse_without_endpoint_contact",
            "literal_receipt_contract_keeps_prompt_and_peer_extensions_private",
        } <= set(prompts["tests"]))
        self.assertTrue(all(not item["findings"] for item in native))
        self.assertTrue(any("tests/support/session_readers.rs" in item["rust_dependencies"] for item in native))
        self.assertIn("not execution", self.report["native_inventory_scope"])

    def test_cli_check_passes_but_complete_deliberately_fails(self):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(coverage.main(["--check", "--summary"]), 0)
            self.assertEqual(coverage.main(["--require-complete"]), 1)
        self.assertIn("A=53 B=15 C=0 D=873", out.getvalue())
        self.assertIn("mapped=67 unresolved=874", out.getvalue())
        self.assertIn("Original methods: 941", out.getvalue())
        self.assertIn("Supplemental Python methods: 5 in 1 files", out.getvalue())
        self.assertIn("Full Python suite methods: 946", out.getvalue())
        self.assertIn("retirement incomplete", err.getvalue())

    def test_report_is_deterministic_and_never_imports_runner(self):
        original = __import__

        def guarded(name, *args, **kwargs):
            if name == "runner" or name.startswith("runner.") or name in {"coven", "storage"}:
                raise AssertionError(f"audit imported production code: {name}")
            return original(name, *args, **kwargs)

        with mock.patch("builtins.__import__", side_effect=guarded):
            second = coverage.audit(coverage.ROOT, self.manifest)
        self.assertEqual(json.dumps(self.report, sort_keys=True), json.dumps(second, sort_keys=True))

    def test_cli_json_expands_all_stable_ids_without_a_summary_prefix(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(coverage.main(["--json"]), 0)
        report = json.loads(out.getvalue())
        self.assertEqual(report["units"], self.report["units"])
        self.assertEqual(report["errors"], [])

    def test_missing_candidate_and_unknown_review_ids_fail(self):
        manifest = copy.deepcopy(self.manifest)
        identity = next(iter(manifest["candidate_mappings"]))
        manifest["candidate_mappings"][identity] = ["rust::tests/session_prompts_workflow.rs::absent"]
        manifest["reviews"]["python::absent::Class::test_absent"] = {"status": "mapped"}
        errors = coverage.audit(coverage.ROOT, manifest)["errors"]
        self.assertTrue(any("absent native test" in error for error in errors))
        self.assertIn("unknown original IDs in reviews", errors)

    def test_mapped_requires_assertion_review_native_tests_and_a_fixtures(self):
        manifest = copy.deepcopy(self.manifest)
        identity = next(unit["id"] for unit in self.report["units"] if unit["category"] == "A")
        manifest["reviews"][identity] = {"status": "mapped"}
        self.assertTrue(any("mapping lacks" in error for error in coverage.audit(coverage.ROOT, manifest)["errors"]))
        manifest["reviews"][identity] = {
            "status": "unresolved", "fixtures": [{"path": "tests/retirement/absent.json", "sha256": "0" * 64}],
        }
        with self.assertRaisesRegex(ValueError, "missing or escaping"):
            coverage.audit(coverage.ROOT, manifest)

    def test_fixture_hash_and_duplicate_method_selectors_fail(self):
        manifest = copy.deepcopy(self.manifest)
        identity = next(unit["id"] for unit in self.report["units"] if unit["category"] == "A")
        manifest["reviews"][identity] = {
            "fixtures": [{"path": coverage.MANIFEST, "sha256": "0" * 64}],
        }
        self.assertTrue(any("fixture digest mismatch" in error for error in coverage.audit(coverage.ROOT, manifest)["errors"]))
        entry = next(entry for entry in manifest["files"] if entry["path"] == "tests/test_records.py")
        self.assertEqual(entry["overrides"], [{
            "class": "RecordTests", "category": "A", "methods": entry["classes"]["RecordTests"],
        }])
        entry["overrides"].append(copy.deepcopy(entry["overrides"][0]))
        with self.assertRaisesRegex(ValueError, "overlapping"):
            coverage.audit(coverage.ROOT, manifest)

    def test_baseline_cannot_silently_shrink_or_reclassify(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["files"][0]["classes"]["ActionCommandTests"].pop()
        errors = coverage.audit(coverage.ROOT, manifest)["errors"]
        self.assertTrue(any("method drift" in error for error in errors))
        self.assertTrue(any("original method/category totals drift" in error for error in errors))

    def test_invalid_json_and_repository_escape_fail(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            json.loads('{"schema_version":1,"schema_version":2}', object_pairs_hook=coverage.unique_object)
        for name in ["../outside", "/etc/passwd", "tests/retirement/absent"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                coverage.local_file(coverage.ROOT, name)

    def test_fragment_file_alone_cannot_accept_or_change_global_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reviews = root / "tests/retirement/reviews"
            reviews.mkdir(parents=True)
            source = root / "tests/test_sample.py"
            source.write_text("class Sample:\n    def test_original(self):\n        assert True\n")
            (root / "tests/native.rs").write_text("#[test] fn substitute() {}\n")
            identity = "python::tests/test_sample.py::Sample::test_original"
            native = ["rust::tests/native.rs::substitute"]
            manifest = {
                "schema_version": 1, "original_method_count": 1,
                "category_counts": {"A": 0, "B": 1, "C": 0, "D": 0},
                "provenance": {}, "default_obligation": "unresolved",
                "candidate_mappings": {identity: native}, "independent_python": [],
                "native_integration_inventory": [], "reviews": {},
                "files": [{"path": "tests/test_sample.py", "sha256": coverage.digest(source),
                           "classes": {"Sample": ["test_original"]},
                           "counts": {"A": 0, "B": 1, "C": 0, "D": 0},
                           "overrides": [{"class": "Sample", "category": "B", "methods": ["test_original"]}]}],
            }
            before = coverage.audit(root, manifest)
            review = {"status": "mapped", "assertion_review": "Explicit reviewed substitute.", "native_tests": native}
            (reviews / "newly_approved.json").write_text(json.dumps({"reviews": {identity: review}}))
            (root / coverage.MANIFEST).write_text(json.dumps(manifest))
            self.assertEqual(coverage.audit(root, manifest), before)
            self.assertEqual(before["errors"], [])
            self.assertEqual(before["statuses"], {"unresolved": 1})
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(coverage.main(["--root", directory, "--check"]), 0)
                self.assertEqual(coverage.main(["--root", directory, "--require-complete"]), 1)
            manifest["reviews"][identity] = review
            accepted = coverage.audit(root, manifest)
            self.assertEqual(accepted["errors"], [])
            self.assertEqual(accepted["statuses"], {"mapped": 1})

    def test_c_cannot_be_declared_for_a_transitively_runner_dependent_file(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["independent_python"].append("tests/_legacy_lifecycle_parity.py")
        self.assertTrue(any("C file is not independently" in error for error in coverage.audit(coverage.ROOT, manifest)["errors"]))

    def test_ast_closure_handles_cycles_aliases_and_child_scripts_without_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests").mkdir()
            (root / "runner").mkdir()
            (root / "tests/helper.py").write_text("from test_example import Example\nfrom runner import storage as s\n")
            (root / "tests/test_example.py").write_text("import helper\nclass Example: pass\n")
            (root / "tests/child.py").write_text('code = "import storage; raise RuntimeError()"\n')
            (root / "tests/dynamic.py").write_text('code = "from runner import " + name\n')
            (root / "runner/relative.py").write_text("from .storage import example\n")
            (root / "runner/storage.py").write_text("raise RuntimeError('never import me')\n")
            inventory = coverage.python_inventory(root)
            self.assertEqual(inventory["tests/test_example.py"]["runner_dependencies"], ["runner/storage.py"])
            self.assertEqual(inventory["tests/child.py"]["runner_dependencies"], ["runner/storage.py"])
            self.assertIn("runner/storage.py", inventory["runner/relative.py"]["runner_dependencies"])
            self.assertEqual(inventory["tests/dynamic.py"]["dynamic_import_files"], ["tests/dynamic.py"])

    def test_source_drift_is_detected_even_without_method_id_changes(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["files"][0]["sha256"] = "0" * 64
        self.assertTrue(any("assertion source drift" in error for error in coverage.audit(coverage.ROOT, manifest)["errors"]))

    def test_supplemental_cohort_has_exact_five_actual_ids_without_original_mappings(self):
        supplemental = self.report["supplemental_python"]
        entry, = self.manifest["supplemental_python"]
        actual = coverage.methods(coverage.ROOT / entry["path"])
        self.assertEqual(supplemental["method_count"], 5)
        self.assertEqual(supplemental["file_counts"], {"tests/test_wrapper.py": 5})
        self.assertEqual(supplemental["ids"], sorted(
            f"python::{entry['path']}::{key.replace('.', '::', 1)}" for key in actual))
        self.assertEqual(self.report["full_python_method_count"], 946)
        self.assertEqual(coverage.digest(coverage.ROOT / entry["path"]), entry["sha256"])
        self.assertEqual(entry["source_main_commit"], "649324cda16915a4a5cf8e9746f02c32f6b77f59")
        self.assertEqual(entry["original_main_sha256"],
                         "30c204782c1f87be53b35b9fe805f8935724bb6cdef9b5269e32d6fe26c712fc")
        self.assertFalse(set(supplemental["ids"]) & {unit["id"] for unit in self.report["units"]})
        for name in ("reviews", "candidate_mappings"):
            manifest = copy.deepcopy(self.manifest)
            manifest[name][supplemental["ids"][0]] = {} if name == "reviews" else []
            self.assertIn(f"unknown original IDs in {name}", coverage.audit(coverage.ROOT, manifest)["errors"])

    def test_unapproved_supplemental_file_is_not_silently_dropped(self):
        manifest = copy.deepcopy(self.manifest)
        manifest.pop("supplemental_python")
        errors = coverage.audit(coverage.ROOT, manifest)["errors"]
        self.assertTrue(any("inventory drift" in error and "tests/test_wrapper.py" in error for error in errors))

    def test_supplemental_new_and_renamed_methods_fail_even_with_same_count(self):
        original_methods = coverage.methods
        for rename in (False, True):
            def changed_methods(path):
                actual = original_methods(path)
                if path.name == "test_wrapper.py":
                    if rename:
                        actual.pop(next(iter(actual)))
                    actual["WrapperTests.test_unapproved"] = None
                return actual

            with self.subTest(rename=rename), mock.patch.object(coverage, "methods", side_effect=changed_methods):
                errors = coverage.audit(coverage.ROOT, self.manifest)["errors"]
            self.assertIn("tests/test_wrapper.py: supplemental method drift", errors)
            self.assertTrue(any("inventory drift" in error for error in errors))

    def test_supplemental_source_assertion_drift_fails_without_id_changes(self):
        entry, = self.manifest["supplemental_python"]
        original_local_file = coverage.local_file
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test_wrapper.py"
            tree = ast.parse((coverage.ROOT / entry["path"]).read_text())
            method = next(node for cls in tree.body if isinstance(cls, ast.ClassDef)
                          for node in cls.body if isinstance(node, ast.FunctionDef)
                          and node.name.startswith("test_"))
            method.body.append(ast.Assert(test=ast.Constant(value=False)))
            path.write_text(ast.unparse(ast.fix_missing_locations(tree)))
            with mock.patch.object(coverage, "local_file", side_effect=lambda root, name:
                                   path if name == entry["path"] else original_local_file(root, name)):
                errors = coverage.audit(coverage.ROOT, self.manifest)["errors"]
        self.assertEqual(errors, ["tests/test_wrapper.py: supplemental assertion source drift"])

    def test_supplemental_duplicates_and_original_file_overlap_fail(self):
        for kind in ("file", "method", "original"):
            manifest = copy.deepcopy(self.manifest)
            entry = manifest["supplemental_python"][0]
            if kind == "file":
                manifest["supplemental_python"].append(copy.deepcopy(entry))
            elif kind == "method":
                entry["classes"]["WrapperTests"].append(entry["classes"]["WrapperTests"][0])
            else:
                entry["path"] = manifest["files"][0]["path"]
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, "duplicate|overlapping"):
                coverage.audit(coverage.ROOT, manifest)

    def test_supplemental_provenance_is_required(self):
        for key in ("source_main_commit", "original_main_sha256", "sha256", "policy_transition_notes"):
            manifest = copy.deepcopy(self.manifest)
            manifest["supplemental_python"][0][key] = ""
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "supplemental provenance"):
                coverage.audit(coverage.ROOT, manifest)

    def test_active_oracle_hash_does_not_replace_historical_capture_hash(self):
        provenance = self.manifest["provenance"]
        self.assertEqual(provenance["oracle_coven_sha256"],
                         "f1303a96c043820913855a7f5b5361e0cbcb0d66ecd9e5b207d417f69c21fb36")
        self.assertNotEqual(provenance["oracle_coven_sha256"], provenance["active_oracle"]["sha256"])
        manifest = copy.deepcopy(self.manifest)
        manifest["provenance"]["active_oracle"]["sha256"] = "0" * 64
        self.assertIn("active oracle source drift", coverage.audit(coverage.ROOT, manifest)["errors"])

    def test_eleven_approved_transitions_preserve_original_hashes_and_ids(self):
        self.enterContext(mock.patch.object(coverage, "rust_tests",
                                           side_effect=functools.lru_cache()(coverage.rust_tests)))
        references = self.manifest["provenance"]["accepted_source_transitions"]
        self.assertEqual(len(references), 1)
        reference = references[0]
        path = coverage.ROOT / reference["path"]
        self.assertEqual(coverage.digest(path), reference["sha256"])
        record = json.loads(path.read_text(), object_pairs_hook=coverage.unique_object)
        self.assertEqual(record["approval_status"], "parent_approved")
        changes = {change["path"]: change for change in record["files"]}
        self.assertEqual(len(record["files"]), 11)
        self.assertEqual(len(changes), 11)
        transitioned = [entry for entry in self.manifest["files"] if "original_sha256" in entry]
        self.assertEqual({entry["path"] for entry in transitioned}, set(changes))
        self.assertEqual(len(self.manifest["files"]) - len(transitioned), 33)
        self.assertEqual(self.manifest["provenance"]["source_head"],
                         "e389a08669204d4e51a49ffd13cbe01b90b5d8ba")
        for entry in transitioned:
            with self.subTest(path=entry["path"]):
                change = changes[entry["path"]]
                self.assertEqual(entry["source_transition"], reference["path"])
                self.assertEqual(entry["original_sha256"], change["old_sha256"])
                self.assertNotEqual(entry["original_sha256"], entry["sha256"])
                self.assertEqual(entry["sha256"], change["new_sha256"])
                self.assertEqual(coverage.digest(coverage.ROOT / entry["path"]), entry["sha256"])
                self.assertEqual(len(change["method_ids"]), 1)
                self.assertIn(change["method_ids"][0], [
                    f"{Path(entry['path']).stem}.{cls}.{method}"
                    for cls, names in entry["classes"].items() for method in names
                ])
                for field in ("original_sha256", "sha256", "source_transition"):
                    manifest = copy.deepcopy(self.manifest)
                    target = next(item for item in manifest["files"] if item["path"] == entry["path"])
                    target.pop(field)
                    if field == "sha256":
                        target[field] = "0" * 64
                    self.assertIn(f"{entry['path']}: accepted source transition provenance mismatch",
                                  coverage.audit(coverage.ROOT, manifest)["errors"])

    def test_approved_transition_never_allows_unexpected_ast_or_rewrites(self):
        self.enterContext(mock.patch.object(coverage, "rust_tests",
                                           side_effect=functools.lru_cache()(coverage.rust_tests)))
        before = copy.deepcopy(self.manifest)
        original_local_file = coverage.local_file
        ledger_hash = coverage.digest(coverage.ROOT / coverage.MANIFEST)
        for entry in self.manifest["files"]:
            if "original_sha256" not in entry:
                continue
            with self.subTest(path=entry["path"]), tempfile.TemporaryDirectory() as directory:
                source = coverage.ROOT / entry["path"]
                source_hash = coverage.digest(source)
                tree = ast.parse(source.read_text())
                method = next(node for cls in tree.body if isinstance(cls, ast.ClassDef)
                              for node in cls.body if isinstance(node, ast.FunctionDef)
                              and node.name.startswith("test_"))
                method.body.append(ast.Assert(test=ast.Constant(value=False)))
                changed = Path(directory) / source.name
                changed.write_text(ast.unparse(ast.fix_missing_locations(tree)))

                def redirect(root, name):
                    return changed if name == entry["path"] else original_local_file(root, name)

                with mock.patch.object(coverage, "local_file", side_effect=redirect):
                    errors = coverage.audit(coverage.ROOT, self.manifest)["errors"]
                self.assertEqual(errors, [f"{entry['path']}: assertion source drift; review before updating baseline hash"])
                self.assertEqual(coverage.digest(source), source_hash)
        self.assertEqual(self.manifest, before)
        self.assertEqual(coverage.digest(coverage.ROOT / coverage.MANIFEST), ledger_hash)

    def test_transition_record_hash_and_approval_are_required(self):
        manifest = copy.deepcopy(self.manifest)
        reference = manifest["provenance"]["accepted_source_transitions"][0]
        reference["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "changed accepted source transition"):
            coverage.audit(coverage.ROOT, manifest)
        manifest["provenance"]["accepted_source_transitions"] = []
        self.assertEqual(sum("unaccepted source transition" in error
                             for error in coverage.audit(coverage.ROOT, manifest)["errors"]), 11)
        manifest = copy.deepcopy(self.manifest)
        reference = manifest["provenance"]["accepted_source_transitions"][0]
        record = json.loads((coverage.ROOT / reference["path"]).read_text())
        record["approval_status"] = "pending_parent_diff_review"
        with tempfile.TemporaryDirectory() as directory:
            changed = Path(directory) / "transition.json"
            changed.write_text(json.dumps(record))
            reference["sha256"] = coverage.digest(changed)
            original_local_file = coverage.local_file
            with mock.patch.object(coverage, "local_file", side_effect=lambda root, name:
                                   changed if name == reference["path"] else original_local_file(root, name)):
                with self.assertRaisesRegex(ValueError, "not parent approved"):
                    coverage.audit(coverage.ROOT, manifest)

    def test_same_count_method_rename_still_fails_against_original_ids(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests").mkdir()
            path = root / "tests/test_sample.py"
            path.write_text("class Sample:\n    def test_original(self):\n        assert True\n")
            manifest = {
                "schema_version": 1, "original_method_count": 1,
                "category_counts": {"A": 0, "B": 0, "C": 0, "D": 1},
                "provenance": {}, "default_obligation": "unresolved",
                "candidate_mappings": {}, "independent_python": [], "native_integration_inventory": [],
                "files": [{"path": "tests/test_sample.py", "sha256": coverage.digest(path),
                           "classes": {"Sample": ["test_original"]},
                           "counts": {"A": 0, "B": 0, "C": 0, "D": 1}}],
            }
            self.assertEqual(coverage.audit(root, manifest)["errors"], [])
            path.write_text("class Sample:\n    def test_replacement(self):\n        assert True\n")
            errors = coverage.audit(root, manifest)["errors"]
            self.assertTrue(any("method drift" in error for error in errors))
            self.assertTrue(any("original ID drift" in error for error in errors))

    def test_rust_inventory_checks_transitive_helpers_and_ignores_comments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests").mkdir()
            (root / "tests/native.rs").write_text(
                '// Command::new("python3");\n/* nested /* comment */ "python3" */\n'
                '#[path = "helper.rs"] mod fixture;\n#[test] fn native() {}\n'
            )
            helper = root / "tests/helper.rs"
            helper.write_text('fn example() { Command::new("python3"); }\n')
            entry = {"path": "tests/native.rs", "tests": ["native"]}
            report = coverage.native_inventory(root, [entry])[0]
            self.assertEqual(report["rust_dependencies"], ["tests/helper.rs", "tests/native.rs"])
            self.assertEqual(len(report["findings"]), 1)
            self.assertIn("tests/helper.rs", report["findings"][0])
            helper.write_text("// python3 only in a comment\n")
            self.assertEqual(coverage.native_inventory(root, [entry])[0]["findings"], [])
            entry["tests"] = ["renamed"]
            self.assertIn("tests/native.rs: native test ID drift", coverage.native_inventory(root, [entry])[0]["findings"])

    def test_rust_raw_string_cannot_manufacture_a_native_test_id(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tests").mkdir()
            (root / "tests/native.rs").write_text(
                'const TEXT: &str = r###"#[test] fn fake() { /* not a comment */ }"###;\n'
                'const QUOTE: u8 = b\'"\';\n'
                '#[test] fn real() {}\n'
            )
            self.assertEqual(coverage.rust_tests(root, "tests/native.rs"), ["real"])


if __name__ == "__main__":
    unittest.main()
