"""Offline, in-memory selection contracts; no API or native runtime required."""

from copy import deepcopy
import json
import os
import socket
import subprocess
import time
import unittest
from unittest import mock

from runner import session_views as views


def association(group, member, check="matching_process_evidence"):
    return {
        "group_id": "group-" + group, "group_key": group, "group_name": group.title(),
        "participant_id": group + "-" + member, "member_key": member, "role": "Reviewer",
        "association_basis": "explicit_declaration",
        "binding_provenance": ("kitsune.session.snapshot" if check == "legacy_evidence_not_revalidated"
                               else "kitsune.pane.process_info"),
        "declared_pane_id": "old-pane", "declared_terminal_id": "terminal-1",
        "declared_native_reference": {"source": "declared", "agent": "djinn",
                                      "kind": "id", "value": "historical-native-id"},
        "native_identity": "unestablished", "live_attachment_declared": True,
        "binding_check": check,
    }


def target(index, recognized, associations):
    return {
        "pane_id": "w1:p" + str(index), "terminal_id": "terminal-" + str(index),
        "workspace_id": "w1", "tab_id": "tab-1", "provenance": "kitsune.pane.process_info",
        "interpreter": "coven.kitsune.djinn-process.v1",
        "sample_received_at": "2026-10-26T00:00:00+00:00",
        "mapping_evidence": "matching_inventory_brackets",
        "recognition": "djinn_executable_family" if recognized else "unknown",
        "matches": ([{"pid": index, "executable_family": "djinn", "rule": "exact_posix_argv0_basename"}]
                    if recognized else []),
        "activity": "unknown", "attention": "unknown", "native_identity": "unestablished",
        "native_lifecycle": "unestablished", "job_completion": "unestablished",
        "associations": associations,
    }


def report():
    return {
        "schema_version": 1, "contract": "coven.session-inspection.v1",
        "scope": "existing_panes_on_selected_kitsune_endpoint", "runtime": "djinn",
        "workspace": "/fixture/workspace",
        "request": {"mode": "list", "pane_id": None, "expected_terminal_id": None},
        "endpoint": {"selected": "/fixture/api.sock", "resolved": "/fixture/api.sock",
                     "device": 1, "inode": 2},
        "started_at": "2026-10-26T00:00:00+00:00",
        "observed_at": "2026-10-26T00:00:00.010000+00:00", "duration_ms": 10,
        "assessment": "observed", "diagnostics": [],
        "targets": [
            target(1, True, [association("alpha", "one"), association("beta", "one"),
                             association("alpha", "legacy", "legacy_evidence_not_revalidated")]),
            target(2, False, [association("alpha", "changed", "process_evidence_changed")]),
            target(3, True, [association("beta", "two")]),
            target(4, True, []),
            target(5, False, []),
        ],
        "target_count": 5, "recognized_target_count": 3,
        "declarations": {
            "assessment": "observed", "revision": "groups-v1:fixture", "authority_schema_version": 3,
            "unmatched": [association("alpha", "lost", "terminal_not_observed"),
                          association("alpha", "replaced", "endpoint_identity_changed"),
                          association("beta", "lost", "terminal_not_observed")],
            "other_endpoint_count": 7, "outside_inspection_count": 4,
        },
        "authorization": "none", "dispatchable": False, "native_catalog_complete": False,
        "native_identity": "unestablished", "native_lifecycle": "unestablished",
        "native_worker_acceptance": "unestablished", "job_completion": "unestablished",
        "runtime_fact_freshness": "unknown", "server_snapshot_timestamp": None,
        "limitations": ["Process hints are not native identity.", "Unknown is not idle or absent."],
        "server_version": "fixture", "protocol": 18,
    }


def failure(assessment):
    value = report()
    value.update(assessment=assessment, diagnostics=["timeout" if assessment == "unavailable"
                                                   else "unsupported_protocol"],
                 targets=[], target_count=None, recognized_target_count=None)
    value["declarations"] = {
        "assessment": "unavailable", "revision": None, "authority_schema_version": None,
        "unmatched": [], "other_endpoint_count": None, "outside_inspection_count": None,
    }
    value.pop("server_version")
    value.pop("protocol")
    return value


