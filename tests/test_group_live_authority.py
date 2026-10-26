"""Offline live attachment authority tests; every workspace is disposable."""

from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import uuid

from runner import groups, storage


RUNTIME = "616fed3e-ae7f-41a0-99e5-80546d659d76"
INSTANCE = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e"
OTHER = "0c1a32bf-1a23-4da1-8234-123456789abc"


class GroupLiveAuthorityTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="coven-live-authority-")
        self.addCleanup(temp.cleanup)
        self.workspace = Path(temp.name).resolve()
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_bytes(b"not parsed")
        self.path = self.workspace / "state" / "groups.json"
        self.endpoint = {"path": str(self.workspace / "kitsune.sock"), "device": 1, "inode": 2}
        groups.create_group(self.workspace, "review", "Review")
        self.add()

    def binding(self, terminal="terminal-1", *, process=False):
        binding = {
            "presentation": {"adapter": "kitsune", "endpoint": deepcopy(self.endpoint),
                             "terminal_id": terminal, "pane_id_at_binding": "pane-1"},
            "evidence": {"provenance": "kitsune.session.snapshot",
                         "observed_at": "2026-10-23T12:00:00Z", "agent_kind_hint": "djinn",
                         "native_reference": None, "native_binding": "unestablished"}}
        if process:
            binding["evidence"].update(
                provenance="kitsune.pane.process_info",
                process_evidence={"interpreter": "coven.kitsune.djinn-process.v1", "matches": [
                    {"pid": 7, "executable_family": "djinn", "rule": "exact_posix_argv0_basename"}]})
        return binding

    def attachment(self, **updates):
        return {"adapter": "djinn_live", "source_contract": "djinn.live-observation.v1",
                "artifact_path": str(self.workspace / "absent" / (RUNTIME + ".json")),
                "runtime_id": RUNTIME, "instance_id": INSTANCE, "session_id": "session:opaque!",
                "attached_at": "2026-10-23T12:00:00Z"} | updates

    def read(self):
        return groups.read_groups(self.workspace)

    def member(self):
        return self.read()["state"]["groups"][0]["members"][0]

    def add(self, key="reviewer", terminal="terminal-1", **options):
        return groups.add_member(self.workspace, "review", key, options.pop("role", "Reviewer"),
                                 options.pop("binding", self.binding(terminal)), **options)

    def attach(self, attachment=None, **options):
        args = {"expected_revision": self.read()["revision"],
                "expected_participant_id": self.member()["participant_id"]} | options
        return groups.attach_live(self.workspace, "review", "reviewer",
                                  self.attachment() if attachment is None else attachment, **args)

    def detach(self, **options):
        args = {"expected_revision": self.read()["revision"],
                "expected_participant_id": self.member()["participant_id"]} | options
        return groups.detach_live(self.workspace, "review", "reviewer", **args)

    def reserve(self):
        return groups.reserve_cycle(self.workspace, "review", self.read()["revision"],
                                    self.endpoint, "coven.groups", "epoch-1",
                                    [m["binding"]["presentation"]["terminal_id"]
                                     for m in self.read()["state"]["groups"][0]["members"]],
                                    allow_new_epoch=False)

    def write(self, state):
        self.path.write_bytes(storage.encode_json(state))

    def test_closed_attachment_shape_types_and_forbidden_source_facts(self):
        valid = self.attachment()
        invalid = [None, [], "attachment", True, {}]
        for key in valid:
            missing = valid.copy()
            del missing[key]
            invalid.append(missing)
            for value in (None, False, 1, [], {}):
                invalid.append(valid | {key: value})
        for key in ("extra", "source", "activity", "sequence", "sampled_at_ms", "valid_for_ms",
                    "sample", "freshness", "ttl", "capabilities", "authorization", "dispatchable"):
            invalid.append(valid | {key: None})
        invalid.extend([valid | {"adapter": "djinn"},
                        valid | {"source_contract": "djinn.live-observation.v2"}])
        revision, participant = self.read()["revision"], self.member()["participant_id"]
        raw = self.path.read_bytes()
        with patch.object(storage, "workspace_lock", side_effect=AssertionError("lock")):
            for index, value in enumerate(invalid):
                with self.subTest(index=index), self.assertRaises(groups.GroupError):
                    groups.attach_live(self.workspace, "review", "reviewer", value,
                                       expected_revision=revision, expected_participant_id=participant)
        self.assertEqual(self.path.read_bytes(), raw)

    def test_uuid_version_variant_and_canonical_spelling(self):
        for key in ("runtime_id", "instance_id"):
            for value in (RUNTIME.upper(), "{" + RUNTIME + "}", RUNTIME.replace("-", ""),
                          str(uuid.uuid1()), "00000000-0000-0000-0000-000000000000",
                          "616fed3e-ae7f-41a0-79e5-80546d659d76",
                          "616fed3e-ae7f-41a0-c9e5-80546d659d76"):
                attachment = self.attachment(**{key: value})
                if key == "runtime_id":
                    attachment["artifact_path"] = "/absent/" + value + ".json"
                with self.subTest(key=key, value=value), self.assertRaises(groups.GroupError):
                    groups._attachment(attachment)
        groups._attachment(self.attachment(instance_id=RUNTIME))

    def test_path_session_timestamp_bounds_and_pure_validation(self):
        name = RUNTIME + ".json"
        invalid_paths = ("", "/", "relative/" + name, "//absent/" + name,
                         "/absent/../" + name, "/absent/./" + name, "/absent//" + name,
                         "/absent/" + name + "/", "/absent/other.json", "/absent/" + name.upper(),
                         "/" + "x" * 4096 + "/" + name, "/\ud800/" + name)
        invalid_paths += tuple("/bad" + c + "/" + name for c in ("\0", "\n", "\t", "\x1f", "\x7f", "\x85"))
        with patch.object(os, "stat", side_effect=AssertionError("stat")), patch.object(
                os, "lstat", side_effect=AssertionError("lstat")), patch.object(
                os, "open", side_effect=AssertionError("open")), patch.object(
                Path, "resolve", side_effect=AssertionError("resolve")):
            groups._attachment(self.attachment())
            for path in invalid_paths:
                with self.subTest(path=repr(path)), self.assertRaises(groups.GroupError):
                    groups._attachment(self.attachment(artifact_path=path))
            for session in ("", "a" * 129, "has space", "\n", "\x7f", "\u00e9", "\ud800"):
                with self.subTest(session=repr(session)), self.assertRaises(groups.GroupError):
                    groups._attachment(self.attachment(session_id=session))
            for stamp in ("", "2026-10-23", "2026-10-23T12:00:00", "bad",
                          "2026-10-23T12:00:00+01:00", "x" * 65, "\ud800"):
                with self.subTest(stamp=repr(stamp)), self.assertRaises(groups.GroupError):
                    groups._attachment(self.attachment(attached_at=stamp))
            for stamp in ("1900-01-01T00:00:00Z", "2999-01-01T00:00:00+00:00"):
                groups._attachment(self.attachment(attached_at=stamp, session_id="!" * 128))
            prefix = "/" + "\u00e9" * 2000
            path = prefix + "a" * (4096 - len((prefix + "/" + name).encode("utf-8"))) + "/" + name
            groups._attachment(self.attachment(artifact_path=path))
            with self.assertRaises(groups.GroupError):
                groups._attachment(self.attachment(artifact_path="/a" + path))

    def test_schema_member_shapes_binding_variants_and_read_only_validation(self):
        original = self.read()["state"]
        for version in (1, 2, 3):
            for process in (False, True):
                for live in ("missing", None, self.attachment()):
                    state = deepcopy(original)
                    state["schema_version"] = version
                    member = state["groups"][0]["members"][0]
                    member["binding"] = self.binding(process=process)
                    if live != "missing":
                        member["live_attachment"] = live
                    self.write(state)
                    raw = self.path.read_bytes()
                    valid = (version >= 2 or not process) and ((version == 3) == (live != "missing"))
                    with self.subTest(version=version, process=process, live=live), patch.object(
                            storage, "workspace_lock", side_effect=AssertionError("lock")), patch.object(
                            storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                        if valid:
                            self.assertEqual(self.read()["state"], state)
                        else:
                            with self.assertRaises(groups.GroupError):
                                self.read()
                    self.assertEqual(self.path.read_bytes(), raw)
        state = deepcopy(original)
        state["schema_version"] = 4
        with self.assertRaises(groups.GroupError):
            groups._validate(state)

    def test_promotion_one_write_one_revision_all_members_and_identities(self):
        self.add("second", "terminal-2")
        groups.create_group(self.workspace, "other", "Other")
        groups.add_member(self.workspace, "other", "third", "Role", self.binding("terminal-3"))
        original = self.read()["state"]
        for version in (1, 2):
            state = deepcopy(original)
            state["schema_version"] = version
            if version == 2:
                state["groups"][0]["members"][1]["binding"] = self.binding("terminal-2", process=True)
            self.write(state)
            self.reserve()
            before = self.read()
            expected = deepcopy(before["state"])
            expected["schema_version"] = 3
            expected["content_revision"] += 1
            for group in expected["groups"]:
                for member in group["members"]:
                    member["live_attachment"] = None
            expected["groups"][0]["members"][0]["live_attachment"] = self.attachment()
            publish = storage.atomic_replace_bytes

            def checked(path, data):
                self.assertEqual(self.read(), before)
                self.assertEqual(json.loads(data), expected)
                publish(path, data)

            with self.subTest(version=version), patch.object(
                    storage, "atomic_replace_bytes", side_effect=checked) as writer:
                result = self.attach()
            self.assertEqual(writer.call_count, 1)
            self.assertEqual(self.read()["state"], expected)
            self.assertEqual(result["member"], expected["groups"][0]["members"][0])
            self.assertEqual(set(result), {"outcome", "revision", "group", "member", "delivery",
                                           "authorization", "dispatchable"})
            self.assertEqual(result["authorization"], "none")
            self.assertIs(result["dispatchable"], False)
            self.assertEqual(result["delivery"]["outcome"], "durably_committed")

    def test_malformed_stored_attachments_and_member_shapes_fail_without_rewrite(self):
        self.attach()
        original = self.read()["state"]
        attachment = self.attachment()
        invalid = [True, False, 1, [], {}, "attachment"]
        for field in attachment:
            missing = attachment.copy()
            del missing[field]
            invalid.append(missing)
        invalid += [attachment | {"activity": "busy"}, attachment | {"sequence": 1},
                    attachment | {"attached_at": "not a timestamp"},
                    attachment | {"instance_id": str(uuid.uuid1())},
                    attachment | {"session_id": "has space"}]
        for value in invalid:
            state = deepcopy(original)
            state["groups"][0]["members"][0]["live_attachment"] = value
            self.write(state)
            raw = self.path.read_bytes()
            with self.subTest(value=value), patch.object(
                    storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                for operation in (self.read, self.attach, self.detach, self.add):
                    with self.assertRaises(groups.GroupError):
                        operation()
            self.assertEqual(self.path.read_bytes(), raw)
        for version in (1, 2, 3):
            state = deepcopy(original)
            state["schema_version"] = version
            member = state["groups"][0]["members"][0]
            if version < 3:
                del member["live_attachment"]
            member["extra"] = None
            with self.assertRaises(groups.GroupError):
                groups._validate(state)

    def test_schema_three_retains_authority_caps(self):
        self.attach()
        state = self.read()["state"]
        prototype = deepcopy(state["groups"][0]["members"][0])
        state["groups"] = []
        for group_index in range(4):
            members = []
            for member_index in range(16):
                member = deepcopy(prototype)
                member.update(participant_id=str(uuid.uuid4()), key=f"m{member_index}",
                              binding=self.binding(f"t{group_index}-{member_index}"))
                members.append(member)
            state["groups"].append({"group_id": str(uuid.uuid4()), "key": f"g{group_index}",
                                    "name": "Group", "members": members})
        self.assertEqual(groups._validate(state), state)
        overflow = deepcopy(state)
        extra = deepcopy(prototype)
        extra.update(participant_id=str(uuid.uuid4()), key="extra", binding=self.binding("extra"))
        overflow["groups"][0]["members"].append(extra)
        with self.assertRaises(groups.GroupError):
            groups._validate(overflow)
        overflow = deepcopy(state)
        overflow["groups"].append({"group_id": str(uuid.uuid4()), "key": "extra",
                                   "name": "Extra", "members": [extra]})
        with self.assertRaisesRegex(groups.GroupError, "participant limit"):
            groups._validate(overflow)
        for index in range(4, 16):
            state["groups"].append({"group_id": str(uuid.uuid4()), "key": f"g{index}",
                                    "name": "Group", "members": []})
        groups._validate(state)
        state["groups"].append({"group_id": str(uuid.uuid4()), "key": "overflow",
                                "name": "Group", "members": []})
        with self.assertRaises(groups.GroupError):
            groups._validate(state)

    def test_old_schema_detach_is_unchanged_and_never_promotes(self):
        original = self.read()["state"]
        for version in (1, 2):
            state = deepcopy(original)
            state["schema_version"] = version
            self.write(state)
            raw = self.path.read_bytes()
            with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                result = self.detach()
            self.assertEqual(result["outcome"], "unchanged")
            self.assertNotIn("live_attachment", result["member"])
            self.assertEqual(self.path.read_bytes(), raw)

    def test_schema_three_never_downgrades_and_new_members_are_unattached(self):
        self.attach()
        operations = [lambda: self.add(binding=self.binding(process=True), rebind=True),
                      lambda: self.add(role="Coordinator", binding=self.binding(process=True)),
                      lambda: groups.create_group(self.workspace, "other", "Other"),
                      lambda: self.add("second", "terminal-2"), self.reserve, self.detach,
                      lambda: groups.remove_member(self.workspace, "review", "second"),
                      lambda: self.add(binding=self.binding(), rebind=True)]
        for operation in operations:
            operation()
            state = self.read()["state"]
            self.assertEqual(state["schema_version"], 3)
            for group in state["groups"]:
                for member in group["members"]:
                    self.assertIn("live_attachment", member)
        self.assertIsNone(self.member()["live_attachment"])

    def test_mandatory_revision_and_participant_pins_rejected_before_lock(self):
        revision, participant = self.read()["revision"], self.member()["participant_id"]
        for operation in (groups.attach_live, groups.detach_live):
            positional = [self.workspace, "review", "reviewer"]
            if operation is groups.attach_live:
                positional.append(self.attachment())
            for kwargs in ({}, {"expected_revision": revision}, {"expected_participant_id": participant}):
                with self.assertRaises(TypeError):
                    operation(*positional, **kwargs)
            invalid = [{"expected_revision": value} for value in (None, "", 1, True,
                       groups.UNINITIALIZED, "groups-v3:bad", revision + "x")]
            invalid += [{"expected_participant_id": value} for value in (None, "", True, 1, OTHER.upper())]
            with patch.object(storage, "workspace_lock", side_effect=AssertionError("lock")):
                for options in invalid:
                    with self.subTest(operation=operation.__name__, options=options), self.assertRaises(groups.GroupError):
                        operation(*positional, **({"expected_revision": revision,
                                                  "expected_participant_id": participant} | options))
                if operation is groups.attach_live:
                    for reattach in (None, 1, "true"):
                        with self.assertRaises(groups.GroupError):
                            operation(*positional, expected_revision=revision,
                                      expected_participant_id=participant, reattach=reattach)

    def test_missing_targets_never_create_participants_or_promote(self):
        revision, participant = self.read()["revision"], self.member()["participant_id"]
        raw = self.path.read_bytes()
        for operation in (groups.attach_live, groups.detach_live):
            for group_key, member_key in (("missing", "reviewer"), ("review", "missing")):
                args = [self.workspace, group_key, member_key]
                if operation is groups.attach_live:
                    args.append(self.attachment())
                with patch.object(uuid, "uuid4", side_effect=AssertionError("implicit identity")), patch.object(
                        storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
                    with self.assertRaises(groups.GroupError):
                        operation(*args, expected_revision=revision, expected_participant_id=participant)
                self.assertEqual(self.path.read_bytes(), raw)

    def test_cas_participant_conflicts_and_remove_readd_aba(self):
        before = self.read()
        old_id = self.member()["participant_id"]
        self.add(role="Coordinator")
        for operation in (self.attach, self.detach):
            raw = self.path.read_bytes()
            with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
                with self.assertRaisesRegex(groups.GroupError, "revision conflict"):
                    operation(expected_revision=before["revision"])
                with self.assertRaisesRegex(groups.GroupError, "identity conflict"):
                    operation(expected_participant_id=OTHER)
            self.assertEqual(self.path.read_bytes(), raw)
        self.attach()
        groups.remove_member(self.workspace, "review", "reviewer")
        new = self.add()
        self.assertNotEqual(new["member"]["participant_id"], old_id)
        self.assertIsNone(new["member"]["live_attachment"])
        raw = self.path.read_bytes()
        for operation in (self.attach, self.detach):
            with self.assertRaisesRegex(groups.GroupError, "identity conflict"):
                operation(expected_participant_id=old_id)
            self.assertEqual(self.path.read_bytes(), raw)

    def test_idempotent_retry_ignores_only_timestamp_and_never_refreshes(self):
        first = self.attach()
        raw = self.path.read_bytes()
        later = self.attachment(attached_at="2999-01-01T00:00:00+00:00")
        for reattach in (False, True):
            with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                result = self.attach(later, reattach=reattach)
            self.assertEqual(result["outcome"], "unchanged")
            self.assertEqual(result["member"], first["member"])
            self.assertEqual(result["revision"], first["revision"])
            self.assertEqual(self.path.read_bytes(), raw)
        original = self.attachment()
        self.assertEqual(groups._stable_attachment(original), {k: v for k, v in original.items() if k != "attached_at"})
        self.assertEqual(original, self.attachment())

    def test_changed_pins_and_path_require_reattach_not_rebind(self):
        changes = [self.attachment(instance_id=OTHER), self.attachment(session_id="other"),
                   self.attachment(artifact_path=str(self.workspace / "elsewhere" / (RUNTIME + ".json"))),
                   self.attachment(runtime_id=OTHER, artifact_path="/absent/" + OTHER + ".json")]
        for attachment in changes:
            self.attach(self.attachment(), reattach=True)
            before = self.read()
            raw = self.path.read_bytes()
            with self.assertRaisesRegex(groups.GroupError, "explicit reattach"):
                self.attach(attachment)
            self.assertEqual(self.path.read_bytes(), raw)
            result = self.attach(attachment, reattach=True)
            self.assertEqual(result["member"]["live_attachment"], attachment)
            self.assertEqual(self.read()["state"]["content_revision"], before["state"]["content_revision"] + 1)
            self.assertNotEqual(result["revision"], before["revision"])

    def test_binding_invalidation_matrix(self):
        original = self.read()["state"]
        cases = []
        for change in ("role", "time", "pane", "noop", "terminal", "endpoint_path", "device", "inode",
                       "kind", "native", "process", "process_pid", "process_family"):
            base = self.binding(process=change.startswith("process_"))
            binding = deepcopy(base)
            role, stable_change = "Reviewer", True
            if change in ("role", "time", "pane", "noop"):
                stable_change = False
                if change == "role":
                    role = "Coordinator"
                if change == "time":
                    binding["evidence"]["observed_at"] = "2026-10-24T12:00:00Z"
                if change == "pane":
                    binding["presentation"]["pane_id_at_binding"] = "pane-2"
            elif change == "terminal":
                binding["presentation"]["terminal_id"] = "terminal-2"
            elif change.startswith("endpoint_"):
                binding["presentation"]["endpoint"]["path"] += ".other"
            elif change in ("device", "inode"):
                binding["presentation"]["endpoint"][change] += 1
            elif change == "kind":
                binding["evidence"]["agent_kind_hint"] = "other"
            elif change == "native":
                binding["evidence"].update(native_binding="observed_reference", native_reference={
                    "source": "snapshot", "agent": "djinn", "kind": "id", "value": "session"})
            elif change == "process":
                binding = self.binding(process=True)
            else:
                match = binding["evidence"]["process_evidence"]["matches"][0]
                match.update({"pid": 8} if change == "process_pid" else {"executable_family": "djinn-ui"})
            cases.append((change, base, binding, role, stable_change))
        for change, base, binding, role, stable_change in cases:
            for rebind in (False, True):
                self.write(original)
                self.add(binding=base, rebind=True)
                self.attach()
                raw = self.path.read_bytes()
                with self.subTest(change=change, rebind=rebind):
                    if stable_change and not rebind:
                        with self.assertRaisesRegex(groups.GroupError, "explicit rebind"):
                            self.add(binding=binding, role=role)
                        self.assertEqual(self.path.read_bytes(), raw)
                        continue
                    result = self.add(binding=binding, role=role, rebind=rebind)
                    if stable_change:
                        self.assertIs(result["live_attachment_cleared"], True)
                        self.assertIsNone(result["member"]["live_attachment"])
                        again = self.add(binding=base, rebind=True)
                        self.assertNotIn("live_attachment_cleared", again)
                    else:
                        self.assertNotIn("live_attachment_cleared", result)
                        self.assertEqual(result["member"]["live_attachment"], self.attachment())

    def test_detach_only_clears_metadata_and_retry_is_unchanged(self):
        attachment = self.attachment(artifact_path=str(self.workspace / (RUNTIME + ".json")))
        artifact = Path(attachment["artifact_path"])
        artifact.write_bytes(b"opaque native artifact, never read")
        self.attach(attachment)
        before = self.read()
        with patch.object(Path, "unlink", side_effect=AssertionError("delete")):
            result = self.detach()
            self.assertEqual(result["outcome"], "committed")
            self.assertIsNone(result["member"]["live_attachment"])
            self.assertEqual(result["authorization"], "none")
            self.assertIs(result["dispatchable"], False)
            self.assertNotEqual(result["revision"], before["revision"])
            raw = self.path.read_bytes()
            with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
                retry = self.detach()
            self.assertEqual(retry["outcome"], "unchanged")
            self.assertEqual(retry["revision"], result["revision"])
            self.assertEqual(self.path.read_bytes(), raw)
        self.assertEqual(artifact.read_bytes(), b"opaque native artifact, never read")

    def test_binding_invalidation_is_one_atomic_edit_and_failed_write_keeps_attachment(self):
        self.add("second", "terminal-2")
        self.attach()
        second = self.read()["state"]["groups"][0]["members"][1]
        groups.attach_live(self.workspace, "review", "second", self.attachment(instance_id=OTHER),
                           expected_revision=self.read()["revision"],
                           expected_participant_id=second["participant_id"])
        before = self.read()
        raw = self.path.read_bytes()
        changed_binding = self.binding(process=True)
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("prepublish")):
            with self.assertRaises(storage.StorageError):
                self.add(binding=changed_binding, rebind=True)
        self.assertEqual(self.path.read_bytes(), raw)
        expected = deepcopy(before["state"])
        expected["content_revision"] += 1
        expected["groups"][0]["members"][0].update(binding=changed_binding, live_attachment=None)
        publish = storage.atomic_replace_bytes

        def checked(path, data):
            self.assertEqual(self.read(), before)
            self.assertEqual(json.loads(data), expected)
            publish(path, data)

        with patch.object(storage, "atomic_replace_bytes", side_effect=checked) as writer:
            result = self.add(binding=changed_binding, rebind=True)
        self.assertEqual(writer.call_count, 1)
        self.assertIs(result["live_attachment_cleared"], True)
        self.assertEqual(self.read()["state"], expected)

    def test_attachment_read_and_mutations_never_open_or_stat_artifact(self):
        artifact = self.attachment()["artifact_path"]
        lstat, stat, open_regular = os.lstat, os.stat, storage._open_regular

        def guard(function):
            def checked(path, *args, **kwargs):
                self.assertFalse(str(path).startswith(str(Path(artifact).parent)))
                return function(path, *args, **kwargs)
            return checked

        with patch.object(os, "lstat", side_effect=guard(lstat)), patch.object(
                os, "stat", side_effect=guard(stat)), patch.object(
                storage, "_open_regular", side_effect=guard(open_regular)):
            self.attach()
            self.read()
            self.reserve()
            self.detach()

    def test_groups_v1_digest_domain_is_unchanged_and_counters_preserve_attachment(self):
        self.attach()
        before = self.read()
        state = before["state"]
        content = {key: state[key] for key in ("workspace_id", "content_revision", "groups")}
        encoded = json.dumps(content, sort_keys=True, ensure_ascii=False, allow_nan=False,
                             separators=(",", ":")).encode("utf-8")
        digest = hashlib.sha256(b"coven.groups.content:v1\0" + encoded).hexdigest()
        self.assertEqual(before["revision"], f"groups-v1:{state['workspace_id']}:{state['content_revision']}:{digest}")
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 1})
        after = self.read()
        self.assertEqual(before["revision"], after["revision"])
        self.assertEqual(before["state"]["groups"], after["state"]["groups"])
        self.assertEqual(after["state"]["schema_version"], 3)

    def test_counter_update_between_preflight_and_lock_keeps_fresh_counters(self):
        for operation in (self.attach, self.detach):
            expected = self.read()["revision"]
            lock = storage.workspace_lock
            fired = False

            @contextmanager
            def interleave(*args, **kwargs):
                nonlocal fired
                if not fired:
                    fired = True
                    self.reserve()
                with lock(*args, **kwargs) as held:
                    yield held

            with patch.object(storage, "workspace_lock", side_effect=interleave):
                result = operation(expected_revision=expected)
            self.assertEqual(result["outcome"], "committed")
            self.assertEqual(self.read()["state"]["publication_counters"][0]["watermarks"],
                             {"terminal-1": 1 if operation == self.attach else 2})

    def test_under_lock_content_reload_rejects_stale_pin_without_retry(self):
        for operation in (self.attach, self.detach):
            before = self.read()
            participant = self.member()["participant_id"]
            lock = storage.workspace_lock
            fired = False
            concurrent = None

            @contextmanager
            def interleave(*args, **kwargs):
                nonlocal fired, concurrent
                if not fired:
                    fired = True
                    groups.remove_member(self.workspace, "review", "reviewer")
                    self.add()
                    concurrent = self.path.read_bytes()
                with lock(*args, **kwargs) as held:
                    yield held

            with patch.object(storage, "workspace_lock", side_effect=interleave):
                with self.assertRaisesRegex(groups.GroupError, "revision conflict"):
                    operation(expected_revision=before["revision"], expected_participant_id=participant)
            self.assertEqual(self.path.read_bytes(), concurrent)

    def test_concurrent_attach_has_one_cas_winner(self):
        revision, participant = self.read()["revision"], self.member()["participant_id"]

        def attempt(index):
            try:
                return groups.attach_live(self.workspace, "review", "reviewer",
                                          self.attachment(session_id=f"session-{index}"),
                                          expected_revision=revision, expected_participant_id=participant)
            except groups.GroupError:
                return None

        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(attempt, range(4)))
        self.assertEqual(sum(result is not None for result in results), 1)
        self.assertEqual(self.read()["state"]["content_revision"], 3)

    def test_failed_promotion_overflow_and_prepublish_leave_original_bytes(self):
        original = self.read()["state"]
        for version in (1, 2):
            state = deepcopy(original)
            state["schema_version"] = version
            self.write(state)
            raw = self.path.read_bytes()
            with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("prepublish")):
                with self.assertRaises(storage.StorageError):
                    self.attach()
            self.assertEqual(self.path.read_bytes(), raw)
            state["content_revision"] = 2**64 - 1
            self.write(state)
            raw = self.path.read_bytes()
            with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("write")):
                with self.assertRaisesRegex(groups.GroupError, "overflow"):
                    self.attach()
            self.assertEqual(self.path.read_bytes(), raw)

    def test_uncertain_commit_does_not_rollback_attach_detach_or_invalidation(self):
        publish = storage.atomic_replace_bytes

        def uncertain(path, data):
            publish(path, data)
            raise storage.CommitUncertain(path)

        operations = [self.attach, self.detach, self.attach,
                      lambda: self.add(binding=self.binding(process=True), rebind=True)]
        for operation in operations:
            before = self.read()["state"]["content_revision"]
            with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain) as writer:
                result = operation()
            self.assertEqual(writer.call_count, 1)
            self.assertEqual(result["outcome"], "committed")
            self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
            self.assertEqual(result["revision"], self.read()["revision"])
            self.assertEqual(result["member"], self.member())
            self.assertEqual(self.read()["state"]["schema_version"], 3)
            self.assertEqual(self.read()["state"]["content_revision"], before + 1)


if __name__ == "__main__":
    unittest.main()
