"""Independent audit tests, intentionally outside tests/ discovery."""

import ast
import contextlib
import copy
import functools
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import retirement_coverage as coverage


INDEPENDENT_PYTHON_BASELINE = [
    "tests/_compat_values.py",
    "src/checkpoint_guidance/timestamp/oracle.py",
    "src/process/identity/tests/which_oracle.py",
    "src/workers/fixture.py",
    "src/workspace_commands/python/oracle.py",
]


class ActiveOracleTransitionTests(unittest.TestCase):
    """Approved synthetic records exercise validation without approving the real draft."""

    def setUp(self):
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.root = Path(directory)
        (self.root / "runner").mkdir()
        self.source = self.root / "runner/coven.py"
        self.source.write_text("raise RuntimeError('audit must never execute this source')\n")
        self.path = "tests/retirement/reviews/active_oracle_directory_isolation.json"
        self.record_path = self.root / self.path
        self.record_path.parent.mkdir(parents=True)
        self.active = {"path": "runner/coven.py", "sha256": coverage.digest(self.source),
                       "previous_sha256": "1" * 64, "historical_sha256": "2" * 64,
                       "review": {"path": self.path, "sha256": "0" * 64}}
        self.provenance = {"active_oracle": self.active, "oracle_coven_sha256": "2" * 64,
                           "source_head": "3" * 40}
        self.record = {
            "schema_version": 1, "record_path": self.path, "approval_status": "parent_approved",
            "source": {"path": "runner/coven.py", "old_sha256": "1" * 64,
                       "new_sha256": self.active["sha256"], "historical_sha256": "2" * 64},
            "from": {"commit": "4" * 40, "blob": "5" * 40, "sha256": "1" * 64},
            "to": {"commit": "6" * 40, "blob": "7" * 40, "sha256": self.active["sha256"]},
            "historical_capture": {"commit": "3" * 40, "blob": "8" * 40, "sha256": "2" * 64},
        }
        self.pin_record()

    def pin_record(self):
        self.record_path.write_text(json.dumps(self.record))
        self.active["review"]["sha256"] = coverage.digest(self.record_path)

    def test_approved_link_and_no_active_oracle_are_valid_without_execution(self):
        coverage.validate_active_oracle(self.root, self.provenance)
        coverage.validate_active_oracle(self.root, {})
        manifest = {"schema_version": 1, "provenance": self.provenance, "files": [],
                    "original_method_count": 0, "category_counts": dict.fromkeys(coverage.CATEGORIES, 0),
                    "candidate_mappings": {}, "independent_python": [], "independent_python_baseline": [],
                    "native_integration_inventory": []}
        self.assertEqual(coverage.audit(self.root, manifest)["errors"], [])
        self.record["approval_status"] = "pending_parent_review"
        self.pin_record()
        self.assertEqual(coverage.audit(self.root, manifest)["errors"],
                         ["active oracle transition is not parent approved"])
        manifest = copy.deepcopy(manifest)
        manifest["provenance"].pop("active_oracle")
        self.assertEqual(coverage.audit(self.root, manifest)["errors"],
                         ["missing active oracle transition for historical oracle"])
        manifest["provenance"] = {}
        self.assertEqual(coverage.audit(self.root, manifest)["errors"], [])

    def test_real_manifest_cannot_drop_active_oracle_even_with_changed_parseable_runner(self):
        manifest = json.loads((coverage.ROOT / coverage.MANIFEST).read_text(),
                              object_pairs_hook=coverage.unique_object)
        manifest["provenance"].pop("active_oracle")
        self.source.write_text("def changed_runner():\n    return 'not the reviewed source'\n")
        ast.parse(self.source.read_text())
        for root in (coverage.ROOT, self.root):
            with self.subTest(root=root), self.assertRaisesRegex(ValueError, "missing active oracle transition"):
                coverage.validate_active_oracle(root, manifest["provenance"])

    def test_missing_pending_and_tampered_records_fail_closed(self):
        original = copy.deepcopy(self.record)
        for status in (None, "pending_parent_review", "pending_parent_diff_review"):
            self.record = copy.deepcopy(original)
            if status is None:
                self.record.pop("approval_status")
            else:
                self.record["approval_status"] = status
            self.pin_record()
            with self.subTest(status=status), self.assertRaisesRegex(ValueError, "not parent approved"):
                coverage.validate_active_oracle(self.root, self.provenance)
        self.record_path.write_text(json.dumps(original))
        with self.assertRaisesRegex(ValueError, "review digest mismatch"):
            coverage.validate_active_oracle(self.root, self.provenance)
        self.record_path.unlink()
        with self.assertRaisesRegex(ValueError, "missing or escaping"):
            coverage.validate_active_oracle(self.root, self.provenance)

    def test_record_paths_and_all_hash_links_are_required_even_after_rehash(self):
        original = copy.deepcopy(self.record)
        changes = [("record_path", None), ("source", "path"), ("source", "old_sha256"),
                   ("source", "new_sha256"), ("source", "historical_sha256"),
                   ("from", "sha256"), ("to", "sha256"), ("historical_capture", "sha256"),
                   ("historical_capture", "commit")]
        for key, field in changes:
            self.record = copy.deepcopy(original)
            if field is None:
                self.record[key] = "tests/retirement/reviews/wrong.json"
            else:
                self.record[key][field] = "9" * (40 if field == "commit" else 64)
            self.pin_record()
            with self.subTest(key=key, field=field), self.assertRaisesRegex(ValueError, "mismatch"):
                coverage.validate_active_oracle(self.root, self.provenance)

    def test_source_change_cannot_be_blessed_by_rewriting_only_manifest_pin(self):
        self.source.write_text("raise RuntimeError('changed source, still never executed')\n")
        with self.assertRaisesRegex(ValueError, "source drift"):
            coverage.validate_active_oracle(self.root, self.provenance)
        self.active["sha256"] = coverage.digest(self.source)
        with self.assertRaisesRegex(ValueError, "transition linkage mismatch"):
            coverage.validate_active_oracle(self.root, self.provenance)

    def test_historical_manifest_pin_and_commit_cannot_be_relinked_alone(self):
        for key, value in (("oracle_coven_sha256", "9" * 64), ("source_head", "9" * 40)):
            provenance = copy.deepcopy(self.provenance)
            provenance[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "historical capture linkage mismatch"):
                coverage.validate_active_oracle(self.root, provenance)

    def test_active_oracle_format_and_review_reference_are_explicit(self):
        for key in self.active:
            provenance = copy.deepcopy(self.provenance)
            provenance["active_oracle"].pop(key)
            with self.subTest(missing=key), self.assertRaisesRegex(ValueError, "invalid active oracle format"):
                coverage.validate_active_oracle(self.root, provenance)
        for key in ("sha256", "previous_sha256", "historical_sha256"):
            for value in (None, "", "Z" * 64):
                provenance = copy.deepcopy(self.provenance)
                provenance["active_oracle"][key] = value
                with self.subTest(key=key, value=value), self.assertRaisesRegex(ValueError, "hash format"):
                    coverage.validate_active_oracle(self.root, provenance)
        for value in (None, {}, {"path": self.path, "sha256": ""},
                      {"path": "../outside", "sha256": "0" * 64}):
            provenance = copy.deepcopy(self.provenance)
            provenance["active_oracle"]["review"] = value
            with self.subTest(review=value), self.assertRaisesRegex(ValueError, "review reference"):
                coverage.validate_active_oracle(self.root, provenance)
        provenance = copy.deepcopy(self.provenance)
        provenance["active_oracle"]["path"] = "runner/other.py"
        with self.assertRaisesRegex(ValueError, "source path mismatch"):
            coverage.validate_active_oracle(self.root, provenance)

    def test_record_endpoint_and_schema_formats_are_explicit(self):
        original = copy.deepcopy(self.record)
        for key in ("from", "to", "historical_capture"):
            for field in ("commit", "blob", "sha256"):
                self.record = copy.deepcopy(original)
                self.record[key][field] = "not-a-hash"
                self.pin_record()
                with self.subTest(key=key, field=field), self.assertRaisesRegex(ValueError, "provenance mismatch"):
                    coverage.validate_active_oracle(self.root, self.provenance)
        self.record = copy.deepcopy(original)
        self.record["schema_version"] = 2
        self.pin_record()
        with self.assertRaisesRegex(ValueError, "transition linkage mismatch"):
            coverage.validate_active_oracle(self.root, self.provenance)


