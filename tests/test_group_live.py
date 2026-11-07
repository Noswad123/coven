"""Offline live-group orchestration, deadline, and real CLI boundary contracts."""

import contextlib
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest
from unittest import mock

from runner import coven, djinn_live_observation as live, group_live
from runner import group_publisher as publisher, groups, storage
from runner import djinn_observation, launch, workers
from test_djinn_live_observation import (
    Clock, INSTANCE, OTHER, ROOT, RUNTIME, SAMPLED, SECRET, SESSION,
    native_fixture, no_native_or_workspace_io,
)
from test_group_publisher import Clock as PublisherClock, FakeServer, inventory


@unittest.skipUnless(os.name == "posix", "Private POSIX artifact contract")
class GroupLiveTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="coven-group-live-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.root.chmod(0o700)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_bytes(b"not parsed by groups")
        self.authority = self.workspace / "state/groups.json"
        self.private = self.root / "private"
        self.private.mkdir(mode=0o700)
        self.path = self.private / (RUNTIME + ".json")
        self.fixture = native_fixture()
        self.write_source()
        self.clock = Clock()
        for module in (live, group_live):
            patcher = mock.patch.object(module, "time", self.clock)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.endpoint = {"path": str(self.root / "unused.sock"), "device": 1, "inode": 2}
        groups.create_group(self.workspace, "review", "Review")
        self.add()

    def binding(self, terminal="terminal-1"):
        return publisher.binding_from_snapshot({
            "endpoint": self.endpoint, "agents": inventory()["agents"],
            "observed_at": "2026-10-23T12:00:00Z",
        }, "w1:p1") | {"presentation": {
            "adapter": "kitsune", "endpoint": deepcopy(self.endpoint),
            "terminal_id": terminal, "pane_id_at_binding": "w1:p1"}}

    def add(self, key="reviewer", terminal="terminal-1", **options):
        return groups.add_member(self.workspace, "review", key,
            options.pop("role", "Reviewer"), options.pop("binding", self.binding(terminal)), **options)

    def read(self):
        return groups.read_groups(self.workspace)

    def member(self):
        return self.read()["state"]["groups"][0]["members"][0]

    def write_source(self, value=None, path=None):
        path = self.path if path is None else path
        value = self.fixture if value is None else value
        path.write_bytes(value if isinstance(value, bytes) else json.dumps(value).encode())
        path.chmod(0o600)

    def attach(self, **options):
        args = dict(workspace=self.workspace, group_key="review", member_key="reviewer",
                    file_path=str(self.path), runtime_id=RUNTIME, instance_id=INSTANCE, session_id=SESSION)
        return group_live.attach(**(args | options))

    def status(self, **options):
        return group_live.status(self.workspace, "review", **options)

    def reserve(self):
        return groups.reserve_cycle(self.workspace, "review", self.read()["revision"], self.endpoint,
            "coven", "epoch-1", [m["binding"]["presentation"]["terminal_id"]
            for m in self.read()["state"]["groups"][0]["members"]], allow_new_epoch=True)

    def argv(self, action="attach-live"):
        args = ["group", action, str(self.workspace), "review"]
        if action != "status":
            args += ["--member", "reviewer"]
        if action == "attach-live":
            args += ["--file", str(self.path), "--runtime", RUNTIME,
                     "--instance", INSTANCE, "--session", SESSION]
        return args

    def command(self, args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(list(map(str, args))), expected)
        self.assertTrue(output.getvalue().isascii())
        self.assertNotIn("\x1b", output.getvalue())
        self.assertNotIn(SECRET, output.getvalue())
        return json.loads(output.getvalue())

    def assert_unknown(self, row, diagnostic=None, assessment="unavailable"):
        self.assertEqual(row["assessment"], assessment)
        self.assertEqual(row["activity"], "unknown")
        self.assertEqual(row["freshness"], "unknown")
        self.assertEqual(row["expires_in_ms"], 0)
        self.assertIsNone(row["source"])
        self.assert_non_authority(row)
        if diagnostic is not None:
            self.assertEqual(row["diagnostics"], [diagnostic])

    def assert_non_authority(self, value):
        self.assertEqual(value["authorization"], "none")
        self.assertIs(value["dispatchable"], False)
        self.assertEqual(value["native_lifecycle"], "unestablished")
        self.assertEqual(value["job_completion"], "unestablished")
        self.assertNotIn(SECRET, json.dumps(value))

    def assert_top_unavailable(self, report, diagnostic):
        self.assertEqual(report["assessment"], "unavailable")
        self.assertEqual(report["participants"], [])
        self.assertEqual(report["diagnostics"], [diagnostic])
        self.assert_non_authority(report)

    def test_cli_attach_busy_and_fresh_unknown_store_only_declaration(self):
        for activity in ("busy", "unknown"):
            with self.subTest(activity=activity):
                self.fixture["instances"][0]["sessions"][0]["activity"] = activity
                self.write_source()
                result = self.command(self.argv() + ["--json"])
                attachment = result["member"]["live_attachment"]
                self.assertEqual(set(attachment), {"adapter", "source_contract", "artifact_path",
                    "runtime_id", "instance_id", "session_id", "attached_at"})
                self.assertEqual(attachment["artifact_path"], str(self.path))
                self.assertEqual(attachment["runtime_id"], RUNTIME)
                self.assertEqual(attachment["instance_id"], INSTANCE)
                self.assertEqual(attachment["session_id"], SESSION)
                self.assertEqual(attachment["adapter"], "djinn_live")
                self.assertEqual(attachment["source_contract"], live.NATIVE_CONTRACT)
                self.assertEqual(result["authorization"], "none")
                group_live.detach(self.workspace, "review", "reviewer")

    def test_attach_promotion_preserves_all_identities_bindings_and_counters(self):
        self.add("second", "terminal-2")
        groups.create_group(self.workspace, "other", "Other")
        groups.add_member(self.workspace, "other", "third", "Role", self.binding("terminal-3"))
        self.reserve()
        before = self.read()["state"]
        self.attach()
        after = self.read()["state"]
        self.assertEqual(after["schema_version"], 3)
        expected = deepcopy(before)
        expected["schema_version"] = 3
        expected["content_revision"] += 1
        for group in expected["groups"]:
            for member in group["members"]:
                member["live_attachment"] = None
        expected["groups"][0]["members"][0]["live_attachment"] = self.member()["live_attachment"]
        self.assertEqual(after, expected)

    def test_preflight_rejections_precede_any_artifact_reader(self):
        raw = self.authority.read_bytes()
        cases = [{"expected_revision": groups.UNINITIALIZED}, {"group_key": "Missing"},
                 {"member_key": "Bad"}, {"group_key": "missing"}, {"member_key": "missing"},
                 {"runtime_id": RUNTIME.upper()}, {"instance_id": INSTANCE.upper()},
                 {"instance_id": "native-agent-id"}, {"session_id": "has space"}]
        with mock.patch.object(live, "inspect", side_effect=AssertionError("artifact I/O")) as reader:
            for options in cases:
                with self.subTest(options=options), self.assertRaises((ValueError, groups.GroupError)):
                    self.attach(**options)
        reader.assert_not_called()
        self.assertEqual(self.authority.read_bytes(), raw)

    def test_cli_stale_revision_invalid_selector_uuid_and_missing_member_reject_before_io(self):
        raw = self.authority.read_bytes()
        cases = [["--expect-revision", groups.UNINITIALIZED], ["--member", "missing"],
                 ["--member", "Bad"], ["--instance", INSTANCE.upper()], ["--session", "a b"]]
        with mock.patch.object(live, "inspect", side_effect=AssertionError("artifact I/O")) as reader:
            for extra in cases:
                with self.subTest(extra=extra), contextlib.redirect_stderr(io.StringIO()), \
                        self.assertRaises(SystemExit) as raised:
                    coven.main(self.argv() + extra)
                self.assertEqual(raised.exception.code, 1)
        reader.assert_not_called()
        self.assertEqual(self.authority.read_bytes(), raw)

    def test_changed_obvious_pins_need_reattach_before_artifact_io(self):
        self.attach()
        raw = self.authority.read_bytes()
        with mock.patch.object(live, "inspect", side_effect=AssertionError("artifact I/O")):
            for options in ({"instance_id": OTHER}, {"session_id": "other"},
                            {"runtime_id": OTHER, "file_path": str(self.private / (OTHER + ".json"))}):
                with self.subTest(options=options), self.assertRaisesRegex(groups.GroupError, "reattach"):
                    self.attach(**options)
        self.assertEqual(self.authority.read_bytes(), raw)

    def test_automatic_revision_and_participant_cas_are_always_passed(self):
        before = self.read()
        participant = self.member()["participant_id"]
        with mock.patch.object(groups, "attach_live", wraps=groups.attach_live) as attach:
            self.attach()
        self.assertEqual(attach.call_args.kwargs["expected_revision"], before["revision"])
        self.assertEqual(attach.call_args.kwargs["expected_participant_id"], participant)
        before = self.read()
        with mock.patch.object(groups, "detach_live", wraps=groups.detach_live) as detach:
            group_live.detach(self.workspace, "review", "reviewer")
        self.assertEqual(detach.call_args.kwargs["expected_revision"], before["revision"])
        self.assertEqual(detach.call_args.kwargs["expected_participant_id"], participant)

    def test_attach_concurrent_content_edit_or_remove_readd_never_writes_stale_attachment(self):
        inspect = live.inspect
        for mode in ("role", "aba"):
            concurrent = []

            def interleave(*args, **kwargs):
                report = inspect(*args, **kwargs)
                if mode == "aba":
                    groups.remove_member(self.workspace, "review", "reviewer")
                    self.add()
                else:
                    self.add(role="Coordinator")
                concurrent.append(self.authority.read_bytes())
                return report

            with self.subTest(mode=mode), mock.patch.object(live, "inspect", side_effect=interleave):
                with self.assertRaisesRegex(groups.GroupError, "revision conflict"):
                    self.attach()
            self.assertEqual(self.authority.read_bytes(), concurrent[0])
            self.assertIsNone(self.member().get("live_attachment"))

    def test_same_pins_always_fresh_read_then_noop_even_reattach(self):
        first = self.attach()
        raw = self.authority.read_bytes()
        for reattach in (False, True):
            with self.subTest(reattach=reattach), mock.patch.object(live, "inspect", wraps=live.inspect) as reader, \
                    mock.patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                result = self.attach(reattach=reattach)
            reader.assert_called_once()
            self.assertEqual(result["outcome"], "unchanged")
            self.assertEqual(result["member"], first["member"])
            self.assertEqual(result["revision"], first["revision"])
        self.clock.wall_ms += 3000
        with self.assertRaises(groups.GroupError):
            self.attach(reattach=True)
        self.assertEqual(self.authority.read_bytes(), raw)

    def test_parent_alias_spelling_resolves_same_path_without_reattach(self):
        first = self.attach()
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        result = self.attach(file_path=str(alias / "private" / self.path.name))
        self.assertEqual(result["outcome"], "unchanged")
        self.assertEqual(result["member"], first["member"])

    def test_changed_canonical_path_requires_reattach_after_checked_read(self):
        self.attach()
        other = self.root / "other"
        other.mkdir(mode=0o700)
        path = other / self.path.name
        self.write_source(path=path)
        raw = self.authority.read_bytes()
        with mock.patch.object(live, "inspect", wraps=live.inspect) as reader:
            with self.assertRaisesRegex(groups.GroupError, "reattach"):
                self.attach(file_path=str(path))
        reader.assert_called_once()
        self.assertEqual(self.authority.read_bytes(), raw)
        result = self.attach(file_path=str(path), reattach=True)
        self.assertEqual(result["member"]["live_attachment"]["artifact_path"], str(path))

    def test_rejected_sources_never_promote_or_write_membership(self):
        original = self.authority.read_bytes()
        for mode in ("missing", "stale", "private", "schema", "incompatible", "json"):
            self.write_source()
            self.clock.wall_ms = SAMPLED + 100
            if mode == "missing":
                self.path.unlink()
            elif mode == "stale":
                self.clock.wall_ms = SAMPLED + 3000
            elif mode == "private":
                self.path.chmod(0o644)
            elif mode == "schema":
                self.write_source({})
            elif mode == "incompatible":
                self.write_source(self.fixture | {"schema_version": 2})
            else:
                self.write_source(SECRET.encode())
            with self.subTest(mode=mode), mock.patch.object(
                    storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
                with self.assertRaises(groups.GroupError) as raised:
                    self.attach()
            self.assertNotIn(SECRET, str(raised.exception))
            self.assertEqual(self.authority.read_bytes(), original)
            self.assertEqual(self.read()["state"]["schema_version"], 1)

    def test_source_io_is_outside_workspace_lock(self):
        inspect = live.inspect

        def unlocked(*args, **kwargs):
            with storage.workspace_lock(self.workspace, self.workspace / "state/.workspace.lock", timeout=0):
                return inspect(*args, **kwargs)

        with mock.patch.object(live, "inspect", side_effect=unlocked):
            self.attach()
            self.assertEqual(self.status()["participants"][0]["activity"], "busy")

    def test_live_commands_never_probe_native_identity_readiness_or_execute_jobs(self):
        binding = self.binding()
        binding["evidence"].update(native_binding="observed_reference", native_reference={
            "source": "kitsune:djinn", "agent": "djinn", "kind": "id", "value": "unrelated-native-id"})
        self.add(binding=binding, rebind=True)
        with contextlib.ExitStack() as stack:
            for target, name in ((subprocess, "run"), (subprocess, "Popen"), (socket, "socket"),
                                 (djinn_observation, "inspect"), (workers, "inspect_readiness"),
                                 (workers, "run_session_observation_probe"), (workers, "run_help_probe"),
                                 (launch, "execute"), (coven, "load_team"), (coven, "send_to_agent_pane"),
                                 (publisher, "read_publication_snapshot")):
                guard = stack.enter_context(mock.patch.object(target, name,
                    side_effect=AssertionError("unexpected native operation: " + name)))
                stack.callback(guard.assert_not_called)
            attached = self.command(self.argv() + ["--json"])
            self.assertEqual(attached["member"]["binding"], binding)
            row = self.command(self.argv("status") + ["--json"])["participants"][0]
            self.assertEqual(row["association_basis"], "explicit_declaration")
            self.assertNotIn("native_reference", row)
            self.assert_non_authority(row)
            self.command(self.argv("detach-live") + ["--json"])
        self.assertEqual(self.member()["binding"], binding)

    def test_detach_never_reads_deleted_artifact_and_retry_is_noop(self):
        self.attach()
        self.path.unlink()
        with mock.patch.object(live, "inspect", side_effect=AssertionError("artifact I/O")):
            result = self.command(self.argv("detach-live") + ["--json"])
            self.assertIsNone(result["member"]["live_attachment"])
            raw = self.authority.read_bytes()
            retry = self.command(self.argv("detach-live") + ["--json"])
        self.assertEqual(retry["outcome"], "unchanged")
        self.assertEqual(self.authority.read_bytes(), raw)
        self.assertFalse(self.path.exists())

    def test_old_schema_detach_and_status_never_promote(self):
        for version in (1, 2):
            state = self.read()["state"]
            state["schema_version"] = version
            self.authority.write_bytes(storage.encode_json(state))
            raw = self.authority.read_bytes()
            with mock.patch.object(live, "inspect", side_effect=AssertionError("artifact I/O")), \
                    mock.patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
                result = group_live.detach(self.workspace, "review", "reviewer")
                report = self.status()
            self.assertEqual(result["outcome"], "unchanged")
            self.assertEqual(report["authority_schema_version"], version)
            self.assertEqual(report["assessment"], "observed")
            self.assert_unknown(report["participants"][0], "live_source_unattached", "unattached")
            self.assertEqual(self.authority.read_bytes(), raw)

    def test_remove_readd_does_not_inherit_live_attachment(self):
        old = self.attach()["member"]["participant_id"]
        groups.remove_member(self.workspace, "review", "reviewer")
        result = self.add()
        self.assertNotEqual(result["member"]["participant_id"], old)
        self.assertIsNone(result["member"]["live_attachment"])
        with mock.patch.object(live, "inspect", side_effect=AssertionError("implicit source")):
            self.assert_unknown(self.status()["participants"][0], assessment="unattached")

    def test_stable_binding_rebind_clears_attachment_but_presentation_edits_do_not(self):
        self.attach()
        self.reserve()
        participant = self.member()["participant_id"]
        attachment = self.member()["live_attachment"]
        counters = self.read()["state"]["publication_counters"]
        for change in ("role", "time", "pane", "terminal"):
            binding = deepcopy(self.member()["binding"])
            role = self.member()["role"]
            if change == "role":
                role = "Coordinator"
            elif change == "time":
                binding["evidence"]["observed_at"] = "2026-10-24T12:00:00Z"
            elif change == "pane":
                binding["presentation"]["pane_id_at_binding"] = "w2:p9"
            else:
                binding["presentation"]["terminal_id"] = "replacement"
            result = self.add(role=role, binding=binding, rebind=True)
            self.assertEqual(result["member"]["participant_id"], participant)
            self.assertEqual(self.read()["state"]["publication_counters"], counters)
            if change == "terminal":
                self.assertIs(result["live_attachment_cleared"], True)
                self.assertIsNone(result["member"]["live_attachment"])
            else:
                self.assertNotIn("live_attachment_cleared", result)
                self.assertEqual(result["member"]["live_attachment"], attachment)

    def test_static_publisher_never_reads_source_or_changes_label_or_ttl(self):
        with FakeServer() as server:
            self.endpoint = publisher._endpoint(server.path)
            self.add(binding=self.binding(), rebind=True)
            self.attach()
            self.path.unlink()
            attachment = self.member()["live_attachment"]
            with mock.patch.object(live, "inspect", side_effect=AssertionError("live read")), \
                    mock.patch.object(publisher, "time", PublisherClock()):
                result = publisher.publish_group(self.workspace, "review", server.path, "coven", duration=1)
            self.assertEqual(result["acknowledged"], 1)
            params = server.requests[-1]["params"]
            self.assertEqual(params["text"], "Review \u00b7 Reviewer")
            self.assertEqual(params["ttl_ms"], 6000)
            self.assertEqual(self.member()["live_attachment"], attachment)
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot", "pane.annotate"])

    def test_status_exact_projection_preserves_static_metadata_without_equivalence(self):
        member = self.attach()["member"]
        report = self.status()
        self.assertEqual(report["contract"], group_live.STATUS_CONTRACT)
        self.assertEqual(report["revision"], self.read()["revision"])
        self.assertEqual(report["assessment"], "observed")
        self.assert_non_authority(report)
        row, = report["participants"]
        self.assertEqual(set(row), {"participant_id", "key", "role", "live_attachment", "association_basis",
            "assessment", "activity", "activity_scope", "attention", "freshness", "expires_in_ms", "source",
            "diagnostics", "authorization", "dispatchable", "native_lifecycle", "job_completion"})
        self.assertEqual(row["participant_id"], member["participant_id"])
        self.assertEqual(row["live_attachment"], member["live_attachment"])
        self.assertEqual(row["association_basis"], "explicit_declaration")
        self.assertEqual(row["activity"], "busy")
        self.assertEqual(row["attention"], "unknown")
        self.assertEqual(row["activity_scope"], "legacy_runner_ownership")
        self.assertEqual(row["expires_in_ms"], 2900)
        self.assertEqual(row["source"], live.inspect(str(self.path), RUNTIME, INSTANCE, SESSION)["source"])
        self.assert_non_authority(row)
        row["live_attachment"]["session_id"] = "mutated-client-copy"
        self.assertEqual(self.member(), member)
        self.assertIsNone(member["binding"]["evidence"]["native_reference"])

    def test_fresh_unknown_keeps_source_until_expiry_without_idle_or_completion(self):
        self.fixture["instances"][0]["sessions"][0]["activity"] = "unknown"
        self.write_source()
        self.attach()
        first = self.command(self.argv("status") + ["--json"])["participants"][0]
        self.assertEqual(first["assessment"], "observed")
        self.assertEqual(first["activity"], "unknown")
        self.assertEqual(first["freshness"], "fresh_sample")
        self.assertIsNotNone(first["source"])
        self.assertEqual(first["expires_in_ms"], 2900)
        self.assert_non_authority(first)
        self.clock.wall_ms += 3000
        row = self.command(self.argv("status") + ["--json"])["participants"][0]
        self.assert_unknown(row, "stale_sample")

    def test_empty_group_and_optional_member_filter(self):
        groups.create_group(self.workspace, "empty", "Empty")
        report = self.command(["group", "status", self.workspace, "empty", "--json"])
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["participants"], [])
        self.attach()
        self.add("second", "terminal-2")
        with mock.patch.object(live, "inspect", side_effect=AssertionError("filtered out")):
            filtered = self.command(self.argv("status") + ["--member", "second", "--json"])
        self.assertEqual([r["key"] for r in filtered["participants"]], ["second"])
        with self.assertRaisesRegex(groups.GroupError, "member does not exist"):
            self.status(member_key="missing")

    def test_status_read_only_even_with_corrupt_unrelated_state_and_environment(self):
        self.attach()
        for name in ("coven.json", "state/tasks.json", "state/agents.json", "state/task-ledger.json",
                     "state/task-attempts.json", "state/execution.json", "state/runtime-launch-control.json"):
            (self.workspace / name).write_bytes(b"corrupt\xff" + SECRET.encode())
        before = {p.relative_to(self.workspace): p.read_bytes() for p in self.workspace.rglob("*") if p.is_file()}
        source = self.path.read_bytes()
        flags_seen = []
        opened = os.open

        def readonly(path, flags, *args, **kwargs):
            self.assertEqual(flags & os.O_ACCMODE, os.O_RDONLY)
            self.assertFalse(flags & (os.O_CREAT | os.O_TRUNC | os.O_APPEND))
            flags_seen.append(str(path))
            return opened(path, flags, *args, **kwargs)

        with no_native_or_workspace_io(), mock.patch.object(os, "open", side_effect=readonly), \
                mock.patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("write")), \
                mock.patch.dict(os.environ, {k: SECRET for k in ("DJINN_HOME", "DJINN_CONFIG",
                    "KITSUNE_SOCKET_PATH", "KITSUNE_CLIENT_SOCKET_PATH", "COVEN_WORKSPACE")}):
            self.command(self.argv("status") + ["--json"])
        self.assertTrue(flags_seen)
        self.assertTrue(set(flags_seen) <= {str(self.authority), str(self.path)})
        self.assertEqual(before, {p.relative_to(self.workspace): p.read_bytes()
                                 for p in self.workspace.rglob("*") if p.is_file()})
        self.assertEqual(self.path.read_bytes(), source)

    def test_individual_unavailable_incompatible_unattached_rows_still_exit_zero(self):
        self.attach()
        self.add("second", "terminal-2")
        for value, assessment in ((b"bad", "unavailable"),
                                 (self.fixture | {"schema_version": 2}, "incompatible")):
            self.write_source(value)
            with self.subTest(assessment=assessment):
                report = self.command(self.argv("status") + ["--json"])
                self.assertEqual(report["assessment"], "observed")
                self.assert_unknown(report["participants"][0], assessment=assessment)
                self.assert_unknown(report["participants"][1], assessment="unattached")

    def test_status_reader_errors_and_unapproved_metadata_never_leak(self):
        self.attach()
        accepted = live.inspect(str(self.path), RUNTIME, INSTANCE, SESSION)
        accepted["raw_error"] = SECRET
        accepted["source"]["extra_approved_metadata"] = SECRET
        cases = [(OSError(SECRET), "live_source_unavailable"),
                 (ValueError(SECRET), "live_source_unavailable"),
                 ({"assessment": "unavailable", "diagnostics": [SECRET], "raw": SECRET},
                  "live_source_unavailable"), (accepted, None)]
        for value, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic), mock.patch.object(live, "inspect",
                    **({"side_effect": value} if isinstance(value, Exception) else {"return_value": value})):
                report = self.status()
            self.assertNotIn(SECRET, json.dumps(report))
            if diagnostic:
                self.assert_unknown(report["participants"][0], diagnostic)
            else:
                self.assertEqual(report["participants"][0]["activity"], "busy")
                self.assertNotIn("extra_approved_metadata", report["participants"][0]["source"])
        self.write_source(SECRET.encode())
        self.assert_unknown(self.status()["participants"][0], "live_source_unavailable")

    def test_in_process_wrong_report_shape_cannot_attach_or_supply_positive_row(self):
        valid = live.inspect(str(self.path), RUNTIME, INSTANCE, SESSION)
        raw = self.authority.read_bytes()
        invalid = [None, [], valid | {"dispatchable": True}, valid | {"expires_in_ms": True},
                   valid | {"selection": {}}, valid | {"source": {}}, valid | {"activity": "idle"}]
        for value in invalid:
            with self.subTest(value=value), mock.patch.object(live, "inspect", return_value=value):
                with self.assertRaises(groups.GroupError):
                    self.attach()
            self.assertEqual(self.authority.read_bytes(), raw)
        self.attach()
        for value in invalid:
            with self.subTest(status=value), mock.patch.object(live, "inspect", return_value=value):
                row = self.status()["participants"][0]
            self.assert_unknown(row, "invalid_live_report")

    def test_counter_only_drift_between_reads_is_allowed(self):
        self.attach()
        before = self.read()
        inspect = live.inspect

        def reserve_during_read(*args, **kwargs):
            report = inspect(*args, **kwargs)
            self.reserve()
            return report

        with mock.patch.object(live, "inspect", side_effect=reserve_during_read):
            report = self.status()
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["participants"][0]["activity"], "busy")
        self.assertEqual(self.read()["revision"], before["revision"])
        self.assertNotEqual(self.read()["state"]["publication_counters"], before["state"]["publication_counters"])

    def test_whole_authority_recheck_after_all_reads_rejects_every_content_change(self):
        self.attach()
        self.add("second", "terminal-2")
        self.attach(member_key="second")
        original = self.authority.read_bytes()
        inspect = live.inspect
        for mode in ("role", "raw_content", "unrelated", "aba", "corrupt"):
            self.authority.write_bytes(original)
            calls = []
            concurrent = []

            def edit_on_last(*args, **kwargs):
                report = inspect(*args, **kwargs)
                calls.append(args)
                if len(calls) == 2:
                    if mode == "role":
                        self.add(role="Coordinator")
                    elif mode == "raw_content":
                        state = self.read()["state"]
                        state["groups"][0]["members"][0]["role"] = "Coordinator"
                        self.authority.write_bytes(storage.encode_json(state))
                    elif mode == "unrelated":
                        groups.create_group(self.workspace, "other", "Other")
                    elif mode == "aba":
                        groups.remove_member(self.workspace, "review", "reviewer")
                        self.add()
                    else:
                        self.authority.write_bytes(b"broken")
                    concurrent.append(self.authority.read_bytes())
                return report

            with self.subTest(mode=mode), mock.patch.object(live, "inspect", side_effect=edit_on_last):
                report = self.command(self.argv("status") + ["--json"], expected=1)
            self.assertEqual(len(calls), 2)
            self.assert_top_unavailable(report, "groups_unavailable" if mode == "corrupt" else "groups_changed")
            self.assertEqual(self.authority.read_bytes(), concurrent[0])

    def test_timeout_validation_and_serial_sixteen_reads_share_remaining_budget(self):
        for value in (0, -1, 1.001, True, float("nan"), float("inf")):
            with self.subTest(timeout=value), mock.patch.object(groups, "read_groups",
                    side_effect=AssertionError("state I/O")), self.assertRaises(ValueError):
                self.status(timeout=value)
        self.attach()
        for index in range(1, 16):
            self.add(f"m{index}", f"terminal-{index + 1}")
            self.attach(member_key=f"m{index}")
        inspect = live.inspect
        budgets = []
        active = False

        def delayed(*args, **kwargs):
            nonlocal active
            self.assertFalse(active, "source reads must be serial")
            active = True
            budgets.append(kwargs["timeout"])
            report = inspect(*args, **kwargs)
            self.clock.elapsed_ms += 10
            active = False
            return report

        with mock.patch.object(live, "inspect", side_effect=delayed):
            report = self.status(timeout=0.5)
        self.assertEqual(len(budgets), 16)
        for index, budget in enumerate(budgets):
            self.assertAlmostEqual(budget, 0.5 - index * 0.01)
        self.assertEqual(len(report["participants"]), 16)
        self.assertEqual(report["assessment"], "observed")

    def test_shared_timeout_discards_all_rows_instead_of_resetting_per_member(self):
        self.attach()
        self.add("second", "terminal-2")
        self.attach(member_key="second")
        inspect = live.inspect
        budgets = []

        def delayed(*args, **kwargs):
            budgets.append(kwargs["timeout"])
            report = inspect(*args, **kwargs)
            self.clock.elapsed_ms += 600
            return report

        with mock.patch.object(live, "inspect", side_effect=delayed):
            report = self.command(self.argv("status") + ["--json"], expected=1)
        self.assertEqual(len(budgets), 2)
        self.assertAlmostEqual(budgets[0], 1)
        self.assertAlmostEqual(budgets[1], 0.4)
        self.assert_top_unavailable(report, "status_timeout")

    def test_later_member_delay_expires_busy_and_observed_unknown_completely(self):
        self.attach()
        self.add("second", "terminal-2")
        self.attach(member_key="second")
        inspect = live.inspect
        for activity in ("busy", "unknown"):
            self.fixture["instances"][0]["sessions"][0]["activity"] = activity
            self.write_source()
            self.clock.wall_ms = SAMPLED + 2800
            self.clock.elapsed_ms = 0
            calls = []

            def delayed(*args, **kwargs):
                report = inspect(*args, **kwargs)
                calls.append(report)
                if len(calls) == 2:
                    self.clock.elapsed_ms += 250
                return report

            with self.subTest(activity=activity), mock.patch.object(live, "inspect", side_effect=delayed):
                report = self.status()
            self.assertEqual(calls[0]["assessment"], "observed")
            self.assertEqual(calls[0]["activity"], activity)
            self.assertEqual(report["assessment"], "observed")
            self.assert_unknown(report["participants"][0], "source_expired")

    def test_expiry_uses_row_call_start_and_final_wall_age_not_attachment_or_mtime(self):
        self.attach()
        inspect = live.inspect

        def delayed(*args, **kwargs):
            report = inspect(*args, **kwargs)
            self.clock.elapsed_ms += 200
            return report

        with mock.patch.object(live, "inspect", side_effect=delayed):
            self.assertEqual(self.status()["participants"][0]["expires_in_ms"], 2700)
        self.clock.elapsed_ms = 0
        self.clock.wall_ms = SAMPLED + 2800
        read = groups.read_groups
        calls = []

        def advance_wall(*args, **kwargs):
            view = read(*args, **kwargs)
            calls.append(view)
            if len(calls) == 2:
                self.clock.wall_ms += 250
            return view

        with mock.patch.object(groups, "read_groups", side_effect=advance_wall):
            self.assert_unknown(self.status()["participants"][0], "source_expired")
        os.utime(self.path, None)
        self.assert_unknown(self.status()["participants"][0], "stale_sample")
        self.write_source()
        self.assert_unknown(self.status()["participants"][0], "stale_sample")
        # A clock rollback during the authority recheck invalidates an accepted
        # sample too, including a freshly observed unknown rather than just busy.
        for activity in ("busy", "unknown"):
            self.fixture["instances"][0]["sessions"][0]["activity"] = activity
            self.write_source()
            self.clock.wall_ms = SAMPLED + 100
            self.clock.elapsed_ms = 0
            rechecks = []

            def rollback_wall(*args, **kwargs):
                view = read(*args, **kwargs)
                rechecks.append(view)
                if len(rechecks) == 2:
                    self.clock.wall_ms = SAMPLED - 100
                return view

            with self.subTest(future_after_recheck=activity), \
                    mock.patch.object(groups, "read_groups", side_effect=rollback_wall):
                self.assert_unknown(self.status()["participants"][0], "future_sample")

    def test_post_decode_and_post_recheck_deadline_guards(self):
        self.attach()
        decode = live._decode

        def slow_decode(*args):
            result = decode(*args)
            self.clock.elapsed_ms += 1000
            return result

        with mock.patch.object(live, "_decode", side_effect=slow_decode):
            self.assert_top_unavailable(self.status(), "status_timeout")
        self.clock.elapsed_ms = 0
        read = groups.read_groups
        calls = []

        def slow_recheck(*args, **kwargs):
            result = read(*args, **kwargs)
            calls.append(result)
            if len(calls) == 2:
                self.clock.elapsed_ms += 1000
            return result

        with mock.patch.object(groups, "read_groups", side_effect=slow_recheck):
            self.assert_top_unavailable(self.status(), "status_timeout")

    def test_post_projection_and_last_acceptance_clock_guards(self):
        self.attach()
        read = groups.read_groups
        monotonic = self.clock.monotonic
        for boundary in (2, 3):
            self.clock.elapsed_ms = 0
            reads = []
            ticks = []

            def recheck(*args, **kwargs):
                result = read(*args, **kwargs)
                reads.append(result)
                return result

            def final_tick():
                if len(reads) == 2:
                    ticks.append(True)
                    if len(ticks) == boundary:
                        self.clock.elapsed_ms = 1000
                return monotonic()

            with self.subTest(boundary=boundary), mock.patch.object(groups, "read_groups", side_effect=recheck), \
                    mock.patch.object(self.clock, "monotonic", side_effect=final_tick):
                self.assert_top_unavailable(self.status(), "status_timeout")
            self.assertGreaterEqual(len(ticks), boundary)

    def test_cli_requires_explicit_arguments_and_actual_wrapper_workflow_is_offline(self):
        base = self.argv()
        invalid = [["group", "attach-live"], ["group", "status"], ["group", "detach-live"]]
        for flag in ("--member", "--file", "--runtime", "--instance", "--session"):
            index = base.index(flag)
            invalid.append(base[:index] + base[index + 2:])
        with mock.patch.object(live, "inspect", side_effect=AssertionError("dispatch")):
            for args in invalid:
                with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), \
                        self.assertRaises(SystemExit) as raised:
                    coven.main(args)
                self.assertEqual(raised.exception.code, 2)
        env = {k: v for k, v in os.environ.items()
               if k not in {"COVEN_HOME", "COVEN_COMMAND", "PYTHONPATH"}}
        env.update(PYTHONDONTWRITEBYTECODE="1", DJINN_HOME="/never-use", DJINN_CONFIG="/never-use",
                   KITSUNE_SOCKET_PATH="/never-use", KITSUNE_CLIENT_SOCKET_PATH="/never-use")

        def run(args, code=0):
            response = subprocess.run([str(ROOT / "bin/coven"), *map(str, args)], cwd=self.workspace,
                env=env, capture_output=True, timeout=10)
            self.assertEqual(response.returncode, code, response.stderr.decode(errors="replace"))
            self.assertEqual(response.stderr, b"")
            self.assertTrue(response.stdout.isascii())
            self.assertNotIn(b"\x1b", response.stdout)
            self.assertNotIn(SECRET.encode(), response.stdout)
            return json.loads(response.stdout)

        run(["group", "create", self.workspace, "cli", "--name", "CLI Group", "--json"])
        with FakeServer() as server:
            added = run(["group", "add", self.workspace, "cli", "--member", "worker", "--role", "Worker",
                         "--socket", server.path, "--pane", "w1:p1", "--json"])
            self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])
        args = ["group", "attach-live", str(self.workspace), "cli", "--member", "worker",
                "--file", str(self.path), "--runtime", RUNTIME, "--instance", INSTANCE,
                "--session", SESSION, "--json"]
        self.fixture["sampled_at_ms"] = int(time.time() * 1000)
        self.write_source()
        attached = run(args)
        self.assertEqual(attached["member"]["participant_id"], added["member"]["participant_id"])
        status_args = ["group", "status", self.workspace, "cli", "--json"]
        report = run(status_args)
        self.assertEqual(report["participants"][0]["activity"], "busy")
        self.assertGreater(report["participants"][0]["expires_in_ms"], 0)
        self.path.unlink()
        self.assert_unknown(run(status_args)["participants"][0], "io_error")
        detached = run(["group", "detach-live", self.workspace, "cli", "--member", "worker", "--json"])
        self.assertIsNone(detached["member"]["live_attachment"])
        self.assert_unknown(run(status_args)["participants"][0], assessment="unattached")


if __name__ == "__main__":
    unittest.main()
