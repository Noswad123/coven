"""Byte-preserving checkpoint contracts, without filesystem or runtime access."""

import importlib
import json
from pathlib import Path
import subprocess
import sys
import unittest

from runner import checkpoints as cp, storage


TEMPLATE = (b"# Plan\n\nStatus: pending\n\n## Purpose\nShip the plan.\n\n"
            b"## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n")


class CheckpointTests(unittest.TestCase):
    def decide(self, data=TEMPLATE, action="approved", notes="Review rationale", number=1, **kwargs):
        return cp.prepare("plan", data, action, notes, decision_id=f"{number:032x}",
                          decided_at="2026-10-22T12:00:00Z", source="cli", **kwargs)

    def assert_rejected(self, data):
        with self.assertRaisesRegex(cp.CheckpointError, "^Cannot edit checkpoint"):
            cp.inspect("plan", data)

    def replace_record(self, data, change, kind="decision"):
        marker = f"<!-- coven-checkpoint-{kind}:v1\n".encode()
        start = data.index(marker) + len(marker)
        end = data.index(b"\n-->\n", start)
        value = json.loads(data[start:end])
        change(value)
        return data[:start] + json.dumps(value).encode() + data[end:]

    def test_approve_suggest_approve_keeps_exact_scope_and_history(self):
        original = cp.inspect("plan", TEMPLATE)
        data = TEMPLATE
        snapshots = {original.revision}
        for number, action in enumerate(("approved", "changes_requested", "approved"), 1):
            previous = data
            old = cp.inspect("plan", previous)
            data, view = self.decide(data, action, number=number)
            self.assertEqual(view.scope_revision, original.scope_revision)
            self.assertEqual(view.decision["reviewed_revision"], old.revision)
            self.assertEqual(view.decision["scope_revision"], view.scope_revision)
            self.assertEqual(view.effective_status, action)
            self.assertEqual(view.binding, "bound")
            self.assertEqual(len(view.history), number)
            self.assertEqual(view.history[-1]["notes"], "Review rationale")
            for item in old.history:
                self.assertIn(cp._block("history", item), data)
            snapshots.add(view.revision)
        self.assertEqual(len(snapshots), 4)
        self.assertEqual(json.loads(json.dumps(view.to_dict())), view.to_dict())

    def test_every_covered_byte_matters(self):
        extra = ("\nCustom prose: caf\u00e9.  \n\n```markdown\nStatus: example\n"
                 "## User decision\n- [x] Approved\n```\n"
                 "\n## Approval\nOld human rationale.\n"
                 "- Prior list\n  - [x] Approved\n"
                 "    Status: indented example\n").encode()
        original = TEMPLATE + extra
        data, bound = self.decide(original)
        self.assertIn(extra, data)
        for old, new in ((b"Ship the plan.", b"Ship the plan!"),
                         (b"Custom prose", b"Custom Prose"),
                         (b".  \n", b". \n"), (b"Status: example", b"Status: other"),
                         (b"Old human rationale.", b"Old human rationale!"),
                         (b"  - [x] Approved", b"  - [ ] Approved"),
                         (b"indented example", b"indented changed"),
                         (b"caf\xc3\xa9", b"cafe")):
            with self.subTest(old=old):
                changed = data.replace(old, new)
                stale = cp.inspect("plan", changed)
                self.assertNotEqual(stale.scope_revision, bound.scope_revision)
                self.assertEqual(stale.effective_status, "approval_stale")
                repaired, final = self.decide(changed, number=2)
                self.assertEqual(final.scope_revision, stale.scope_revision)
                self.assertEqual(final.effective_status, "approved")
                self.assertEqual(cp.inspect("plan", repaired).revision, final.revision)

    def test_crlf_whitespace_and_unicode_survive_in_place(self):
        original = TEMPLATE.replace(b"\n", b"\r\n").replace(
            b"Status: pending", b"  sTaTuS: PENDING \t").replace(
            b"- [ ] Approved", b"-\t[ ]\tApproved \t") + "\r\nPurpose: \u96ea\r\n".encode()
        data, view = self.decide(original)
        self.assertEqual(view.scope_revision, cp.inspect("plan", original).scope_revision)
        self.assertIn(b"Status: approved\r\n", data)
        self.assertIn(b"-\t[x]\tApproved \t\r\n", data)
        self.assertIn("Purpose: \u96ea\r\n".encode(), data)
        stale = cp.inspect("plan", data.replace(b"\r\n", b"\n"))
        self.assertEqual(stale.effective_status, "approval_stale")

    def test_snapshot_captures_machine_changes_but_scope_does_not(self):
        data, view = self.decide()
        changed = self.replace_record(data, lambda value: value.update(decided_at="later"))
        changed = self.replace_record(changed, lambda value: value.update(notes="Other rationale"), "history")
        final = cp.inspect("plan", changed)
        self.assertNotEqual(final.revision, view.revision)
        self.assertEqual(final.scope_revision, view.scope_revision)
        self.assertEqual(final.effective_status, "approved")

    def test_missing_empty_and_id_domains_and_revision_validation(self):
        missing = cp.inspect("plan", None)
        empty = cp.inspect("plan", b"")
        other = cp.inspect("other", b"")
        self.assertIsNone(missing.content)
        self.assertIsNone(missing.scope_revision)
        self.assertEqual(missing.revision, "checkpoint-missing-v1:plan")
        self.assertEqual(len({missing.revision, empty.revision, other.revision}), 3)
        self.assertNotEqual(empty.scope_revision, other.scope_revision)
        for token in (None, missing.revision, empty.revision):
            self.assertEqual(cp.validate_revision(token), token)
        for token in (empty.scope_revision, "checkpoint-missing-v1:../bad", "", False,
                      "checkpoint-snapshot-v1:" + "A" * 64, "checkpoint-snapshot-v1:" + "a" * 63):
            with self.subTest(token=token), self.assertRaises(cp.CheckpointError):
                cp.validate_revision(token)
        data, created = self.decide(None, initial_content=TEMPLATE)
        self.assertEqual(created.decision["reviewed_revision"], missing.revision)
        self.assertEqual(created.scope_revision, cp.inspect("plan", TEMPLATE).scope_revision)
        self.assertEqual(cp.inspect("plan", data).effective_status, "approved")
        with self.assertRaises(cp.CheckpointError):
            self.decide(None)

    def test_legacy_inspection_does_not_mutate_or_grant_bound_approval(self):
        data = TEMPLATE.replace(b"pending", b"APPROVED")
        view = cp.inspect("plan", data)
        self.assertEqual(view.content.encode(), data)
        self.assertEqual(view.binding, "legacy_unbound")
        self.assertEqual(view.effective_status, "approved_unbound")
        self.assertIsNone(view.decision)
        self.assertEqual(view.history, [])
        for status in (b"PENDING", b"Changes_Requested", b"custom", b"  "):
            view = cp.inspect("plan", TEMPLATE.replace(b"pending", status))
            self.assertEqual(view.effective_status, status.decode().lower().strip() or "unknown")

    def test_contradictory_status_and_present_checkboxes_are_repairable(self):
        data, view = self.decide()
        for old, new in ((b"Status: approved", b"Status: pending"),
                         (b"- [x] Approved", b"- [ ] Approved"),
                         (b"- [ ] Rejected / needs changes", b"- [x] Rejected / needs changes")):
            with self.subTest(old=old):
                changed = data.replace(old, new)
                invalid = cp.inspect("plan", changed)
                self.assertEqual(invalid.scope_revision, view.scope_revision)
                self.assertEqual(invalid.binding, "inconsistent")
                self.assertEqual(invalid.effective_status, "decision_invalid")
                self.assertTrue(invalid.reason)
                _, repaired = self.decide(changed, number=2)
                self.assertEqual(repaired.effective_status, "approved")
                self.assertEqual(repaired.scope_revision, view.scope_revision)

    def test_changes_request_remains_effective_after_scope_edit(self):
        data, _ = self.decide(action="changes_requested")
        view = cp.inspect("plan", data.replace(b"Ship the plan.", b"Different plan."))
        self.assertEqual(view.effective_status, "changes_requested")
        self.assertEqual(view.binding, "bound")
        self.assertTrue(view.reason)

    def test_missing_fields_do_not_invent_covered_sections(self):
        for original in (b"", b"# Plan", b"# Plan\n\nCustom prose.\n",
                         b"# Plan\n## User decision\nCustom decision prose.\n",
                         b"# Plan\nStatus: pending", b"## User decision\n- [ ] Approved"):
            with self.subTest(original=original):
                data, view = self.decide(original)
                self.assertEqual(view.scope_revision, cp.inspect("plan", original).scope_revision)
                self.assertEqual(data.count(b"## User decision"), original.count(b"## User decision"))
                self.assertEqual(data.count(b"Rejected / needs changes"), original.count(b"Rejected / needs changes"))
                self.assertEqual(view.effective_status, "approved")

    def test_code_comments_and_next_heading_are_not_current_fields(self):
        extra = (b"\n### Archive\n- [x] Approved\nStatus: old\n"
                 b"<!--\n## User decision\nStatus: hidden\n-->\n"
                 b"~~~markdown\n<!-- coven-checkpoint-decision:v99\n"
                 b"## User decision\nStatus: example\n~~~\n"
                 b"    <!-- coven-checkpoint-history:v99\n")
        data, view = self.decide(TEMPLATE + extra)
        self.assertIn(extra, data)
        self.assertEqual(view.scope_revision, cp.inspect("plan", TEMPLATE + extra).scope_revision)

    def test_nested_header_sections_and_checklists_stay_covered(self):
        nested_header = (b"- Historical example\n  Status: approved\n"
                         b"  ## User decision\n  - [x] Approved\n"
                         b"  ~~~markdown\n  Status: example\n  ~~~\n\n")
        nested_checks = b"- Historical list\n  - [x] Approved\n  - [x] Rejected / needs changes\n"
        original = nested_header + TEMPLATE + nested_checks
        data, bound = self.decide(original)
        self.assertIn(nested_header, data)
        self.assertIn(nested_checks, data)
        self.assertEqual(bound.scope_revision, cp.inspect("plan", original).scope_revision)
        stale = cp.inspect("plan", data.replace(b"  Status: approved", b"  Status: pending"))
        self.assertEqual(stale.effective_status, "approval_stale")
        stale = cp.inspect("plan", data.replace(b"  - [x] Approved", b"  - [ ] Approved"))
        self.assertEqual(stale.effective_status, "approval_stale")

    def test_notes_roundtrip_and_previous_human_history_is_untouched(self):
        notes = "Rationale: \u96ea, caf\u00e9 <tag>\n```md\n## User decision\n```\n- Prior\n  - [x] Approved"
        human = b"\n## Changes requested\nOld rationale.\n```md\n- [x] Approved\n```\n"
        data, view = self.decide(TEMPLATE + human, notes=notes)
        self.assertEqual(view.history[0]["notes"], notes)
        self.assertIn(human, data)
        self.assertNotIn(b"<tag>", data)
        self.assertIn(b"\\u003ctag\\u003e", data)
        old_history = b"\n" + cp._block("history", view.history[0])
        data, _ = self.decide(data, "changes_requested", number=2)
        self.assertIn(old_history, data)
        self.assertIn(human, data)

    def test_relocating_history_into_reviewed_structure_cannot_preserve_approval(self):
        original = TEMPLATE + b"\n## Do not deploy\nProse words and `inline code`."
        data, approved = self.decide(original)
        history = b"\n" + cp._block("history", approved.history[-1])
        without_history = data.replace(history, b"", 1)
        for needle, split in ((b"Do not deploy", 2), (b"Prose words", 5), (b"inline code", 6)):
            with self.subTest(needle=needle):
                offset = without_history.index(needle) + split
                moved = without_history[:offset] + history + without_history[offset:]
                view = cp.inspect("plan", moved)
                self.assertNotEqual(view.scope_revision, approved.scope_revision)
                self.assertEqual(view.effective_status, "approval_stale")
        appended = data + b"New covered notes without final newline"
        stale = cp.inspect("plan", appended)
        self.assertEqual(stale.effective_status, "approval_stale")
        _, rebound = self.decide(appended, number=2)
        self.assertEqual(rebound.scope_revision, stale.scope_revision)
        self.assertEqual(rebound.effective_status, "approved")

    def test_invalid_notes_rejected_before_formatting(self):
        for notes in ("```python\nunfinished", "~~~\nunfinished", "## User decision\n- [x] Approved",
                      "<!-- coven-checkpoint-history:v1", "example coven-checkpoint-any", "raw -->",
                      None, "\ud800"):
            with self.subTest(notes=repr(notes)), self.assertRaisesRegex(cp.CheckpointError, "^Cannot edit checkpoint"):
                self.decide(notes=notes)

    def test_ambiguous_structure_and_unclosed_fences_reject(self):
        for data in (TEMPLATE.replace(b"Status: pending", b"Status: pending\nStatus: approved"),
                     TEMPLATE + b"\n## User decision\n",
                     TEMPLATE + b"- [x] Approved\n",
                     TEMPLATE + b"```\nunclosed\n", TEMPLATE + b"~~~~\nunclosed\n"):
            with self.subTest(data=data):
                self.assert_rejected(data)
                with self.assertRaises(cp.CheckpointError):
                    self.decide(data)

    def test_strict_metadata_and_history_fail_closed(self):
        data, view = self.decide()
        bad_fields = (("version", True), ("version", 2), ("checkpoint", "other"),
                      ("checkpoint", "../plan"), ("decision_id", "no"),
                      ("action", "pending"), ("action", []), ("scope_revision", view.revision),
                      ("reviewed_revision", None), ("reviewed_revision", view.scope_revision),
                      ("reviewed_revision", "checkpoint-missing-v1:other"),
                      ("source", "api"), ("source", {}), ("decided_at", 3), ("extra", True))
        for kind in ("decision", "history"):
            for field, value in bad_fields:
                with self.subTest(kind=kind, field=field, value=value):
                    self.assert_rejected(self.replace_record(data, lambda record: record.update({field: value}), kind))
            self.assert_rejected(self.replace_record(data, lambda record: record.pop("source"), kind))
            marker = f"<!-- coven-checkpoint-{kind}:v1".encode()
            self.assert_rejected(data.replace(marker, marker[:-1] + b"2"))
            self.assert_rejected(data.replace(marker, marker + b" trailing"))
            start = data.index(marker) + len(marker) + 1
            self.assert_rejected(data[:start] + b'{"version":1,' + data[start + 1:])
            self.assert_rejected(data[:start] + b'{"extra":NaN,' + data[start + 1:])
        self.assert_rejected(cp._block("decision", view.decision) + data)
        self.assert_rejected(data + b"\n" + cp._block("history", view.history[0]))
        self.assert_rejected(TEMPLATE + b"\n" + cp._block("decision", view.decision))
        self.assert_rejected(TEMPLATE + b"\n<!-- coven-checkpoint-history:v1\n{}\n")
        self.assert_rejected(data.replace(b"\n-->\n", b"\n--x\n", 1))
        with self.assertRaises(cp.CheckpointError):
            cp.inspect("other", data)
        with self.assertRaises(cp.CheckpointError):
            self.decide(data, number=1)

    def test_invalid_api_inputs_raise_checkpoint_errors(self):
        self.assertTrue(issubclass(cp.CheckpointError, storage.StorageError))
        for checkpoint in ("../plan", "", None, "\u96ea"):
            with self.assertRaises(cp.CheckpointError):
                cp.inspect(checkpoint, TEMPLATE)
        self.assert_rejected(b"\xff")
        for field, value in (("action", "pending"), ("decision_id", "abc"),
                             ("source", "other"), ("decided_at", False)):
            args = dict(checkpoint="plan", data=TEMPLATE, action="approved", notes="",
                        decision_id="a" * 32, decided_at="now", source="console")
            args[field] = value
            with self.subTest(field=field), self.assertRaises(cp.CheckpointError):
                cp.prepare(**args)

    def test_direct_script_import_has_no_cli_or_native_runtime_dependency(self):
        runner = Path(__file__).resolve().parents[1] / "runner"
        result = subprocess.run([sys.executable, "-c",
                                 "import checkpoints; assert checkpoints.inspect('x', b'').checkpoint == 'x'"],
                                cwd=runner, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIs(importlib.import_module("runner.checkpoints"), cp)


if __name__ == "__main__":
    unittest.main()
