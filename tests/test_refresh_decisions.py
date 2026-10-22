"""Refresh preserves authored state; current decisions cannot be revived by history."""

import json
from unittest import mock

from runner import coven
from test_coven import CovenTestCase, snapshot


class RefreshDecisionTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace)
        guard = mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("Unexpected runtime call"))
        self.runtime = guard.start()
        self.addCleanup(guard.stop)
        sender = mock.patch.object(coven, "send_to_agent_pane", return_value=False)
        self.sender = sender.start()
        self.addCleanup(sender.stop)
        status = mock.patch.object(coven, "agent_runtime_status", return_value=("stopped", None))
        status.start()
        self.addCleanup(status.stop)

    def decision(self, checkpoint="plan-approval"):
        content = (self.workspace / f"checkpoints/{checkpoint}.md").read_text()
        return content.split("## User decision", 1)[1].split("\n## ", 1)[0]

    def generated_names(self):
        return {"dashboard.md", "tmux/coven.toml"} | {
            f"prompts/{agent}.start.txt" for agent in self.read_json("coven.json")["agents"]
        }

    def test_refresh_only_rewrites_generated_outputs(self):
        tasks = self.read_json("state/tasks.json")
        tasks[0]["id"] = "T001"  # Exact title formerly triggered an implicit migration.
        tasks.append({**tasks[0], "id": "plan-checkpoint", "notes": "Do not merge or replace me"})
        (self.workspace / "state/tasks.json").write_text(json.dumps(tasks, indent=3) + "\n")
        (self.workspace / "state/lead.json").write_text('{"agent":"srikanth","set_at":"old"}\n')
        for relative in ("goal.md", "spec.md", "agents/jamal.md", "prompts/jamal.md",
                         "work/README.md", "output/README.md", "checkpoints/plan-approval.md"):
            with (self.workspace / relative).open("a") as handle:
                handle.write("\nUser-authored notes and rationale; keep byte-for-byte.\n")
        for relative in ("work/dirty.py", "output/untracked.txt"):
            (self.workspace / relative).write_text("Uncommitted user work\n")
        coven.append_event(self.workspace, "checkpoint.approved", checkpoint="plan-approval")
        coven.append_message(self.workspace, "user", "jamal", "Preserve this discussion")
        for relative in self.generated_names():
            (self.workspace / relative).write_text("Stale generated output\n")
        before = snapshot(self.workspace)
        authored = {name: value for name, value in before.items() if name not in self.generated_names()}
        for _ in range(2):
            self.invoke("refresh", self.workspace)
            after = snapshot(self.workspace)
            self.assertEqual({name: value for name, value in after.items() if name not in self.generated_names()}, authored)
            self.assertEqual(set(before), set(after))
            for relative in self.generated_names():
                self.assertNotEqual((self.workspace / relative).read_text(), "Stale generated output\n")
        self.assertEqual(self.read_json("state/tasks.json"), tasks)
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "pending")
        self.sender.assert_not_called()
        self.runtime.assert_not_called()

    def test_refresh_does_not_recreate_missing_decisions_or_guidance(self):
        self.invoke("approve", self.workspace, "plan-approval")
        self.invoke("approve", self.workspace, "custom")
        removed = ("checkpoints/plan-approval.md", "checkpoints/custom.md",
                   "checkpoints/pre-delivery-review.md", "work/README.md", "output/README.md", "goal.md")
        for relative in removed:
            (self.workspace / relative).unlink()
        events = (self.workspace / "logs/events.jsonl").read_bytes()
        self.invoke("refresh", self.workspace)
        for relative in removed:
            self.assertFalse((self.workspace / relative).exists(), relative)
        self.assertEqual((self.workspace / "logs/events.jsonl").read_bytes(), events)
        self.assertIn("| `plan-approval` | missing |", (self.workspace / "dashboard.md").read_text())

    def test_refresh_can_recreate_generated_files_but_preserves_existing_prompts(self):
        (self.workspace / "prompts/clippy.md").write_text("Custom role prompt\n")
        for relative in ("prompts/jamal.md", "prompts/jamal.start.txt", "tmux/coven.toml", "dashboard.md"):
            (self.workspace / relative).unlink()
        (self.workspace / "tmux").rmdir()
        self.invoke("refresh", self.workspace)
        self.assertEqual((self.workspace / "prompts/clippy.md").read_text(), "Custom role prompt\n")
        self.assertIn("Senior developer.", (self.workspace / "prompts/jamal.md").read_text())
        self.assertTrue((self.workspace / "tmux/coven.toml").is_file())
        first = {name: value[:2] for name, value in snapshot(self.workspace).items()}
        self.invoke("refresh", self.workspace)
        second = {name: value[:2] for name, value in snapshot(self.workspace).items()}
        self.assertEqual(first, second)

    def test_approval_change_request_stays_revoked_after_repeated_refresh(self):
        self.invoke("approve", self.workspace, "plan-approval", "--message", "Original approval rationale")
        other = (self.workspace / "checkpoints/pre-delivery-review.md").read_bytes()
        self.invoke("suggest", self.workspace, "plan-approval", "Add executable QA evidence")
        path = self.workspace / "checkpoints/plan-approval.md"
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "changes_requested")
        self.assertIn("- [ ] Approved", self.decision())
        self.assertIn("- [x] Rejected / needs changes", self.decision())
        self.assertIn("Original approval rationale", path.read_text())
        self.assertIn("Add executable QA evidence", path.read_text())
        preserved = path.read_bytes()
        logs = {name: (self.workspace / f"logs/{name}.jsonl").read_bytes() for name in ("events", "messages")}
        for _ in range(2):
            self.invoke("refresh", self.workspace)
            self.assertEqual(path.read_bytes(), preserved)
            for name, content in logs.items():
                self.assertEqual((self.workspace / f"logs/{name}.jsonl").read_bytes(), content)
        self.assertEqual((self.workspace / "checkpoints/pre-delivery-review.md").read_bytes(), other)
        self.assertIn("| `plan-approval` | changes_requested |", self.invoke("status", self.workspace))
        self.assertIn("`plan-approval` — changes_requested", self.invoke("standup", self.workspace))
        self.assertIn("plan-approval", coven.checkpoint_review_candidates(self.workspace))
        self.assertIn("approve checkpoint `plan-approval`", self.invoke("next", self.workspace))

    def test_explicit_reapproval_clears_rejection_and_preserves_history(self):
        self.invoke("approve", self.workspace, "plan-approval", "--message", "Approval one")
        self.invoke("suggest", self.workspace, "plan-approval", "Request one")
        self.invoke("suggest", self.workspace, "plan-approval", "Request two")
        self.invoke("approve", self.workspace, "plan-approval", "--message", "Approval two")
        self.invoke("refresh", self.workspace)
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "approved")
        self.assertIn("- [x] Approved", self.decision())
        self.assertIn("- [ ] Rejected / needs changes", self.decision())
        content = (self.workspace / "checkpoints/plan-approval.md").read_text()
        for text in ("Approval one", "Request one", "Request two", "Approval two"):
            self.assertIn(text, content)
        self.assertNotIn("plan-approval", coven.checkpoint_review_candidates(self.workspace))
        self.assertNotIn("approve checkpoint", self.invoke("next", self.workspace))

    def test_suggest_missing_checkpoint_saves_revocation_before_notifications(self):
        def assert_saved(*args):
            self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "changes_requested")
            return False
        self.sender.side_effect = assert_saved
        self.invoke("suggest", self.workspace, "custom", "Need more detail")
        self.assertIn("- [ ] Approved", self.decision("custom"))
        self.assertIn("- [x] Rejected / needs changes", self.decision("custom"))
        self.assertEqual(self.sender.call_count, 5)

    def test_invalid_or_empty_suggestions_do_not_revoke_or_log(self):
        self.invoke("approve", self.workspace, "plan-approval")
        before = snapshot(self.workspace)
        self.sender.reset_mock()
        self.invoke("suggest", self.workspace, "plan-approval", "   ", error="cannot be empty")
        self.invoke("suggest", self.workspace, "plan-approval", "Change", "--agent", "ghost", error="Unknown agent")
        self.assertEqual(snapshot(self.workspace), before)
        self.sender.assert_not_called()

    def test_current_decision_writer_preserves_historical_checklists(self):
        historical = "## Notes\n\n- [x] Approved\n- [x] Rejected / needs changes\nStatus: approved\nKeep this evidence.\n"
        path = self.workspace / "checkpoints/custom.md"
        path.write_text("# Custom\n\nsTaTuS: APPROVED\n\n## User decision\n\n- [X] Approved\n- [x] Rejected / needs changes\n\n" + historical)
        self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "approved_unbound")
        self.invoke("suggest", self.workspace, "custom", "Reconsider")
        self.assertIn(historical, path.read_text())
        self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "changes_requested")
        self.assertNotIn("- [x] Approved", self.decision("custom"))
        self.invoke("approve", self.workspace, "custom")
        self.assertIn(historical, path.read_text())
        self.assertNotIn("- [x] Rejected / needs changes", self.decision("custom"))

    def test_explicit_decision_adds_missing_metadata_without_inferring_history(self):
        path = self.workspace / "checkpoints/custom.md"
        historical = "## Notes\n\nStatus: approved\n- [x] Approved\nHistorical evidence only.\n"
        path.write_text("# Custom checkpoint\n\n" + historical)
        self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "unknown")
        self.invoke("suggest", self.workspace, "custom", "Please revise")
        self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "changes_requested")
        self.assertIn(historical, path.read_text())
        self.invoke("approve", self.workspace, "custom")
        self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "approved")
        self.assertIn(historical, path.read_text())

    def test_fenced_metadata_and_decision_examples_are_not_current_state(self):
        path = self.workspace / "checkpoints/custom.md"
        for fence in ("```", "~~~~"):
            with self.subTest(fence=fence):
                metadata_example = f"{fence}\nStatus: pending\n## User decision\n- [x] Approved\n{fence}\n"
                decision_example = f"{fence}markdown\n## User decision\n\n- [x] Approved\n- [ ] Rejected / needs changes\n{fence}\n"
                path.write_text("# Custom\n\n" + metadata_example + "\nStatus: approved\n\n## Purpose\n\n" + decision_example +
                                "\n## User decision\n\n- [x] Approved\n- [ ] Rejected / needs changes\n\n## Notes\nKeep these notes.\n")
                self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "approved_unbound")
                self.invoke("suggest", self.workspace, "custom", "More QA")
                content = path.read_text()
                self.assertIn(metadata_example, content)
                self.assertIn(decision_example, content)
                self.assertIn("\n## User decision\n\n- [ ] Approved\n- [x] Rejected / needs changes\n", content)
                self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), "changes_requested")
                self.invoke("approve", self.workspace, "custom")
                self.assertIn(metadata_example, path.read_text())
                self.assertIn(decision_example, path.read_text())

    def test_nested_and_fenced_historical_checklists_remain_unchanged(self):
        path = self.workspace / "checkpoints/custom.md"
        fenced = "```markdown\n- [x] Approved\n- [ ] Rejected / needs changes\n```\n"
        historical = "### Previous approval evidence\n\n- [x] Approved\n- [ ] Rejected / needs changes\n\n## Notes\nHistorical status and rationale.\n"
        path.write_text("# Custom\n\nStatus: approved\n\n## User decision\n\n- [x] Approved\n- [ ] Rejected / needs changes\n\n" + fenced + "\n" + historical)
        for command, status in (("suggest", "changes_requested"), ("approve", "approved")):
            extra = ["More QA"] if command == "suggest" else []
            self.invoke(command, self.workspace, "custom", *extra)
            self.assertEqual(coven.checkpoint_status(self.workspace, "custom"), status)
            self.assertIn(fenced, path.read_text())
            self.assertIn(historical, path.read_text())

    def test_ambiguous_decision_layouts_fail_before_writes_or_notifications(self):
        path = self.workspace / "checkpoints/custom.md"
        fixtures = (
            "# Custom\nStatus: approved\nStatus: pending\n## User decision\n- [x] Approved\n",
            "# Custom\nStatus: approved\n## User decision\n- [x] Approved\n## User decision\n- [ ] Approved\n",
            "# Custom\nStatus: approved\n## User decision\n- [x] Approved\n- [ ] Approved\n",
            "# Custom\nStatus: approved\n## User decision\n```markdown\n- [x] Approved\n",
        )
        for content in fixtures:
            path.write_text(content)
            before = snapshot(self.workspace)
            self.sender.reset_mock()
            for command in ("approve", "suggest"):
                with self.subTest(content=content, command=command):
                    extra = ["More QA"] if command == "suggest" else []
                    self.invoke(command, self.workspace, "custom", *extra, error="Cannot edit checkpoint")
                    self.assertEqual(snapshot(self.workspace), before)
                    self.sender.assert_not_called()

    def test_next_uses_current_status_not_historical_approval_events(self):
        self.invoke("approve", self.workspace, "plan-approval")
        path = self.workspace / "checkpoints/plan-approval.md"
        path.write_text(path.read_text().replace("Status: approved", "Status: pending"))
        with (self.workspace / "logs/messages.jsonl").open("a") as handle:
            handle.write(json.dumps({"type": "checkpoint.requested", "checkpoint": "plan-approval"}) + "\n")
        self.assertIn("approve checkpoint `plan-approval`", self.invoke("next", self.workspace))
        self.invoke("approve", self.workspace, "plan-approval")
        self.assertNotIn("approve checkpoint", self.invoke("next", self.workspace))

    def test_incompatible_decision_notes_are_rejected_before_any_effects(self):
        self.invoke("approve", self.workspace, "plan-approval")
        for notes in ("Example:\n```python\npass", "Example:\n~~~\nunfinished", "Prior checklist:\n## User decision\n- [x] Approved"):
            for command in ("approve", "suggest"):
                with self.subTest(notes=notes, command=command):
                    before = snapshot(self.workspace)
                    self.sender.reset_mock()
                    extra = ["--message", notes] if command == "approve" else [notes]
                    self.invoke(command, self.workspace, "plan-approval", *extra, error="Cannot edit checkpoint")
                    self.assertEqual(snapshot(self.workspace), before)
                    self.sender.assert_not_called()
        self.invoke("suggest", self.workspace, "plan-approval", "Valid follow-up")
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "changes_requested")
        self.invoke("approve", self.workspace, "plan-approval")
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "approved")

    def test_next_merges_requests_from_both_logs_by_best_effort_timestamp(self):
        with (self.workspace / "logs/events.jsonl").open("a") as handle:
            handle.write(json.dumps({"type": "checkpoint.requested", "checkpoint": "plan-approval", "ts": "2026-01-01T00:00:00Z"}) + "\n")
        with (self.workspace / "logs/messages.jsonl").open("a") as handle:
            handle.write(json.dumps({"type": "checkpoint.requested", "checkpoint": "pre-delivery-review", "ts": "2026-01-02T00:00:00+00:00"}) + "\n")
        self.assertIn("approve checkpoint `pre-delivery-review`", self.invoke("next", self.workspace))

    def test_console_and_interactive_review_share_decision_transitions(self):
        with mock.patch("builtins.input", side_effect=["approve plan-approval", "y", "suggest plan-approval More QA", "y", "quit"]):
            self.invoke("console", self.workspace)
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "changes_requested")
        with mock.patch("builtins.input", return_value="a"):
            self.invoke("review", self.workspace, "plan-approval")
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "approved")
        with mock.patch("builtins.input", side_effect=["s", "Please check edge cases", "."]):
            self.invoke("review", self.workspace, "plan-approval")
        self.assertEqual(coven.checkpoint_status(self.workspace, "plan-approval"), "changes_requested")
        self.invoke("refresh", self.workspace)
        self.assertIn("More QA", (self.workspace / "checkpoints/plan-approval.md").read_text())
        self.assertIn("Please check edge cases", (self.workspace / "checkpoints/plan-approval.md").read_text())