class RetirementCoverageTests(unittest.TestCase):
    new_batches = {
        "launch": "a48f6842247a0f40ed04fa791925e25097e3f6cc00483bb22f5983e2b32ca2be",
        "groups": "f5792f0c3bc098078b5ba69e5d1e391e8aa10d52b72f5dba7fc86e3aae547dd2",
        "handoff_schema": "2dbcfd4e1c7b0c1d278bca7f84f4d90b64a1dc963631b2a8c4cdb8c2cc720a0b",
    }

    @classmethod
    def setUpClass(cls):
        cached = mock.patch.object(coverage, "rust_tests", side_effect=functools.lru_cache()(coverage.rust_tests))
        cached.start()
        cls.addClassCleanup(cached.stop)
        cls.manifest = json.loads((coverage.ROOT / coverage.MANIFEST).read_text(), object_pairs_hook=coverage.unique_object)
        cls.report = coverage.audit(coverage.ROOT, cls.manifest)
        cls.fragments = {
            name: json.loads((coverage.ROOT / f"tests/retirement/reviews/{name}.json").read_text(),
                             object_pairs_hook=coverage.unique_object)
            for name in cls.new_batches
        }

    def test_frozen_original_ledger_content_and_id_fingerprint(self):
        ids = sorted(unit["id"] for unit in self.report["units"])
        self.assertEqual(hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest(),
                         "a48118da016436fea96b135e5e6a799eaab6d249f617a901da1a288b05f10b0a")
        expected = {
            "files": "209e0e91963110eccc11cb4e04538617b2ad8683abbb59ad331cba3584434b65",
            "reviews": "e184ec83f7454cf6f235a98ab214983c570449491a035f24378e538bfc34df8b",
            "candidate_mappings": "0ebe8e27d5254b84c9bd2e0b2d33ac8191627d515b9fcebbd7b1d8d48c35603b",
            "category_counts": "99cb27b97d73523ac4a779566cc20b0a5f39f2ad8b5bb85beac9cbbd1fb92a60",
            "original_method_count": "dae40fcfed8e9ce405c3e0e207ba4781acb0a6a55a527f591b6b2b442f8cbbc5",
            "accepted_batches": "5823b5379842eaea60a4f4dbcf952e2e2c7d4c04ad1cbda11d957fe00660c837",
            "accepted_source_transitions": "d401a700b6785de521414458ccdee78a23d944fc0dce81423d2989bd69f61e96",
        }
        # Project only the three accepted additions away; retain the 24d73fe fingerprints.
        prior = copy.deepcopy(self.manifest)
        new_reviews = {identity: review for fragment in self.fragments.values()
                       for identity, review in fragment["reviews"].items()}
        self.assertEqual(len(new_reviews), 31)
        new_b = {identity for identity, review in new_reviews.items()
                 if review.get("category", review.get("category_suggestion")) == "B"}
        self.assertEqual(len(new_b), 14)
        for identity in new_reviews:
            prior["reviews"].pop(identity)
        for identity in new_b:
            prior["candidate_mappings"].pop(identity)
        originals = {"tests/test_launch.py": 19, "tests/test_groups.py": 35, "tests/test_handoffs.py": 33}
        for entry in prior["files"]:
            if entry["path"] in originals:
                self.assertEqual(sum(len(names) for names in entry["classes"].values()), originals[entry["path"]])
                entry.pop("overrides")
                entry["counts"] = {"A": 0, "B": 0, "C": 0, "D": originals[entry["path"]]}
        prior["category_counts"] = {"A": 53, "B": 15, "C": 0, "D": 873}
        prior["provenance"]["accepted_batches"] = prior["provenance"]["accepted_batches"][:4]
        self.assertEqual(len(prior["reviews"]), 80)
        self.assertEqual(sum(review["status"] == "mapped" for review in prior["reviews"].values()), 67)
        self.assertEqual(len(prior["candidate_mappings"]), 15)
        for key, sha256 in expected.items():
            value = prior.get(key, prior["provenance"].get(key))
            with self.subTest(key=key):
                self.assertEqual(hashlib.sha256(json.dumps(value, sort_keys=True,
                                                         separators=(",", ":")).encode()).hexdigest(), sha256)

    def test_original_ids_categories_and_unresolved_status_are_frozen(self):
        self.assertEqual(self.report["errors"], [])
        self.assertEqual(len(self.report["units"]), 941)
        self.assertEqual(len({unit["id"] for unit in self.report["units"]}), 941)
        self.assertEqual(len(self.report["file_counts"]), 44)
        self.assertEqual(self.report["categories"], {"A": 62, "B": 29, "C": 0, "D": 850})
        self.assertEqual(self.report["statuses"], {"mapped": 90, "unresolved": 851})
        self.assertEqual(len(self.report["independent_python"]), 4)
        views = [unit for unit in self.report["units"] if "::tests/test_session_views.py::" in unit["id"]]
        self.assertEqual(len(views), 19)
        self.assertEqual(sum(unit["status"] == "mapped" for unit in views), 18)
        self.assertEqual([unit["category"] for unit in views if unit["status"] == "unresolved"], ["D"])
        batches = self.manifest["provenance"]["accepted_batches"]
        self.assertEqual([batch["path"] for batch in batches], [
            f"tests/retirement/reviews/{name}.json"
            for name in ["records", "checkpoints", "task_graphs", "session_prompts", *self.new_batches]
        ])
        expected = {
            "tests/test_records.py": (4, 0, {"A": 4, "B": 0, "C": 0, "D": 0}),
            "tests/test_checkpoints.py": (16, 2, {"A": 16, "B": 0, "C": 0, "D": 2}),
            "tests/test_task_graphs.py": (14, 4, {"A": 15, "B": 0, "C": 0, "D": 3}),
            "tests/test_session_prompts.py": (15, 6, {"A": 0, "B": 15, "C": 0, "D": 6}),
            "tests/test_launch.py": (11, 8, {"A": 9, "B": 2, "C": 0, "D": 8}),
            "tests/test_groups.py": (9, 26, {"A": 0, "B": 9, "C": 0, "D": 26}),
            "tests/test_handoffs.py": (3, 30, {"A": 0, "B": 3, "C": 0, "D": 30}),
        }
        for name, (mapped, unresolved, categories) in expected.items():
            units = [unit for unit in self.report["units"] if unit["id"].split("::")[1] == name]
            self.assertEqual(sum(unit["status"] == "mapped" for unit in units), mapped)
            self.assertEqual(sum(unit["status"] == "unresolved" for unit in units), unresolved)
            self.assertEqual(self.report["file_counts"][name], categories)
        reviewed_files = set(expected) | {"tests/test_session_views.py"}
        untouched = [unit for unit in self.report["units"] if unit["id"].split("::")[1] not in reviewed_files]
        self.assertEqual(len(untouched), 941 - sum(sum(counts.values()) for _, _, counts in expected.values()) - len(views))
        self.assertEqual(len(untouched), 774)
        self.assertTrue(all(unit["status"] == "unresolved" and not unit["review"]
                            and not unit["candidate_native_tests"] for unit in untouched))
        self.assertEqual(len(self.manifest["reviews"]), 111)
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

    def test_independent_helper_history_is_separate_from_original_mapping_units(self):
        self.assertEqual(self.manifest["independent_python_baseline"], INDEPENDENT_PYTHON_BASELINE)
        self.assertEqual(self.report["independent_python_baseline"], INDEPENDENT_PYTHON_BASELINE)
        retired_path = "src/process/identity/tests/which_oracle.py"
        self.assertEqual(self.manifest["independent_python"],
                         [path for path in INDEPENDENT_PYTHON_BASELINE if path != retired_path])
        entry = {"path": retired_path,
                 "sha256": "a3920a4df586b1ce97d938c4b48b96d02c0349b982f63008c10825302a5ab6e2",
                 "review": {"path": "tests/retirement/reviews/process_identity_oracles.json",
                            "sha256": "32dd1216b41d67b69502a2e19e60c99abaa480cd3a7b703a82d53c4bcf18bc1f"}}
        self.assertEqual(self.manifest["retired_independent_python"], [entry])
        record = json.loads((coverage.ROOT / entry["review"]["path"]).read_text())
        helper = record["retired_helpers"][retired_path]
        self.assertEqual(self.report["retired_independent_python"], [{
            **entry, "historical_commit": "d6515b0ada93036b28869ed1b9ed60633a2c1bbf",
            "mapping_delta": 0, "kind": "independent_stdlib_helper",
            "replacement_tests": helper["replacement_tests"], "fixtures": helper["fixtures"],
        }])
        self.assertEqual(helper["replacement_tests"], [
            "rust::src/process/identity/tests.rs::relative_path_directories_and_unset_default",
        ])
        self.assertEqual(helper["fixtures"], [
            {"path": "tests/retirement/fixtures/process_identity.json",
             "sha256": "ee3f56e401ae88caddd2ad9165537e4a244a32baa98dfb1fdd7156285951bf37"},
            {"path": "tests/retirement/fixtures/process_identity_provenance.json",
             "sha256": "6fb4e35e30b383305ae2b292a7f694253e7b0c14d424f3d4e09cef1f433cf1ff"},
        ])
        self.assertFalse(any(retired_path in unit["id"] for unit in self.report["units"]))
        for key in ("reviews", "candidate_mappings"):
            self.assertFalse(any(retired_path in identity for identity in self.manifest[key]))
        self.assertNotIn(entry["review"]["path"],
                         [batch["path"] for batch in self.manifest["provenance"]["accepted_batches"]])

    def test_new_batches_are_exact_hash_pinned_fragments_with_scoped_categories(self):
        batches = self.manifest["provenance"]["accepted_batches"][4:]
        self.assertEqual(len(batches), 3)
        units = {unit["id"]: unit for unit in self.report["units"]}
        for batch, (name, sha256) in zip(batches, self.new_batches.items()):
            with self.subTest(batch=name):
                self.assertEqual(batch["path"], f"tests/retirement/reviews/{name}.json")
                self.assertEqual(batch["sha256"], sha256)
                self.assertEqual(coverage.digest(coverage.ROOT / batch["path"]), sha256)
                self.assertEqual(batch["acceptance"],
                                 "Main and independent review accepted; explicitly merged into global reviews. "
                                 "This acceptance supersedes proposal/draft scope wording preserved in historical "
                                 "fragment metadata; it does not authorize global retirement or source Python deletion.")
                fragment = self.fragments[name]
                self.assertEqual(batch["metadata"], {key: value for key, value in fragment.items() if key != "reviews"})
                source = fragment["original_source"]
                entry = next(entry for entry in self.manifest["files"] if entry["path"] == source["path"])
                self.assertEqual(entry["sha256"], source["sha256"])
                self.assertEqual(coverage.digest(coverage.ROOT / source["path"]), source["sha256"])
                selected = {identity: review for identity, review in self.manifest["reviews"].items()
                            if identity.split("::")[1] == source["path"]}
                self.assertEqual(selected, fragment["reviews"])
                source_text = (coverage.ROOT / source["path"]).read_text()
                methods = coverage.methods(coverage.ROOT / source["path"])
                for identity, review in selected.items():
                    unit = units[identity]
                    category = review.get("category", review.get("category_suggestion"))
                    self.assertEqual((unit["category"], unit["status"]), (category, review["status"]))
                    self.assertEqual(unit["review"], review)
                    if category == "B":
                        self.assertEqual(self.manifest["candidate_mappings"][identity], review["native_tests"])
                    else:
                        self.assertNotIn(identity, self.manifest["candidate_mappings"])
                    for reference in review["native_tests"]:
                        kind, path, function = reference.split("::")
                        self.assertEqual(kind, "rust")
                        self.assertEqual(coverage.rust_tests(coverage.ROOT, path).count(function), 1)
                    if "method_sha256" in review:
                        cls, method = identity.split("::")[2:]
                        node = methods[f"{cls}.{method}"]
                        segment = "".join(source_text.splitlines(keepends=True)[node.lineno - 1:node.end_lineno])
                        self.assertEqual(hashlib.sha256(segment.encode()).hexdigest(), review["method_sha256"])
                if "reference_implementation" in fragment:
                    reference = fragment["reference_implementation"]
                    self.assertEqual(coverage.digest(coverage.ROOT / reference["path"]), reference["sha256"])
        handoffs = [unit for unit in units.values() if unit["id"].split("::")[1] == "tests/test_handoffs.py"]
        schema = [unit for unit in handoffs if unit["id"].split("::")[2] == "SchemaTests"]
        self.assertEqual(len(schema), 5)
        self.assertEqual(sum(unit["category"] == "B" for unit in schema), 3)
        self.assertTrue(all(unit["category"] == "D" and unit["status"] == "unresolved"
                            for unit in handoffs if unit not in schema))
        for identity in self.fragments["handoff_schema"]["unresolved"]:
            self.assertEqual((units[identity]["category"], units[identity]["status"]), ("D", "unresolved"))
            self.assertFalse(units[identity]["review"])

    def test_launch_a_fixtures_and_historical_capture_remain_pinned(self):
        fixtures = [
            {"path": "tests/retirement/fixtures/launch.json",
             "sha256": "a737221789c46c060f055b93e407e3313bb6b2ffd80573acda4714b0e6d9e8e6"},
            {"path": "tests/retirement/fixtures/launch_provenance.json",
             "sha256": "17e1d7f5b69eab53c13d000b165750060fd74b2d447fa470dc1a41eff463a021"},
        ]
        for fixture in fixtures:
            self.assertEqual(coverage.digest(coverage.ROOT / fixture["path"]), fixture["sha256"])
        reviews = self.fragments["launch"]["reviews"]
        a_reviews = [review for review in reviews.values() if review["category_suggestion"] == "A"]
        self.assertEqual(len(a_reviews), 9)
        for review in a_reviews:
            self.assertEqual(review["fixtures"], fixtures)
        corpus_path = coverage.ROOT / fixtures[0]["path"]
        corpus = json.loads(corpus_path.read_text())
        provenance = json.loads((coverage.ROOT / fixtures[1]["path"]).read_text())
        self.assertEqual(provenance["corpus_sha256"], fixtures[0]["sha256"])
        self.assertEqual(provenance["corpus_bytes"], corpus_path.stat().st_size)
        self.assertEqual(provenance["cases"], len(corpus))
        self.assertEqual(len(corpus), 298)
        case_ids = {case["id"] for case in corpus}
        self.assertEqual(len(case_ids), 298)
        self.assertEqual(sum(identity.startswith("original/") for identity in case_ids), 66)
        self.assertEqual(provenance["oracle_commit"], "24d73fede7d8e305cc2572c623a0eb93b440e806")
        self.assertEqual(provenance["unchanged_launch_since"], self.manifest["provenance"]["source_head"])
        for name, sha256 in provenance["source_sha256"].items():
            # These two source pins describe historical capture, not the new native test source.
            if name not in {"src/launch/tests.rs", "src/launch/oracle.py"}:
                self.assertEqual(coverage.digest(coverage.ROOT / name), sha256)
        self.assertEqual(coverage.digest(coverage.ROOT / provenance["capture_tool"]), provenance["capture_tool_sha256"])
        self.assertEqual(len(provenance["original_methods"]), 13)
        source = (coverage.ROOT / "tests/test_launch.py").read_text()
        methods = coverage.methods(coverage.ROOT / "tests/test_launch.py")
        for method, evidence in provenance["original_methods"].items():
            node = methods[f"LaunchTests.{method}"]
            self.assertEqual([node.lineno, node.end_lineno], evidence["lines"])
            self.assertEqual(hashlib.sha256(ast.get_source_segment(source, node).encode()).hexdigest(),
                             evidence["sha256"])
            review = reviews[f"python::tests/test_launch.py::LaunchTests::{method}"]
            if method == "test_strict_recomputed_schema":
                self.assertEqual(evidence["captured_cases"], 0)
                self.assertEqual(review["category_suggestion"], "B")
                self.assertEqual(len(review["cases"]), 9)
                self.assertTrue(all(case.startswith("mutation/") for case in review["cases"]))
                self.assertFalse(set(review["cases"]) & case_ids)
            else:
                self.assertEqual(len(review["cases"]), evidence["captured_cases"])
                self.assertTrue(set(review["cases"]) <= case_ids)

    def test_twenty_nine_b_candidates_have_explicit_reviewed_native_test_ids(self):
        units = [unit for unit in self.report["units"] if unit["category"] == "B"]
        self.assertEqual(len(units), 29)
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
        self.assertEqual(sum(len(item["tests"]) for item in native), 55)
        interactive = next(item for item in native if item["path"] == "tests/interactive_sessions_workflow.rs")
        self.assertEqual(interactive["tests"], ["native_interactive_session_workflow",
                                              "unsupported_adapters_never_contact_or_create_workspace"])
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
        self.assertIn("A=62 B=29 C=0 D=850", out.getvalue())
        self.assertIn("mapped=90 unresolved=851", out.getvalue())
        self.assertIn("Original methods: 941", out.getvalue())
        self.assertIn("Supplemental Python methods: 38 in 3 files", out.getvalue())
        self.assertIn("Full Python suite methods: 979", out.getvalue())
        self.assertIn("Independent stdlib Python files (C): 4 active, 1 retired, 5 historical", out.getvalue())
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

    def test_supplemental_cohorts_have_exact_actual_ids_without_original_mappings(self):
        supplemental = self.report["supplemental_python"]
        entries = {entry["path"]: entry for entry in self.manifest["supplemental_python"]}
        self.assertEqual(supplemental["method_count"], 38)
        self.assertEqual(supplemental["file_counts"], {"tests/test_wrapper.py": 5,
                         "tests/test_dev_install.py": 21, "tests/test_install_dev_environment.py": 12})
        actual_ids = []
        for path, entry in entries.items():
            actual = coverage.methods(coverage.ROOT / path)
            self.assertEqual(set(actual), {f"{cls}.{method}" for cls, names in entry["classes"].items()
                                           for method in names})
            actual_ids.extend(f"python::{path}::{key.replace('.', '::', 1)}" for key in actual)
            self.assertEqual(coverage.digest(coverage.ROOT / path), entry["sha256"])
        self.assertEqual(supplemental["ids"], sorted(actual_ids))
        self.assertEqual(self.report["full_python_method_count"], 979)
        entry = entries["tests/test_wrapper.py"]
        self.assertEqual(entry["source_main_commit"], "649324cda16915a4a5cf8e9746f02c32f6b77f59")
        self.assertEqual(entry["original_main_sha256"],
                         "30c204782c1f87be53b35b9fe805f8935724bb6cdef9b5269e32d6fe26c712fc")
        for path, commit, sha256, counts in [
            ("tests/test_dev_install.py", "1a8d392b4c7fb3f944821cf49a1d01418a1979cf",
             "6083b8ddee0e279356edb19b1e14fafd2a41fffd8cec33d914cf541fc068f6c2", {"DevInstallTests": 21}),
            ("tests/test_install_dev_environment.py", "b5c9ece62af77f96568c3eb9e9542894154f2ecd",
             "6d39332e9c41c378cfe2afc060e9dc3239a1953363a8b78e8fb8b5f26dbc894a",
             {"BuildEnvironmentTests": 11, "SystemProbeTests": 1}),
        ]:
            entry = entries[path]
            self.assertEqual(entry["source_main_commit"], commit)
            self.assertEqual(entry["sha256"], sha256)
            self.assertEqual(entry["original_main_sha256"], sha256)
            self.assertEqual({cls: len(names) for cls, names in entry["classes"].items()}, counts)
            self.assertNotIn(path, self.manifest["independent_python"])
            self.assertIn("spec_from_file_location", entry["policy_transition_notes"])
            self.assertIn("outside the audit's current import closure", entry["policy_transition_notes"])
        self.assertFalse(set(supplemental["ids"]) & {unit["id"] for unit in self.report["units"]})
        for name in ("reviews", "candidate_mappings"):
            manifest = copy.deepcopy(self.manifest)
            manifest[name][supplemental["ids"][0]] = {} if name == "reviews" else []
            self.assertIn(f"unknown original IDs in {name}", coverage.audit(coverage.ROOT, manifest)["errors"])

    def test_unapproved_supplemental_file_is_not_silently_dropped(self):
        for entry in self.manifest["supplemental_python"]:
            manifest = copy.deepcopy(self.manifest)
            manifest["supplemental_python"] = [item for item in manifest["supplemental_python"]
                                                if item["path"] != entry["path"]]
            with self.subTest(path=entry["path"]):
                errors = coverage.audit(coverage.ROOT, manifest)["errors"]
                self.assertTrue(any("inventory drift" in error and entry["path"] in error for error in errors))

    def test_supplemental_new_and_renamed_methods_fail_even_with_same_count(self):
        original_methods = coverage.methods
        for entry in self.manifest["supplemental_python"]:
            for change in ("add", "rename", "remove"):
                def changed_methods(path):
                    actual = original_methods(path)
                    if path.name == Path(entry["path"]).name:
                        if change != "add":
                            actual.pop(next(iter(actual)))
                        if change != "remove":
                            actual[f"{next(iter(entry['classes']))}.test_unapproved"] = None
                    return actual

                with self.subTest(path=entry["path"], change=change), mock.patch.object(
                        coverage, "methods", side_effect=changed_methods):
                    errors = coverage.audit(coverage.ROOT, self.manifest)["errors"]
                self.assertIn(f"{entry['path']}: supplemental method drift", errors)
                self.assertTrue(any("inventory drift" in error for error in errors))

    def test_supplemental_source_assertion_drift_fails_without_id_changes(self):
        original_local_file = coverage.local_file
        for entry in self.manifest["supplemental_python"]:
            with self.subTest(path=entry["path"]), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / Path(entry["path"]).name
                tree = ast.parse((coverage.ROOT / entry["path"]).read_text())
                method = next(node for cls in tree.body if isinstance(cls, ast.ClassDef)
                              for node in cls.body if isinstance(node, ast.FunctionDef)
                              and node.name.startswith("test_"))
                method.body.append(ast.Assert(test=ast.Constant(value=False)))
                path.write_text(ast.unparse(ast.fix_missing_locations(tree)))
                with mock.patch.object(coverage, "local_file", side_effect=lambda root, name:
                                       path if name == entry["path"] else original_local_file(root, name)):
                    errors = coverage.audit(coverage.ROOT, self.manifest)["errors"]
                self.assertEqual(errors, [f"{entry['path']}: supplemental assertion source drift"])

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
        self.assertIn("active oracle transition linkage mismatch", coverage.audit(coverage.ROOT, manifest)["errors"])

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
