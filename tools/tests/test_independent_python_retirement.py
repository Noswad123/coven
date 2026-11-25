"""Synthetic helper retirements fail closed without executing historical Python."""

import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import retirement_coverage as coverage


class IndependentPythonRetirementTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        (self.root / "src").mkdir()
        (self.root / "src/active.py").write_text("import json\n")
        (self.root / "src/native.rs").write_text("#[test] fn replacement() {}\n")
        self.fixture = self.root / "tests/retirement/fixtures/evidence.json"
        self.fixture.parent.mkdir(parents=True)
        self.fixture.write_text('{"captured": true}\n')
        self.path = "src/retired.py"
        self.review = self.root / "tests/retirement/reviews/helper.json"
        self.review.parent.mkdir()
        self.entry = {"path": self.path, "sha256": "a" * 64,
                      "review": {"path": "tests/retirement/reviews/helper.json", "sha256": "0" * 64}}
        self.record = {
            "schema_version": 1, "approval_status": "parent_approved", "mapping_delta": 0,
            "historical_commit": "b" * 40,
            "retired_helpers": {self.path: {
                "sha256": self.entry["sha256"], "kind": "independent_stdlib_helper",
                "replacement_tests": ["rust::src/native.rs::replacement"],
                "fixtures": [{"path": "tests/retirement/fixtures/evidence.json",
                              "sha256": coverage.digest(self.fixture)}],
            }},
        }
        self.manifest = {
            "schema_version": 1, "provenance": {}, "files": [], "original_method_count": 0,
            "category_counts": dict.fromkeys(coverage.CATEGORIES, 0), "candidate_mappings": {},
            "independent_python": ["src/active.py"],
            "independent_python_baseline": ["src/active.py", self.path],
            "retired_independent_python": [self.entry], "native_integration_inventory": [],
        }
        self.pin_record()

    def pin_record(self):
        self.review.write_text(json.dumps(self.record))
        self.entry["review"]["sha256"] = coverage.digest(self.review)

    def test_approved_history_reports_verified_evidence_without_method_mappings(self):
        before = copy.deepcopy(self.manifest)
        report = coverage.audit(self.root, self.manifest)
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["units"], [])
        self.assertEqual(report["categories"], dict.fromkeys(coverage.CATEGORIES, 0))
        self.assertEqual(report["retired_independent_python"], [{
            **self.entry, "historical_commit": self.record["historical_commit"], "mapping_delta": 0,
            **self.record["retired_helpers"][self.path],
        }])
        self.assertEqual(self.manifest, before)
        self.assertEqual(coverage.audit(self.root, self.manifest), report)

    def test_baseline_required_for_retired_or_historical_oracle_registry(self):
        for historical, retired in [(False, True), (True, False), (True, True)]:
            manifest = copy.deepcopy(self.manifest)
            manifest.pop("independent_python_baseline")
            if historical:
                manifest["provenance"]["oracle_coven_sha256"] = "c" * 64
            if not retired:
                manifest.pop("retired_independent_python")
            with self.subTest(historical=historical, retired=retired), self.assertRaisesRegex(
                    ValueError, "missing independent Python baseline"):
                coverage.independent_python_history(self.root, manifest)
        manifest = copy.deepcopy(self.manifest)
        manifest.pop("retired_independent_python")
        manifest.pop("independent_python_baseline")
        self.assertEqual(coverage.audit(self.root, manifest)["errors"], [])

    def test_removing_retirement_link_or_active_helper_cannot_shrink_history(self):
        for change in ("retired entry", "retired registry", "active entry", "new helper", "omitted baseline"):
            manifest = copy.deepcopy(self.manifest)
            if change == "retired entry":
                manifest["retired_independent_python"] = []
            elif change == "retired registry":
                manifest.pop("retired_independent_python")
            elif change == "active entry":
                manifest["independent_python"] = []
            elif change == "new helper":
                manifest["independent_python"].append("src/new.py")
            else:
                manifest["independent_python_baseline"].remove(self.path)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "registry drift"):
                coverage.independent_python_history(self.root, manifest)

    def test_registry_paths_are_unique_disjoint_and_safe(self):
        for key in ("independent_python", "independent_python_baseline", "retired_independent_python"):
            manifest = copy.deepcopy(self.manifest)
            manifest[key].append(copy.deepcopy(manifest[key][0]))
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "duplicate"):
                coverage.independent_python_history(self.root, manifest)
        manifest = copy.deepcopy(self.manifest)
        manifest["independent_python"].append(self.path)
        with self.assertRaisesRegex(ValueError, "overlap"):
            coverage.independent_python_history(self.root, manifest)
        for name in ("../escape.py", "/escape.py", "src/../escape.py", "src//retired.py", "src/retired.rs"):
            manifest = copy.deepcopy(self.manifest)
            manifest["retired_independent_python"][0]["path"] = name
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "unsafe"):
                coverage.independent_python_history(self.root, manifest)

    def test_missing_pending_and_tampered_review_fail(self):
        for status in (None, "pending_parent_review", "pending_parent_diff_review"):
            self.record["approval_status"] = status
            self.pin_record()
            with self.subTest(status=status), self.assertRaisesRegex(ValueError, "not parent approved"):
                coverage.independent_python_history(self.root, self.manifest)
        self.review.write_text("{}")
        with self.assertRaisesRegex(ValueError, "review digest mismatch"):
            coverage.independent_python_history(self.root, self.manifest)
        self.review.unlink()
        with self.assertRaisesRegex(ValueError, "missing or escaping"):
            coverage.independent_python_history(self.root, self.manifest)

    def test_review_provenance_and_helper_linkage_cannot_be_rehashed_away(self):
        original = copy.deepcopy(self.record)
        for change in ("schema", "commit", "short commit", "mapping delta", "bool delta", "path", "hash", "kind"):
            self.record = copy.deepcopy(original)
            if change == "schema":
                self.record["schema_version"] = True
            elif change in ("commit", "short commit"):
                self.record["historical_commit"] = "Z" * 40 if change == "commit" else "b" * 7
            elif change in ("mapping delta", "bool delta"):
                self.record["mapping_delta"] = 1 if change == "mapping delta" else False
            elif change == "path":
                self.record["retired_helpers"]["src/wrong.py"] = self.record["retired_helpers"].pop(self.path)
            elif change == "hash":
                self.record["retired_helpers"][self.path]["sha256"] = "c" * 64
            else:
                self.record["retired_helpers"][self.path]["kind"] = "runner_oracle"
            self.pin_record()
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "provenance|linkage mismatch"):
                coverage.independent_python_history(self.root, self.manifest)

    def test_review_reference_source_hash_and_record_shape_are_required(self):
        for value in (None, {}, {"path": "../escape.json", "sha256": "0" * 64},
                      {"path": "tests/retirement/reviews/helper.json", "sha256": "bad"}):
            manifest = copy.deepcopy(self.manifest)
            manifest["retired_independent_python"][0]["review"] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "review reference"):
                coverage.independent_python_history(self.root, manifest)
        for value in (None, "", "Z" * 64):
            manifest = copy.deepcopy(self.manifest)
            manifest["retired_independent_python"][0]["sha256"] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "source hash"):
                coverage.independent_python_history(self.root, manifest)
        self.record["retired_helpers"] = []
        self.pin_record()
        with self.assertRaisesRegex(ValueError, "linkage mismatch"):
            coverage.independent_python_history(self.root, self.manifest)

    def test_replacement_ids_are_real_unique_and_exactly_three_parts(self):
        helper = self.record["retired_helpers"][self.path]
        for tests in ([], ["rust::src/native.rs::absent"], ["rust::src/native.rs::module::replacement"],
                      ["python::src/native.rs::replacement"], ["rust::src/native.rs::replacement"] * 2):
            helper["replacement_tests"] = tests
            self.pin_record()
            with self.subTest(tests=tests), self.assertRaisesRegex(ValueError, "replacement"):
                coverage.independent_python_history(self.root, self.manifest)

    def test_missing_tampered_and_escaping_fixtures_fail(self):
        helper = self.record["retired_helpers"][self.path]
        original = copy.deepcopy(helper["fixtures"])
        for fixtures in ([], [{"path": "../escape", "sha256": "0" * 64}],
                         [{"path": "tests/retirement/fixtures/absent.json", "sha256": "0" * 64}]):
            helper["fixtures"] = fixtures
            self.pin_record()
            with self.subTest(fixtures=fixtures), self.assertRaisesRegex(ValueError, "fixture|unsafe|missing"):
                coverage.independent_python_history(self.root, self.manifest)
        helper["fixtures"] = original
        self.pin_record()
        self.fixture.write_text('{"tampered": true}')
        with self.assertRaisesRegex(ValueError, "fixture digest mismatch"):
            coverage.independent_python_history(self.root, self.manifest)

    def test_resurrected_files_directories_and_dangling_aliases_fail(self):
        path = self.root / self.path
        for kind in ("file", "directory", "dangling alias", "live alias"):
            if kind == "file":
                path.write_text("raise AssertionError('never execute')\n")
            elif kind == "directory":
                path.mkdir()
            else:
                path.symlink_to("absent.py" if kind == "dangling alias" else "active.py")
            try:
                with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, "exists or is aliased"):
                    coverage.independent_python_history(self.root, self.manifest)
            finally:
                if kind == "directory":
                    path.rmdir()
                else:
                    path.unlink()

    def test_aliased_or_non_directory_ancestors_are_rejected_before_traversal(self):
        for target in (self.root / "absent", self.root.parent):
            alias = self.root / "alias"
            alias.symlink_to(target, target_is_directory=True)
            manifest = copy.deepcopy(self.manifest)
            manifest["retired_independent_python"][0]["path"] = "alias/retired.py"
            manifest["independent_python_baseline"][1] = "alias/retired.py"
            try:
                with self.subTest(target=target), self.assertRaisesRegex(ValueError, "is aliased"):
                    coverage.independent_python_history(self.root, manifest)
            finally:
                alias.unlink()
        (self.root / "not_directory").write_text("not a directory")
        manifest["retired_independent_python"][0]["path"] = "not_directory/retired.py"
        manifest["independent_python_baseline"][1] = "not_directory/retired.py"
        with self.assertRaisesRegex(ValueError, "unsafe.*ancestor"):
            coverage.independent_python_history(self.root, manifest)

    def test_active_files_still_require_presence_and_independent_ast_closure(self):
        active = self.root / "src/active.py"
        active.unlink()
        with self.assertRaisesRegex(ValueError, "missing or escaping"):
            coverage.audit(self.root, self.manifest)
        (self.root / "runner").mkdir()
        (self.root / "runner/coven.py").write_text("raise RuntimeError('never execute')\n")
        active.write_text("from runner import coven\n")
        self.manifest["independent_python_baseline"].append("src/omitted.py")
        errors = coverage.audit(self.root, self.manifest)["errors"]
        self.assertIn("independent Python baseline registry drift", errors)
        self.assertIn("C file is not independently stdlib-only: src/active.py", errors)


if __name__ == "__main__":
    unittest.main()
