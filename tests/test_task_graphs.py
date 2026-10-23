"""Offline, in-memory reviewed DAG contracts. No execution or native calls."""

from copy import deepcopy
import hashlib
import subprocess
import sys
import unittest
from unittest.mock import patch

from runner import storage, task_attempts as attempts, task_graphs as graphs


class TaskGraphTests(unittest.TestCase):
    def setUp(self):
        self.options = dict(authority_id="a" * 32, snapshot_source_id="b" * 32,
                            graph_id="c" * 32, revision_id="d" * 32,
                            predecessor=None, proposed_at="2026-10-22T12:34:56+00:00")
        self.document = {"schema_version": 1, "kind": "task-graph", "nodes": [
            self.node("B", ["A"]), self.node("A"), self.node("C")]}
        self.source = {"identity": {"kind": "snapshot-v1", "source_id": "b" * 32,
                                    "path": "state/tasks.json"},
                       "revision": "tasks-sha256:" + "e" * 64,
                       "tasks": [{"id": key, "title": "Title", "status": "operator-state",
                                  "owner": "Worker", "unknown": {"values": [True, None, 1.0],
                                  "text": "  \u2003caf\u00e9 $(not-executable)  "}}
                                 for key in ("A", "B", "C", "unrelated")],
                       "roster": ["Worker", "other"]}
        self.enrollments = self.enroll(self.source)

    def node(self, key, dependencies=None):
        return {"task_id": key, "owner": "Worker",
                "acceptance_criteria": [{"id": "check", "text": "  \u2003caf\u00e9 $(never-run)  "}],
                "depends_on": dependencies or []}

    def enroll(self, source):
        result = {}
        for index, task in enumerate(source["tasks"]):
            record = {"enrollment_id": f"{index + 1:032x}", "task_id": task["id"],
                      "binding_number": 1, "task_authority_identity": deepcopy(source["identity"]),
                      "task_source_revision": source["revision"],
                      "complete_task_snapshot": deepcopy(task),
                      "task_snapshot_digest": attempts.task_snapshot_digest(task),
                      "enrolled_at": self.options["proposed_at"]}
            result[task["id"]] = {"enrollment_id": record["enrollment_id"],
                                  "task_id": task["id"], "bindings": [record]}
        return result

    def freeze(self, document=None, source=None, enrollments=None, **options):
        return graphs.freeze(self.document if document is None else document,
                             self.source if source is None else source,
                             self.enrollments if enrollments is None else enrollments,
                             **(self.options | options))

    def ledger(self):
        source = deepcopy(self.source)
        source["identity"] = {"kind": "task-ledger-v1", "workspace_id": "1" * 32,
                              "epoch_id": "2" * 32, "path": "state/task-ledger"}
        source["revision"] = ("tasks-ledger:v1:" + "1" * 32 + ":" + "2" * 32 + ":7:"
                              + hashlib.sha256(graphs.canonical(source["tasks"])).hexdigest())
        return source

    def test_topology_lexical_kahn_preserves_arrays(self):
        document = deepcopy(self.document)
        document["nodes"] = [self.node("z"), self.node("b", ["a"]), self.node("a")]
        self.assertEqual(graphs.topology(document), ["a", "b", "z"])
        self.assertEqual(graphs.validate_document(document), document)
        self.assertEqual(graphs.topology(self.freeze()), ["A", "B", "C"])
        self.assertEqual([node["task_id"] for node in self.freeze()["nodes"]], ["B", "A", "C"])

    def test_digest_domains_key_order_and_array_order(self):
        manifest = self.freeze()
        self.assertEqual(graphs.digest(manifest), "task-graph-manifest-v1:" + hashlib.sha256(
            b"coven-task-graph-manifest-v1\0" + attempts.canonical(manifest)).hexdigest())
        self.assertEqual(graphs.digest(manifest), graphs.digest(dict(reversed(list(manifest.items())))))
        node = manifest["nodes"][0]
        payload = {key: value for key, value in node.items() if key != "node_digest"}
        self.assertEqual(node["node_digest"], "task-graph-node-v1:" + hashlib.sha256(
            b"coven-task-graph-node-v1\0" + graphs.canonical(payload)).hexdigest())
        self.assertEqual(graphs.node_digest(node | {"node_digest": "ignored"}), node["node_digest"])
        self.assertNotEqual(graphs.digest(manifest), graphs.digest(manifest | {"nodes": manifest["nodes"][::-1]}))
        self.assertEqual(graphs.digest(self.freeze()), graphs.digest(manifest))

    def test_deep_copies_and_unknown_fields_unicode_operator_values(self):
        original = deepcopy((self.document, self.source, self.enrollments))
        manifest = self.freeze()
        node = manifest["nodes"][0]
        self.assertEqual(node["complete_task_snapshot"], self.source["tasks"][1])
        self.assertEqual(node["acceptance_criteria"], self.document["nodes"][0]["acceptance_criteria"])
        checked = graphs.validate_manifest(manifest)
        checked["nodes"][0]["complete_task_snapshot"]["unknown"]["values"].append("new")
        self.assertNotEqual(checked, manifest)
        checked_document = graphs.validate_document(self.document)
        checked_document["nodes"][0]["depends_on"].clear()
        node["complete_task_snapshot"]["unknown"]["values"].append("new")
        self.assertEqual((self.document, self.source, self.enrollments), original)
        self.assertIn("\u2003caf\u00e9".encode(), graphs.canonical(self.document))

    def test_document_validation_matrix(self):
        invalid = [None, [], {}, self.document | {"extra": 1}]
        for key, value in (("schema_version", True), ("schema_version", 2), ("schema_version", 1.0),
                           ("kind", "execute"), ("nodes", []), ("nodes", {})):
            invalid.append(self.document | {key: value})
        for key, value in (("task_id", "../task"), ("task_id", True), ("owner", "\u00e9"),
                           ("acceptance_criteria", []), ("acceptance_criteria", [{"id": "x", "text": "\u2003"}]),
                           ("acceptance_criteria", [{"id": "x", "text": "x", "extra": 1}]),
                           ("acceptance_criteria", [{"id": "x", "text": "x"}] * 2),
                           ("depends_on", "A"), ("depends_on", [True]), ("depends_on", ["A", "A"]),
                           ("depends_on", ["B"]), ("depends_on", ["missing"]), ("argv", [])):
            invalid.append(self.document | {"nodes": [self.node("B") | {key: value}, self.node("A")]})
        invalid.extend([self.document | {"nodes": [self.node("A"), self.node("A")]},
                        self.document | {"nodes": [self.node("A", ["B"]), self.node("B", ["A"])]}])
        for value in invalid:
            with self.subTest(value=repr(value)[:120]), self.assertRaises(graphs.GraphError):
                graphs.validate_document(value)

    def test_node_edge_and_document_size_bounds(self):
        document = self.document | {"nodes": [self.node(str(i)) for i in range(64)]}
        graphs.validate_document(document)
        with self.assertRaises(graphs.GraphError):
            graphs.validate_document(document | {"nodes": document["nodes"] + [self.node("65")]})
        # 16 roots and 16 dependent nodes provide exactly 256 acyclic edges.
        nodes = [self.node(str(i)) for i in range(16)]
        nodes += [self.node(str(i), [str(j) for j in range(16)]) for i in range(16, 32)]
        graphs.validate_document(document | {"nodes": nodes})
        nodes.append(self.node("32", ["0"]))
        with self.assertRaises(graphs.GraphError):
            graphs.validate_document(document | {"nodes": nodes})
        node = self.node("A")
        node["acceptance_criteria"] = [{"id": str(i), "text": "x" * 16384} for i in range(64)]
        with self.assertRaises(graphs.GraphError):
            graphs.validate_document(document | {"nodes": [node]})

    def test_manifest_shape_types_digests_and_snapshot_validation(self):
        manifest = self.freeze()
        changes = [("schema_version", True), ("schema_version", 2), ("kind", "task-graph"),
                   ("execution_authority_id", "a"), ("graph_id", True), ("graph_revision", "D" * 32),
                   ("predecessor", False), ("proposed_at", "2026-10-22T12:34:56"), ("extra", 1)]
        for key, value in changes:
            with self.subTest(key=key), self.assertRaises(graphs.GraphError):
                graphs.validate_manifest(manifest | {key: value})
        for key, value in (("enrollment_id", "bad"), ("binding_number", True), ("binding_number", 0),
                           ("binding_number", 1.0), ("task_snapshot_digest", "0" * 64),
                           ("node_digest", "bad"), ("complete_task_snapshot", {}),
                           ("complete_task_snapshot", self.source["tasks"][0]), ("extra", None)):
            changed = deepcopy(manifest)
            changed["nodes"][0][key] = value
            with self.subTest(key=key), self.assertRaises(graphs.GraphError):
                graphs.validate_manifest(changed)
        changed = deepcopy(manifest)
        changed["nodes"][1]["enrollment_id"] = changed["nodes"][0]["enrollment_id"]
        changed["nodes"][1]["node_digest"] = graphs.node_digest(changed["nodes"][1])
        with self.assertRaises(graphs.GraphError):
            graphs.validate_manifest(changed)
        self.assertEqual(self.freeze(predecessor="f" * 32)["predecessor"], "f" * 32)

    def test_manifest_bound_includes_unknown_source_records(self):
        self.source["tasks"][0]["unknown"] = "x" * (1024 * 1024)
        with self.assertRaises(graphs.GraphError):
            self.freeze()

    def test_exact_document_and_manifest_byte_bounds(self):
        node = self.node("A")
        node["acceptance_criteria"] = [{"id": str(i), "text": "x" * 16384} for i in range(64)]
        document = self.document | {"nodes": [node]}
        excess = len(graphs.canonical(document)) - graphs.MAX_BYTES
        node["acceptance_criteria"][0]["text"] = "x" * (16384 - excess)
        self.assertEqual(len(graphs.canonical(document)), graphs.MAX_BYTES)
        graphs.validate_document(document)
        node["acceptance_criteria"][0]["text"] += "x"
        with self.assertRaises(graphs.GraphError):
            graphs.validate_document(document)
        self.source["tasks"][0]["padding"] = ""
        base = self.freeze()
        self.source["tasks"][0]["padding"] = "x" * (graphs.MAX_BYTES - len(graphs.canonical(base)))
        manifest = self.freeze()
        self.assertEqual(len(graphs.canonical(manifest)), graphs.MAX_BYTES)
        graphs.validate_manifest(manifest)
        self.source["tasks"][0]["padding"] += "x"
        with self.assertRaises(graphs.GraphError):
            self.freeze()

    def test_rehashed_nodes_still_require_valid_snapshots_and_dag(self):
        baseline = self.freeze()
        changes = [lambda node: node["complete_task_snapshot"].update(title="tampered"),
                   lambda node: node.update(owner="other"),
                   lambda node: node.update(depends_on=["missing"]),
                   lambda node: node.update(depends_on=["B"]),
                   lambda node: node.update(depends_on=["A", "A"])]
        for change in changes:
            manifest = deepcopy(baseline)
            node = manifest["nodes"][0]
            change(node)
            node["node_digest"] = graphs.node_digest(node)
            with self.subTest(change=change), self.assertRaises(graphs.GraphError):
                graphs.validate_manifest(manifest)
        manifest = deepcopy(baseline)
        manifest["nodes"][1]["depends_on"] = ["B"]
        manifest["nodes"][1]["node_digest"] = graphs.node_digest(manifest["nodes"][1])
        with self.assertRaises(graphs.GraphError):
            graphs.validate_manifest(manifest)

    def test_exact_owner_no_aliases_and_unselected_owner_irrelevant(self):
        self.source["tasks"][-1]["owner"] = "outside-roster"
        self.freeze()
        for owner in (None, "worker", "-Worker-"):
            source = deepcopy(self.source)
            source["tasks"][0]["owner"] = owner
            with self.subTest(owner=owner), self.assertRaises(graphs.GraphError):
                self.freeze(source=source)
        for roster in (["worker"], ["-Worker-"], ["Worker", "worker"]):
            with self.assertRaises(graphs.GraphError):
                self.freeze(source=self.source | {"roster": roster})

    def test_current_binding_not_enrollment_snapshot_is_frozen(self):
        self.source["tasks"][0]["title"] = "edited since enrollment"
        self.source["revision"] = "tasks-sha256:" + "f" * 64
        enrollment = self.enrollments["A"]
        binding = deepcopy(enrollment["bindings"][-1])
        binding["binding_number"] = 2
        enrollment["bindings"].append(binding)
        manifest = self.freeze()
        node = next(node for node in manifest["nodes"] if node["task_id"] == "A")
        self.assertEqual(node["binding_number"], 2)
        self.assertEqual(node["complete_task_snapshot"]["title"], "edited since enrollment")
        self.assertEqual(manifest["task_source_revision"], self.source["revision"])
        with self.assertRaises(graphs.GraphError):
            self.freeze(enrollments={})
        binding["task_authority_identity"]["source_id"] = "f" * 32
        with self.assertRaises(graphs.GraphError):
            self.freeze()

    def test_malformed_current_enrollments_fail_closed(self):
        manifest = self.freeze()
        changes = [lambda entry: entry.update(bindings=[]),
                   lambda entry: entry.update(task_id="wrong"),
                   lambda entry: entry["bindings"][-1].update(binding_number=True),
                   lambda entry: entry["bindings"][-1].update(enrollment_id="f" * 32),
                   lambda entry: entry["bindings"][-1].update(task_snapshot_digest="0" * 64)]
        for change in changes:
            enrollments = deepcopy(self.enrollments)
            change(enrollments["A"])
            with self.subTest(change=change), self.assertRaises(graphs.GraphError):
                self.freeze(enrollments=enrollments)
            with self.assertRaises(graphs.GraphError):
                graphs.match_source(manifest, self.source, enrollments)

    def test_selected_drift_and_unrelated_whole_revision_drift(self):
        manifest = self.freeze()
        self.assertEqual(graphs.match_source(manifest, self.source, self.enrollments),
                         {"status": "current", "whole_revision_changed": False, "reasons": []})
        self.source["tasks"][-1]["title"] = "unrelated edit"
        self.source["revision"] = "tasks-sha256:" + "f" * 64
        self.assertEqual(graphs.match_source(manifest, self.source, self.enrollments),
                         {"status": "current", "whole_revision_changed": True, "reasons": []})
        self.source["tasks"][0]["unknown"]["values"][2] = 1
        match = graphs.match_source(manifest, self.source, self.enrollments)
        self.assertEqual(match["status"], "selected_inputs_changed")
        self.assertEqual(match["reasons"], [{"task_id": "A", "reason": "selected_inputs_changed"}])
        self.source["tasks"] = self.source["tasks"][1:]
        self.assertEqual(graphs.match_source(manifest, self.source, self.enrollments)["status"], "selected_inputs_changed")

    def test_match_owner_enrollment_and_authority_changes(self):
        manifest = self.freeze()
        self.assertEqual(graphs.match_source(manifest, self.source | {"roster": ["other"]},
                                            self.enrollments)["status"], "owner_missing")
        self.assertEqual(graphs.match_source(manifest, self.source, {})["status"], "enrollment_changed")
        for field, value in (("binding_number", 2), ("enrollment_id", "f" * 32)):
            enrollments = deepcopy(self.enrollments)
            enrollments["A"]["bindings"][-1][field] = value
            if field == "enrollment_id":
                enrollments["A"][field] = value
            self.assertEqual(graphs.match_source(manifest, self.source, enrollments)["status"], "enrollment_changed")
        source = self.ledger()
        self.assertEqual(graphs.match_source(manifest, source, self.enroll(source))["status"], "source_authority_changed")
        source = deepcopy(self.source)
        source["identity"]["source_id"] = "f" * 32
        self.assertEqual(graphs.match_source(manifest, source, self.enroll(source))["status"], "source_authority_changed")

    def test_ledger_identity_and_revision_namespace_not_selected_hash(self):
        source = self.ledger()
        manifest = self.freeze(source=source, enrollments=self.enroll(source))
        self.assertEqual(graphs.validate_manifest(manifest), manifest)
        self.assertEqual(graphs.match_source(manifest, source, self.enroll(source))["status"], "current")
        # A manifest can validate namespaces, not the hash of unselected ledger records.
        changed = manifest | {"task_source_revision": source["revision"].rsplit(":", 1)[0] + ":" + "0" * 64}
        graphs.validate_manifest(changed)
        for identity in (source["identity"] | {"epoch_id": "3" * 32},
                         source["identity"] | {"path": "/actual/path"},
                         source["identity"] | {"workspace_id": True}):
            with self.assertRaises(graphs.GraphError):
                graphs.validate_manifest(manifest | {"task_authority_identity": identity})
        with self.assertRaises(graphs.GraphError):
            graphs.validate_manifest(self.freeze() | {"task_source_revision": source["revision"]})
        with self.assertRaises(graphs.GraphError):
            graphs.validate_manifest(manifest | {"task_source_revision": self.source["revision"]})
        with self.assertRaises(graphs.GraphError):
            self.freeze(source=source | {"tasks": source["tasks"][:-1]}, enrollments=self.enroll(source))

    def test_invalid_sources_fail_closed_and_strict_json(self):
        manifest = self.freeze()
        for source in (self.source | {"extra": True}, self.source | {"roster": []},
                       self.source | {"tasks": self.source["tasks"] * 2},
                       self.source | {"revision": "tasks-missing:v1"},
                       self.source | {"identity": self.source["identity"] | {"source_id": "bad"}},
                       self.source | {"identity": self.source["identity"] | {"path": "../tasks"}},
                       self.source | {"tasks": [self.source["tasks"][0] | {"x": float("nan")}]}):
            with self.subTest(source=repr(source)[:100]), self.assertRaises(graphs.GraphError):
                graphs.match_source(manifest, source, self.enrollments)
        for value in ({1: "bad key"}, {"x": float("inf")}, {"x": "\ud800"}, (1, 2)):
            with self.assertRaises(graphs.GraphError):
                graphs.canonical(value)
        self.assertTrue(issubclass(graphs.GraphError, storage.StorageError))

    def test_direct_script_import_compatible(self):
        result = subprocess.run([sys.executable, "-B", "-c",
                                 "import task_graphs; assert task_graphs.canonical({'x': 1}) == b'{\"x\":1}'"],
                                cwd="runner", capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pure_operations_do_not_open_files_or_dispatch(self):
        with patch("builtins.open", side_effect=AssertionError("no file I/O")), \
                patch.object(subprocess, "run", side_effect=AssertionError("no dispatch")), \
                patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("no writes")):
            manifest = self.freeze()
            graphs.validate_document(self.document)
            graphs.validate_manifest(manifest)
            graphs.topology(manifest)
            graphs.digest(manifest)
            self.assertEqual(graphs.match_source(manifest, self.source, self.enrollments)["status"], "current")


if __name__ == "__main__":
    unittest.main()
