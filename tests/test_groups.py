"""Disposable association authority fixtures; no live apps or transport I/O."""

from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import uuid

from runner import groups, storage


class GroupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coven-groups-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_bytes(b"not mutable config JSON")
        self.path = self.workspace / "state" / "groups.json"
        self.endpoint = {"path": str(self.root / "kitsune.sock"), "device": 1, "inode": 2}

    def binding(self, terminal="terminal-1", native=None):
        return {"presentation": {"adapter": "kitsune", "endpoint": deepcopy(self.endpoint),
                                 "terminal_id": terminal, "pane_id_at_binding": "pane-1"},
                "evidence": {"provenance": "kitsune.session.snapshot", "observed_at": "2026-10-23T12:00:00Z",
                             "agent_kind_hint": "djinn", "native_reference": native,
                             "native_binding": "unestablished" if native is None else "observed_reference"}}

    def create(self, key="review", name="Review", **kwargs):
        return groups.create_group(self.workspace, key, name, **kwargs)

    def add(self, key="reviewer", terminal="terminal-1", **kwargs):
        return groups.add_member(self.workspace, "review", key, kwargs.pop("role", "Reviewer"),
                                 kwargs.pop("binding", self.binding(terminal)), **kwargs)

    def read(self):
        return groups.read_groups(self.workspace)

    def reserve(self, **kwargs):
        args = {"group_key": "review", "expected_revision": self.read()["revision"],
                "endpoint": self.endpoint, "source": "coven.groups", "epoch": "epoch-1",
                "terminal_ids": ["terminal-1"], "allow_new_epoch": False}
        return groups.reserve_cycle(self.workspace, **(args | kwargs))

    def write(self, state):
        self.path.write_text(json.dumps(state), encoding="utf-8")

    def test_missing_read_is_pure_and_only_create_initializes(self):
        with patch.object(storage, "workspace_lock", side_effect=AssertionError("lock")), patch(
                "os.fsync", side_effect=AssertionError("sync")):
            self.assertEqual(self.read(), {"initialized": False, "revision": groups.UNINITIALIZED, "state": None})
        self.assertEqual(list((self.workspace / "state").iterdir()), [])
        for operation in (self.add, lambda: groups.remove_member(self.workspace, "review", "nobody"), self.reserve):
            with self.assertRaises(groups.GroupError):
                operation()
            self.assertFalse(self.path.exists())
        result = self.create(expected_revision=groups.UNINITIALIZED)
        self.assertEqual(result["outcome"], "committed")
        self.assertEqual(result["authorization"], "none")
        self.assertIs(result["dispatchable"], False)
        self.assertEqual(self.read()["state"]["content_revision"], 1)
        self.assertEqual(result["group"]["group_id"], str(uuid.UUID(result["group"]["group_id"])))

    def test_create_retry_confirms_without_rewrite_and_name_conflicts(self):
        first = self.create()
        info = self.path.stat()
        with patch.object(storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
            retry = self.create()
        self.assertEqual(retry["outcome"], "unchanged")
        self.assertEqual(retry["group"], first["group"])
        self.assertEqual(retry["revision"], first["revision"])
        self.assertEqual(self.path.stat().st_mtime_ns, info.st_mtime_ns)
        with self.assertRaises(groups.GroupError):
            self.create(name="Other")
        with self.assertRaises(groups.GroupError):
            self.create(expected_revision=groups.UNINITIALIZED)

    def test_raw_tasks_roster_config_are_not_loaded_or_changed(self):
        files = [self.workspace / "coven.json", self.workspace / "state" / "tasks.json",
                 self.workspace / "state" / "agents.json", self.workspace / "state" / "events.jsonl"]
        for path in files[1:]:
            path.write_bytes(b"malformed\xff raw work\n")
        before = {path: (path.read_bytes(), path.stat().st_mtime_ns) for path in files}
        self.create()
        self.add()
        self.reserve()
        groups.remove_member(self.workspace, "review", "reviewer")
        self.assertEqual(before, {path: (path.read_bytes(), path.stat().st_mtime_ns) for path in files})

    def test_binding_retry_move_role_and_explicit_rebind(self):
        self.create()
        first = self.add()
        moved = self.binding()
        moved["presentation"]["pane_id_at_binding"] = "pane-2"
        moved["evidence"]["observed_at"] = "2026-10-23T13:00:00+00:00"
        raw = self.path.read_bytes()
        retry = self.add(binding=moved)
        self.assertEqual(retry["outcome"], "unchanged")
        self.assertEqual(self.path.read_bytes(), raw)
        role = self.add(binding=moved, role="Coordinator")
        self.assertNotEqual(role["revision"], first["revision"])
        self.assertEqual(role["member"]["binding"], moved)
        native = {"source": "snapshot", "agent": "djinn", "kind": "id", "value": "session-1"}
        resolved = self.binding(native=native)
        with self.assertRaises(groups.GroupError):
            self.add(binding=resolved)
        rebound = self.add(binding=resolved, rebind=True)
        self.assertEqual(rebound["member"]["participant_id"], first["member"]["participant_id"])
        self.assertEqual(rebound["member"]["binding"]["evidence"]["native_binding"], "observed_reference")
        for change in ("terminal", "endpoint", "kind", "native"):
            binding = deepcopy(resolved)
            if change == "terminal":
                binding["presentation"]["terminal_id"] = "terminal-2"
            elif change == "endpoint":
                binding["presentation"]["endpoint"]["inode"] += 1
            elif change == "kind":
                binding["evidence"]["agent_kind_hint"] = "other"
            else:
                binding["evidence"]["native_reference"]["value"] = "session-2"
            with self.subTest(change=change), self.assertRaises(groups.GroupError):
                self.add(binding=binding)

    def test_duplicate_terminal_rejected_native_duplicates_allowed_keys_scoped(self):
        self.create()
        self.create("other", "Other")
        native = {"source": "snapshot", "agent": "djinn", "kind": "path", "value": "/session"}
        first = self.add(binding=self.binding(native=native))
        with self.assertRaises(groups.GroupError):
            self.add("another")
        with self.assertRaises(groups.GroupError):
            groups.add_member(self.workspace, "other", "reviewer", "Role", self.binding())
        other = groups.add_member(self.workspace, "other", "reviewer", "Role", self.binding("terminal-2", native))
        self.assertNotEqual(first["member"]["participant_id"], other["member"]["participant_id"])

    def test_remove_is_idempotent_and_retains_counter_watermarks(self):
        self.create()
        self.add()
        self.reserve()
        removed = groups.remove_member(self.workspace, "review", "reviewer")
        self.assertEqual(removed["outcome"], "committed")
        raw = self.path.read_bytes()
        absent = groups.remove_member(self.workspace, "review", "reviewer")
        self.assertIsNone(absent["member"])
        self.assertEqual(absent["outcome"], "unchanged")
        self.assertEqual(self.path.read_bytes(), raw)
        self.add()
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 2})

    def test_fail_closed_json_and_bounded_nesting(self):
        invalid = [b"null", b"[]", b"{}", b"\xff", b"{", b'{"x":1,"x":2}',
                   b'{"x":NaN}', b'{"x":1e999}', b"[" * 50 + b"]" * 50,
                   b" " * (groups.MAX_RECORD_BYTES + 1)]
        for data in invalid:
            self.path.write_bytes(data)
            with self.subTest(data=data[:40]):
                for operation in (self.read, self.create, self.add):
                    with self.assertRaises(storage.StorageError):
                        operation()
                self.assertEqual(self.path.read_bytes(), data)

    def test_strict_schema_identities_and_types(self):
        self.create()
        self.add()
        original = self.read()["state"]
        mutations = [lambda s: s.update(schema_version=True), lambda s: s.update(extra=1),
                     lambda s: s.update(content_revision=2**64), lambda s: s.update(workspace_id="bad"),
                     lambda s: s["groups"].append(deepcopy(s["groups"][0])),
                     lambda s: s["groups"][0]["members"].append(deepcopy(s["groups"][0]["members"][0])),
                     lambda s: s["groups"][0]["members"][0]["binding"]["presentation"]["endpoint"].update(device=True),
                     lambda s: s["groups"][0]["members"][0]["binding"]["evidence"].update(native_binding="observed_reference"),
                     lambda s: s["groups"][0]["members"][0].update(extra=None)]
        for mutate in mutations:
            state = deepcopy(original)
            mutate(state)
            self.write(state)
            with self.assertRaises(groups.GroupError):
                self.read()

    def test_workspace_alias_allowed_managed_symlinks_rejected(self):
        alias = self.root / "alias"
        alias.symlink_to(self.workspace, target_is_directory=True)
        groups.create_group(alias, "review", "Review")
        self.assertEqual(groups.read_groups(alias), self.read())
        self.path.unlink()
        self.path.symlink_to(self.root / "missing")
        for operation in (self.read, self.create):
            with self.assertRaises(storage.StorageError):
                operation()
        self.path.unlink()
        lock = self.workspace / "state" / ".workspace.lock"
        lock.unlink()
        lock.symlink_to(self.root / "missing")
        with self.assertRaises(storage.StorageError):
            self.create()
        lock.unlink()
        (self.workspace / "state").rmdir()
        (self.workspace / "state").symlink_to(self.root)
        with self.assertRaises(storage.StorageError):
            self.create()
        self.assertFalse((self.root / ".workspace.lock").exists())

    def test_marker_must_be_regular_but_never_parsed(self):
        marker = self.workspace / "coven.json"
        marker.unlink()
        with self.assertRaises(groups.GroupError):
            self.read()
        marker.symlink_to(self.root / "marker")
        (self.root / "marker").write_text("{}")
        with self.assertRaises(storage.StorageError):
            self.create()
        with self.assertRaises(groups.GroupError):
            groups.read_groups(self.root / "absent")

    def test_input_boundaries_exact_display_and_utf8(self):
        for key in ("", "UPPER", "a b", "a/", "-a", "a" * 65, None):
            with self.subTest(key=key), self.assertRaises(groups.GroupError):
                self.create(key=key)
        for name in ("", " ", "a\nb", "a\tb", "x" * 65, "\u00e9" * 33, "\ud800"):
            with self.subTest(name=repr(name)), self.assertRaises(groups.GroupError):
                self.create(name=name)
        exact = "  " + "\u00e9" * 30 + "  "
        self.assertEqual(self.create(key="a" * 64, name=exact)["group"]["name"], exact)
        self.create()
        self.assertEqual(self.add(role="\u00e9" * 32)["member"]["role"], "\u00e9" * 32)
        for mutate in (lambda b: b["presentation"].update(terminal_id=""),
                       lambda b: b["presentation"].update(pane_id_at_binding=" pane"),
                       lambda b: b["presentation"]["endpoint"].update(path="relative"),
                       lambda b: b["evidence"].update(observed_at="2026-10-23T00:00:00+01:00"),
                       lambda b: b["evidence"].update(agent_kind_hint="x" * 4097)):
            binding = self.binding()
            mutate(binding)
            with self.assertRaises(groups.GroupError):
                self.add(binding=binding)

    def test_group_member_and_total_caps(self):
        for index in range(16):
            self.create(f"g{index}", "Group")
        with self.assertRaises(groups.GroupError):
            self.create("overflow")
        for group in range(4):
            for member in range(16):
                groups.add_member(self.workspace, f"g{group}", f"m{member}", "Role",
                                  self.binding(f"t{group}-{member}"))
        with self.assertRaises(groups.GroupError):
            groups.add_member(self.workspace, "g0", "overflow", "Role", self.binding("overflow"))
        with self.assertRaises(groups.GroupError):
            groups.add_member(self.workspace, "g4", "overflow", "Role", self.binding("overflow"))
        groups.remove_member(self.workspace, "g0", "m0")
        groups.add_member(self.workspace, "g4", "available", "Role", self.binding("available"))

    def test_counter_cycles_preserve_content_token_and_full_target_set(self):
        self.create()
        self.add()
        self.add("second", "terminal-2")
        revision = self.read()["revision"]
        for terminals in ([], ["terminal-1"], ["terminal-1", "terminal-1"], ["terminal-1", "other"]):
            with self.assertRaises(groups.GroupError):
                self.reserve(terminal_ids=terminals)
        first = self.reserve(terminal_ids=["terminal-2", "terminal-1"])
        self.assertEqual(first["reservations"], {"terminal-1": 1, "terminal-2": 1})
        second = self.reserve(terminal_ids=["terminal-1", "terminal-2"])
        self.assertEqual(second["reservations"], {"terminal-1": 2, "terminal-2": 2})
        self.assertEqual(first["revision"], revision)
        self.assertEqual(second["revision"], revision)
        self.assertEqual(self.read()["revision"], revision)
        with self.assertRaises(groups.GroupError):
            self.reserve(endpoint=self.endpoint | {"inode": 5}, terminal_ids=["terminal-1", "terminal-2"])
        self.add("second", binding=self.binding("terminal-2") | {
            "presentation": self.binding("terminal-2")["presentation"] | {"endpoint": self.endpoint | {"inode": 5}}}, rebind=True)
        with self.assertRaises(groups.GroupError):
            self.reserve(terminal_ids=["terminal-1", "terminal-2"])

    def test_epochs_explicit_reset_whole_map_and_no_entry_eviction(self):
        self.create()
        self.add()
        self.reserve()
        groups.remove_member(self.workspace, "review", "reviewer")
        self.add(terminal="terminal-2")
        self.reserve(terminal_ids=["terminal-2"])
        with self.assertRaises(groups.GroupError):
            self.reserve(epoch="new", terminal_ids=["terminal-2"])
        self.assertEqual(self.reserve(epoch="new", terminal_ids=["terminal-2"], allow_new_epoch=True)["reservations"],
                         {"terminal-2": 1})
        self.assertEqual(self.read()["state"]["publication_counters"][0]["watermarks"], {"terminal-2": 1})
        for index in range(7):
            self.reserve(source=f"source-{index}", terminal_ids=["terminal-2"])
        raw = self.path.read_bytes()
        with self.assertRaises(groups.GroupError):
            self.reserve(source="overflow", terminal_ids=["terminal-2"])
        self.assertEqual(self.path.read_bytes(), raw)
        self.assertEqual(self.reserve(epoch="new", terminal_ids=["terminal-2"])["reservations"], {"terminal-2": 2})

    def test_watermark_cap_and_overflow_do_not_publish_partial_cycles(self):
        self.create()
        self.add()
        self.reserve()
        state = self.read()["state"]
        watermarks = {f"old-{i}": 1 for i in range(63)} | {"terminal-1": 1}
        state["publication_counters"][0]["watermarks"] = watermarks
        self.write(state)
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 2})
        self.add("second", "terminal-2")
        raw = self.path.read_bytes()
        with self.assertRaises(groups.GroupError):
            self.reserve(terminal_ids=["terminal-1", "terminal-2"])
        self.assertEqual(self.path.read_bytes(), raw)
        self.reserve(epoch="new", allow_new_epoch=True, terminal_ids=["terminal-1", "terminal-2"])
        state = self.read()["state"]
        state["publication_counters"][0]["watermarks"]["terminal-2"] = 2**64 - 1
        self.write(state)
        raw = self.path.read_bytes()
        with self.assertRaises(groups.GroupError):
            self.reserve(epoch="new", terminal_ids=["terminal-1", "terminal-2"])
        self.assertEqual(self.path.read_bytes(), raw)

    def test_content_overflow_and_size_limit(self):
        self.create()
        state = self.read()["state"]
        state["content_revision"] = 2**64 - 1
        self.write(state)
        self.assertEqual(self.create()["outcome"], "unchanged")
        with self.assertRaises(groups.GroupError):
            self.add()
        self.assertEqual(self.read()["state"], state)
        with patch.object(groups, "MAX_RECORD_BYTES", 200), self.assertRaises(groups.GroupError):
            self.read()

    def test_cas_checks_fresh_content_but_not_counter_updates(self):
        first = self.create()
        added = self.add(expected_revision=first["revision"])
        for operation in (lambda: self.add(expected_revision=first["revision"]),
                          lambda: self.reserve(expected_revision=first["revision"]),
                          lambda: self.create(expected_revision="malformed")):
            with self.assertRaises(groups.GroupError):
                operation()
        self.reserve(expected_revision=added["revision"])
        self.assertEqual(self.add(expected_revision=added["revision"])["outcome"], "unchanged")
        self.assertEqual(self.reserve(expected_revision=added["revision"])["reservations"], {"terminal-1": 2})
        with self.assertRaises(groups.GroupError):
            self.reserve(expected_revision=None)

    def test_concurrent_writes_no_lost_updates_and_conditional_single_winner(self):
        self.create()
        with ThreadPoolExecutor(max_workers=8) as pool:
            list(pool.map(lambda i: self.add(f"m{i}", f"t{i}"), range(8)))
        self.assertEqual(len(self.read()["state"]["groups"][0]["members"]), 8)
        expected = self.read()["revision"]
        def conditional(index):
            try:
                return self.add(f"c{index}", f"ct{index}", expected_revision=expected)
            except groups.GroupError:
                return None
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(conditional, range(4)))
        self.assertEqual(sum(result is not None for result in results), 1)

    def test_concurrent_reservations_are_unique(self):
        self.create()
        self.add()
        with ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(lambda _: self.reserve(), range(12)))
        self.assertEqual(sorted(r["reservations"]["terminal-1"] for r in results), list(range(1, 13)))

    def test_restart_and_crashed_publisher_burn_without_replay(self):
        self.create()
        self.add()
        script = ("from runner import groups; import sys,os; "
                  "w=sys.argv[1]; s=groups.read_groups(w); "
                  "e=s['state']['groups'][0]['members'][0]['binding']['presentation']['endpoint']; "
                  "r=groups.reserve_cycle(w,'review',s['revision'],e,'coven.groups','epoch-1',"
                  "['terminal-1'],allow_new_epoch=False); "
                  "assert r['reservations']=={'terminal-1':1}; os._exit(23)")
        result = subprocess.run([sys.executable, "-B", "-c", script, str(self.workspace)],
                                cwd=Path(__file__).resolve().parents[1], timeout=10,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(result.returncode, 23, result.stderr.decode())
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 2})

    def test_prepublication_failure_raises_and_uncertainty_keeps_known_results(self):
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("prepublish")):
            with self.assertRaises(storage.StorageError):
                self.create()
        self.assertFalse(self.path.exists())
        original = storage.atomic_replace_bytes
        def uncertain(path, data):
            original(path, data)
            raise storage.CommitUncertain(path)
        with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            result = self.create()
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(result["revision"], self.read()["revision"])
        self.add()
        revision = self.read()["revision"]
        with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            reservation = self.reserve()
        self.assertEqual(reservation["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(reservation["reservations"], {"terminal-1": 1})
        self.assertEqual(reservation["revision"], revision)
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 2})

    def test_postpublication_verification_failure_and_noop_sync_uncertain(self):
        original = storage.atomic_replace_bytes
        def vanish(path, data):
            original(path, data)
            path.unlink()
        with patch.object(storage, "atomic_replace_bytes", side_effect=vanish):
            result = self.create()
        self.assertEqual(result["outcome"], "committed")
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.assertIsNotNone(result["group"])
        self.create()
        self.add()
        with patch.object(storage, "sync_directory", side_effect=OSError("sync")), patch.object(
                storage, "atomic_replace_bytes", side_effect=AssertionError("rewrite")):
            result = self.add()
        self.assertEqual(result["outcome"], "unchanged")
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        with patch.object(storage, "atomic_replace_bytes", side_effect=vanish):
            reservation = self.reserve()
        self.assertEqual(reservation["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(reservation["reservations"], {"terminal-1": 1})

    def test_canonical_hash_ignores_json_format_and_counters_only(self):
        self.create()
        self.add()
        first = self.read()
        self.write(first["state"])
        self.assertEqual(self.read()["revision"], first["revision"])
        self.reserve()
        self.assertEqual(self.read()["revision"], first["revision"])
        state = self.read()["state"]
        state["groups"][0]["name"] = "Changed outside API"
        self.write(state)
        self.assertNotEqual(self.read()["revision"], first["revision"])

    def test_native_reference_bounds_and_absence_do_not_require_identity(self):
        self.create()
        absent = self.binding()
        absent["evidence"]["agent_kind_hint"] = None
        first = self.add(binding=absent)
        self.assertNotEqual(first["member"]["participant_id"], "pane-1")
        native = {"source": "s" * 4096, "agent": "a" * 4096, "kind": "path", "value": "/" + "p" * 8191}
        self.add(binding=self.binding(native=native), rebind=True)
        for field in ("source", "agent", "value"):
            invalid = deepcopy(native)
            invalid[field] += "x"
            with self.subTest(field=field), self.assertRaises(groups.GroupError):
                self.add(binding=self.binding(native=invalid), rebind=True)
        invalid = native | {"kind": "id"}
        with self.assertRaises(groups.GroupError):
            self.add(binding=self.binding(native=invalid), rebind=True)
        self.add(binding=absent, rebind=True)
        self.assertEqual(self.read()["state"]["groups"][0]["members"][0]["participant_id"],
                         first["member"]["participant_id"])

    def test_malformed_counters_and_publication_inputs_fail_closed(self):
        self.create()
        self.add()
        self.reserve()
        original = self.read()["state"]
        for updates in ({"source": "bad source"}, {"epoch": ""}, {"watermarks": {"t": True}},
                        {"watermarks": {"t": 0}}, {"watermarks": {"t": 2**64}},
                        {"watermarks": []}, {"extra": 1}):
            state = deepcopy(original)
            state["publication_counters"][0].update(updates)
            self.write(state)
            with self.subTest(updates=updates), self.assertRaises(groups.GroupError):
                self.read()
        state = deepcopy(original)
        state["publication_counters"].append(deepcopy(state["publication_counters"][0]))
        self.write(state)
        with self.assertRaises(groups.GroupError):
            self.read()
        self.write(original)
        for updates in ({"source": ""}, {"source": "x" * 65}, {"source": "\u00e9"},
                        {"epoch": "e" * 4097}, {"allow_new_epoch": 1},
                        {"terminal_ids": [""]}, {"terminal_ids": ["t"] * 17}):
            with self.subTest(updates=updates), self.assertRaises(groups.GroupError):
                self.reserve(**updates)

    def test_actual_replace_and_fsync_failure_boundaries(self):
        self.create()
        self.add()
        before = self.path.read_bytes()
        with patch.object(storage.os, "replace", side_effect=OSError("replace failed")):
            with self.assertRaises(storage.StorageError):
                self.reserve()
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(list(self.path.parent.glob(".groups.json.*.tmp")), [])
        with patch.object(storage.os, "fsync", side_effect=OSError("file sync failed")):
            with self.assertRaises(storage.StorageError):
                self.reserve()
        self.assertEqual(self.path.read_bytes(), before)
        with patch.object(storage, "sync_directory", side_effect=OSError("directory sync failed")):
            result = self.reserve()
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(result["reservations"], {"terminal-1": 1})
        self.assertEqual(self.reserve()["reservations"], {"terminal-1": 2})

    def test_multitarget_cycle_overflow_does_not_burn_any_target(self):
        self.create()
        self.add()
        self.add("second", "terminal-2")
        self.reserve(terminal_ids=["terminal-1", "terminal-2"])
        state = self.read()["state"]
        state["publication_counters"][0]["watermarks"]["terminal-2"] = 2**64 - 1
        self.write(state)
        with self.assertRaises(groups.GroupError):
            self.reserve(terminal_ids=["terminal-1", "terminal-2"])
        self.assertEqual(self.read()["state"], state)

    def test_persistent_lock_inode_and_inert_orphans(self):
        self.create()
        lock = self.path.parent / ".workspace.lock"
        inode = lock.stat().st_ino
        orphan = self.path.parent / ".groups.json.crashed.tmp"
        orphan.write_bytes(b"not authority")
        self.add()
        self.reserve()
        self.assertEqual(lock.stat().st_ino, inode)
        self.assertEqual(orphan.read_bytes(), b"not authority")
        self.path.unlink()
        self.assertFalse(self.read()["initialized"])
        self.assertTrue(orphan.exists())

    def test_standalone_import_and_storage_error_contract(self):
        result = subprocess.run([sys.executable, "-I", "-S", "-B", "-c",
            "import sys; sys.path.insert(0,sys.argv[1]); import groups; "
            "assert issubclass(groups.GroupError,groups.storage.StorageError)",
            str(Path(groups.__file__).parent)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode())


if __name__ == "__main__":
    unittest.main()