class SelectionValidationTests(unittest.TestCase):
    def test_valid_selector_boundaries(self):
        for key in ("a", "0", "auth-review", "member_one", "0_-", "a" * 64):
            with self.subTest(key=key):
                self.assertIsNone(views.validate_selection(group_key=key))
                self.assertIsNone(views.validate_selection(group_key=key, member_key=key,
                                                           recognized_only=True))
        self.assertIsNone(views.validate_selection())
        self.assertIsNone(views.validate_selection(recognized_only=True))

    def test_invalid_selectors_raise_value_error(self):
        invalid = ("", "a" * 65, "Upper", "a.b", "-a", "_a", "a/b", "a b", "a\n",
                   " a", "a\0", "é", "ａ", "a💡", 1, False, [], {}, b"alpha")
        for key in invalid:
            for field in ("group_key", "member_key"):
                options = {"group_key": "alpha", field: key}
                with self.subTest(field=field, key=key), self.assertRaises(ValueError):
                    views.validate_selection(**options)

    def test_member_requires_group(self):
        for recognized in (False, True):
            with self.subTest(recognized=recognized), self.assertRaises(ValueError):
                views.validate_selection(member_key="one", recognized_only=recognized)

    def test_recognized_only_requires_literal_bool(self):
        for value in (None, 0, 1, 0.0, 1.0, "true", [], {}, object()):
            with self.subTest(value=value), self.assertRaises(ValueError):
                views.validate_selection(recognized_only=value)

    def test_select_report_validates_before_reading_report(self):
        for options in ({"group_key": "BAD"}, {"member_key": "one"}, {"recognized_only": 1}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                views.select_report(None, **options)


class SessionViewTests(unittest.TestCase):
    def assert_independent(self, original, result):
        """Every mutable descendant must be independent, not only the outer dict."""
        def mutable_ids(value):
            if isinstance(value, dict):
                return {id(value)}.union(*(mutable_ids(v) for v in value.values()))
            if isinstance(value, list):
                return {id(value)}.union(*(mutable_ids(v) for v in value))
            return set()
        self.assertFalse(mutable_ids(original) & mutable_ids(result))

    def assert_preserved_context(self, original, result):
        for key in original.keys() - {"targets", "target_count", "recognized_target_count", "declarations"}:
            self.assertEqual(result[key], original[key], key)
        for key in original["declarations"].keys() - {"unmatched"}:
            self.assertEqual(result["declarations"][key], original["declarations"][key], key)

    def test_no_selectors_is_exact_independent_copy(self):
        for value in (report(), failure("unavailable"), failure("incompatible")):
            with self.subTest(assessment=value["assessment"]):
                selected = views.select_report(value)
                self.assertEqual(selected, value)
                self.assertNotIn("view", selected)
                self.assert_independent(value, selected)

    def test_group_selects_exact_associations_without_fabricating_roles(self):
        value = report()
        selected = views.select_report(value, group_key="alpha")
        self.assertEqual([t["pane_id"] for t in selected["targets"]], ["w1:p1", "w1:p2"])
        self.assertEqual([a["member_key"] for a in selected["targets"][0]["associations"]],
                         ["one", "legacy"])
        self.assertEqual(selected["target_count"], 2)
        self.assertEqual(selected["recognized_target_count"], 1)
        self.assertEqual(selected["view"], {
            "group_key": "alpha", "member_key": None, "recognized_only": False,
            "observed_target_count": 5, "observed_recognized_target_count": 3,
            "declaration_count_scope": "full_observation",
        })
        self.assert_preserved_context(value, selected)

    def test_member_is_scoped_to_group_even_with_shared_member_key(self):
        selected = views.select_report(report(), group_key="alpha", member_key="one")
        self.assertEqual(selected["target_count"], 1)
        self.assertEqual(selected["targets"][0]["associations"], [association("alpha", "one")])
        self.assertEqual(selected["declarations"]["unmatched"], [])

    def test_changed_and_legacy_bindings_remain_visible_and_qualified(self):
        for member, index, check in (("changed", 1, "process_evidence_changed"),
                                      ("legacy", 0, "legacy_evidence_not_revalidated")):
            with self.subTest(member=member):
                value = report()
                selected = views.select_report(value, group_key="alpha", member_key=member)
                expected = deepcopy(value["targets"][index])
                expected["associations"] = [a for a in expected["associations"] if a["member_key"] == member]
                self.assertEqual(selected["targets"], [expected])
                self.assertEqual(selected["targets"][0]["associations"][0]["binding_check"], check)
                self.assert_preserved_context(value, selected)

    def test_recognized_only_preserves_unassociated_targets_and_all_their_hints(self):
        value = report()
        selected = views.select_report(value, recognized_only=True)
        self.assertEqual(selected["targets"], [value["targets"][i] for i in (0, 2, 3)])
        self.assertEqual(selected["target_count"], 3)
        self.assertEqual(selected["recognized_target_count"], 3)
        self.assertEqual(selected["declarations"], value["declarations"])
        self.assert_preserved_context(value, selected)

    def test_recognition_and_declaration_filters_intersect(self):
        selected = views.select_report(report(), group_key="alpha", recognized_only=True)
        self.assertEqual(selected["target_count"], 1)
        self.assertEqual(selected["recognized_target_count"], 1)
        self.assertEqual([a["group_key"] for a in selected["targets"][0]["associations"]],
                         ["alpha", "alpha"])
        selected = views.select_report(report(), group_key="alpha", member_key="changed",
                                       recognized_only=True)
        self.assertEqual(selected["targets"], [])
        self.assertEqual(selected["target_count"], 0)
        self.assertEqual(selected["recognized_target_count"], 0)

    def test_unmatched_declarations_filter_by_group_and_member_not_recognition(self):
        for recognized in (False, True):
            for member in (None, "lost", "replaced", "missing"):
                with self.subTest(recognized=recognized, member=member):
                    value = report()
                    selected = views.select_report(value, group_key="alpha", member_key=member,
                                                   recognized_only=recognized)
                    expected = [a for a in value["declarations"]["unmatched"]
                                if a["group_key"] == "alpha"
                                and (member is None or a["member_key"] == member)]
                    self.assertEqual(selected["declarations"]["unmatched"], expected)
                    self.assert_preserved_context(value, selected)

    def test_zero_selected_rows_remains_observed_not_native_absence(self):
        for options in ({"group_key": "alph"}, {"group_key": "missing"},
                        {"group_key": "alpha", "member_key": "on"}):
            with self.subTest(options=options):
                value = report()
                selected = views.select_report(value, **options)
                self.assertEqual(selected["targets"], [])
                self.assertEqual(selected["target_count"], 0)
                self.assertEqual(selected["recognized_target_count"], 0)
                self.assertEqual(selected["assessment"], "observed")
                self.assert_preserved_context(value, selected)

    def test_empty_observation_and_uninitialized_declarations(self):
        value = report()
        value.update(targets=[], target_count=0, recognized_target_count=0)
        value["declarations"].update(assessment="uninitialized", revision="groups-uninitialized:v1",
                                     authority_schema_version=None, unmatched=[],
                                     other_endpoint_count=0, outside_inspection_count=0)
        selected = views.select_report(value, group_key="alpha", recognized_only=True)
        self.assertEqual(selected["view"]["observed_target_count"], 0)
        self.assertEqual(selected["view"]["observed_recognized_target_count"], 0)
        self.assertEqual(selected["target_count"], 0)
        self.assert_preserved_context(value, selected)

    def test_failures_keep_null_counts_and_declaration_failure_semantics(self):
        for assessment in ("unavailable", "incompatible"):
            for options in ({"group_key": "alpha"}, {"recognized_only": True},
                            {"group_key": "alpha", "member_key": "one", "recognized_only": True}):
                with self.subTest(assessment=assessment, options=options):
                    value = failure(assessment)
                    selected = views.select_report(value, **options)
                    metadata = selected.pop("view")
                    self.assertIsNone(metadata["observed_target_count"])
                    self.assertIsNone(metadata["observed_recognized_target_count"])
                    self.assertEqual(metadata["declaration_count_scope"], "full_observation")
                    self.assertEqual(selected, value)
                    self.assert_independent(value, selected)

    def test_each_call_is_independent_and_never_retains_prior_success(self):
        value = report()
        before = deepcopy(value)
        first = views.select_report(value, group_key="alpha")
        second = views.select_report(value, group_key="beta")
        self.assert_independent(value, first)
        self.assert_independent(value, second)
        self.assert_independent(first, second)
        first["targets"][0]["associations"][0]["declared_native_reference"]["value"] = "changed"
        first["limitations"].append("changed")
        first["declarations"]["unmatched"].clear()
        self.assertEqual(value, before)
        self.assertEqual(second, views.select_report(value, group_key="beta"))
        failed = views.select_report(failure("unavailable"), group_key="alpha")
        self.assertEqual(failed["targets"], [])
        self.assertIsNone(failed["target_count"])

    def test_selection_matrix_preserves_input_context_and_order(self):
        for group in (None, "alpha", "beta", "missing"):
            for member in ((None,) if group is None else (None, "one", "lost", "missing")):
                for recognized in (False, True):
                    with self.subTest(group=group, member=member, recognized=recognized):
                        value = report()
                        before = deepcopy(value)
                        selected = views.select_report(value, group_key=group, member_key=member,
                                                       recognized_only=recognized)
                        self.assertEqual(value, before)
                        self.assert_independent(value, selected)
                        self.assert_preserved_context(value, selected)
                        self.assertEqual(selected["target_count"], len(selected["targets"]))
                        self.assertEqual(selected["recognized_target_count"], sum(
                            t["recognition"] == "djinn_executable_family" for t in selected["targets"]))
                        panes = [t["pane_id"] for t in selected["targets"]]
                        self.assertEqual(panes, [t["pane_id"] for t in value["targets"] if t["pane_id"] in panes])
                        json.dumps(selected, allow_nan=False)

    def test_inspect_request_and_full_observation_counts_are_not_reinterpreted(self):
        value = report()
        value["request"].update(mode="inspect", pane_id="w1:p1", expected_terminal_id="terminal-1")
        value.update(targets=value["targets"][:1], target_count=1, recognized_target_count=1)
        selected = views.select_report(value, group_key="beta", member_key="two")
        self.assertEqual(selected["target_count"], 0)
        self.assertEqual(selected["view"]["observed_target_count"], 1)
        self.assertEqual(selected["declarations"]["other_endpoint_count"], 7)
        self.assertEqual(selected["declarations"]["outside_inspection_count"], 4)
        self.assert_preserved_context(value, selected)

    def test_pure_validation_and_selection_have_no_io_or_clock_reads(self):
        value = report()
        with mock.patch("builtins.open", side_effect=AssertionError("file I/O")), \
             mock.patch.object(os, "open", side_effect=AssertionError("file I/O")), \
             mock.patch.object(socket, "socket", side_effect=AssertionError("network I/O")), \
             mock.patch.object(subprocess, "run", side_effect=AssertionError("subprocess")), \
             mock.patch.object(subprocess, "Popen", side_effect=AssertionError("subprocess")), \
             mock.patch.object(time, "time", side_effect=AssertionError("wall clock")), \
             mock.patch.object(time, "monotonic", side_effect=AssertionError("monotonic clock")):
            views.validate_selection(group_key="alpha", member_key="one", recognized_only=True)
            selected = views.select_report(value, group_key="alpha", member_key="one",
                                           recognized_only=True)
        self.assertEqual(selected["target_count"], 1)


if __name__ == "__main__":
    unittest.main()
