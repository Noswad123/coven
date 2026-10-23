"""Disposable Git fixtures only; repository observation must never mutate Git."""

from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from collections import deque
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock
from unittest.mock import patch
import zlib

from runner import repositories as repos, storage


class RepositoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coven-repository-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        (self.workspace / "state").mkdir()
        (self.workspace / "coven.json").write_text("{}")
        self.repo = self.root / "checkout"
        self.repo.mkdir()
        self.git("init", "--quiet")
        (self.repo / "file").write_text("original\n")
        self.git("add", "file")
        self.git("commit", "--quiet", "-m", "fixture")
        self.base = self.git("rev-parse", "HEAD").decode().strip()
        self.tree = self.git("rev-parse", "HEAD^{tree}").decode().strip()
        self.authority = self.workspace / "state" / "repository.json"

    def git(self, *args, cwd=None, input=None):
        env = repos._environment() | {"GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                                      "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        return subprocess.run([shutil.which("git"), "-c", "commit.gpgsign=false", *args],
                              cwd=cwd or self.repo, env=env, input=input, check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10).stdout

    def register(self, **kwargs):
        return repos.register_repository(self.workspace, **({"path": str(self.repo), "base": self.base,
            "delivery_target": "refs/heads/delivery", "expected_revision": repos.UNREGISTERED} | kwargs))

    def snapshot(self, metadata=False):
        result = {}
        for p in self.repo.rglob("*"):
            if p.is_file():
                info = p.stat()
                data = p.read_bytes()
                result[str(p.relative_to(self.repo))] = ((data, info.st_mtime_ns, info.st_ctime_ns,
                                                        info.st_mode, info.st_ino) if metadata else data)
        return result

    def advance_head(self):
        (self.repo / "file").write_text("new HEAD contents\n")
        self.git("add", "file")
        self.git("commit", "--quiet", "-m", "advance fixture HEAD")
        return self.git("rev-parse", "HEAD").decode().strip()

    def test_basic_frozen_record_and_pure_inspection(self):
        self.assertIsNone(repos.read_registration(self.workspace))
        self.assertEqual(repos.inspect_repository(self.workspace)["revision"], repos.UNREGISTERED)
        self.assertFalse((self.workspace / "state" / ".workspace.lock").exists())
        before = self.snapshot(metadata=True)
        result = self.register()
        self.assertEqual(result["delivery"]["outcome"], "durably_committed")
        self.assertFalse(result["replayed"])
        self.assertEqual(result["authorization"], "none")
        self.assertIs(result["dispatchable"], False)
        record = result["registration"]
        self.assertEqual(record["base_commit"], self.base)
        self.assertEqual(record["base_tree"], self.tree)
        self.assertEqual(record["canonical_root"], str(self.repo))
        self.assertEqual(before, self.snapshot(metadata=True))
        (self.workspace / "state" / "tasks.json").write_text("broken")
        (self.workspace / "state" / "agents.json").symlink_to("missing")
        with patch.object(repos, "_run_git", side_effect=AssertionError("Git")), patch.object(
                storage, "workspace_lock", side_effect=AssertionError("lock")), patch("os.fsync", side_effect=AssertionError("sync")):
            self.assertEqual(repos.read_registration(self.workspace), record)
            self.assertEqual(repos.inspect_repository(self.workspace)["registration"], record)

    def test_linked_worktree(self):
        linked = self.root / "linked"
        self.git("worktree", "add", "--quiet", "--detach", str(linked), self.base)
        result = self.register(path=str(linked))
        record = result["registration"]
        self.assertEqual(record["canonical_root"], str(linked))
        self.assertNotEqual(record["git_dir"], record["common_dir"])
        self.assertTrue(repos.inspect_repository(self.workspace, verify=True)["observations"]["available"])

    def test_dirty_index_untracked_and_refs_unchanged(self):
        (self.repo / "file").write_text("staged\n")
        self.git("add", "file")
        (self.repo / "file").write_text("unstaged\n")
        (self.repo / "odd\nname").write_bytes(b"untracked")
        untracked = 1
        try:
            os.close(os.open(os.fsencode(self.repo) + b"/nonutf8-\xff", os.O_CREAT | os.O_WRONLY, 0o600))
            untracked += 1
        except OSError:
            pass  # macOS filesystems reject non-UTF8 names; byte parsing is tested below.
        before = self.snapshot(metadata=True)
        fresh = self.register()
        self.assertTrue(fresh["observations"]["dirty"])
        self.assertEqual(fresh["observations"]["tracked_changes"], 1)
        self.assertEqual(fresh["observations"]["untracked"], untracked)
        self.assertNotIn("observations", fresh["registration"])
        observed = repos.inspect_repository(self.workspace, verify=True)["observations"]
        self.assertEqual(observed["tracked_changes"], 1)
        self.assertEqual(observed["untracked"], untracked)
        self.assertTrue(observed["excluded_from_base"])
        self.assertTrue(observed["dirty"])
        self.assertEqual(before, self.snapshot(metadata=True))

    def test_strict_oid_types_and_refs(self):
        blob = self.git("rev-parse", "HEAD:file").decode().strip()
        self.git("tag", "-a", "tag", "-m", "fixture")
        tag = self.git("rev-parse", "tag").decode().strip()
        for base in ("HEAD", self.base[:12], self.base + "^0", self.base.upper(), self.tree, blob, tag, "f" * 40):
            with self.subTest(base=base), self.assertRaises(repos.RepositoryError):
                self.register(base=base)
        for target in ("main", "refs/tags/x", "refs/heads/", "refs/heads/a..b", "refs/heads/a.lock", "refs/heads/a b", "refs/heads/.a", "refs/heads/a@{b", "refs/heads/a\\b"):
            with self.subTest(target=target), self.assertRaises(repos.RepositoryError):
                self.register(delivery_target=target)
        self.assertFalse(self.authority.exists())

    def test_original_retry_after_deletion_and_changed_inputs(self):
        first = self.register()
        shutil.rmtree(self.repo)
        with patch.object(repos, "_observe", side_effect=AssertionError("reopened")):
            retry = self.register(path=str(self.repo / "."))
        self.assertTrue(retry["replayed"])
        self.assertEqual(retry["registration"], first["registration"])
        self.assertEqual(retry["revision"], first["revision"])
        self.assertEqual(retry["delivery"]["outcome"], "durably_committed")
        for changes in ({"path": str(self.root / "other")}, {"base": "f" * 40},
                        {"delivery_target": "refs/heads/other"}, {"expected_revision": first["revision"]}):
            with self.subTest(changes=changes), self.assertRaises(repos.RepositoryError):
                self.register(**changes)
        unavailable = repos.inspect_repository(self.workspace, verify=True)
        self.assertEqual(unavailable["status"], "unavailable")
        self.assertEqual(unavailable["registration"], first["registration"])

    def test_stale_cas_never_probes(self):
        with patch.object(repos, "_observe", side_effect=AssertionError("probe")):
            with self.assertRaises(repos.RepositoryError):
                self.register(expected_revision="repository-sha256:" + "0" * 64)

    def test_malformed_authority_is_not_absence(self):
        for data in (b"null", b"{}", b"[]", b"{", b"\xff", b'{"x":1,"x":2}', b" " * (repos.MAX_RECORD_BYTES + 1)):
            self.authority.write_bytes(data)
            with self.subTest(data=data[:30]), self.assertRaises(repos.RepositoryError):
                repos.read_registration(self.workspace)
            with self.assertRaises(repos.RepositoryError):
                self.register()
            unavailable = repos.inspect_repository(self.workspace)
            self.assertEqual(unavailable["status"], "unavailable")
            self.assertIsNone(unavailable["revision"])
            self.assertEqual(unavailable["diagnostics"], [{"code": "repository_authority_unavailable"}])
            self.assertEqual(unavailable["authorization"], "none")
            self.assertFalse(unavailable["dispatchable"])
            self.assertNotIn("registration", unavailable)
        self.authority.unlink()
        self.authority.symlink_to(self.root / "missing")
        self.assertEqual(repos.inspect_repository(self.workspace)["status"], "unavailable")

    def test_strict_record_and_exact_byte_revision(self):
        first = self.register()
        record = first["registration"]
        self.authority.write_text(json.dumps(record, separators=(",", ":")))
        self.assertNotEqual(repos.inspect_repository(self.workspace)["revision"], first["revision"])
        for updates in ({"schema_version": True}, {"kind": "other"}, {"base_tree": "x" * 40},
                        {"registration_id": "bad"}, {"registered_at": "yesterday"}, {"extra": 1},
                        {"canonical_root": "relative"}, {"original_request": None}, {"filesystem": {}}):
            self.authority.write_text(json.dumps(record | updates))
            with self.subTest(updates=updates), self.assertRaises(repos.RepositoryError):
                repos.read_registration(self.workspace)

    def test_existing_workspace_required_without_loading_mutable_data(self):
        for workspace in (self.root / "missing", self.repo):
            with self.assertRaises(repos.RepositoryError):
                repos.register_repository(workspace, str(self.repo), self.base, "refs/heads/x", repos.UNREGISTERED)
        (self.workspace / "state").rmdir()
        (self.workspace / "state").symlink_to(self.repo)
        with self.assertRaises(repos.RepositoryError):
            self.register()
        self.assertFalse((self.repo / ".workspace.lock").exists())

    def test_environment_fsmonitor_replacement_and_config_preservation(self):
        marker = self.root / "executed"
        helper = self.root / "hostile-fsmonitor"
        helper.write_text("#!/bin/sh\ntouch '" + str(marker) + "'\n")
        helper.chmod(0o700)
        self.git("config", "core.fsmonitor", str(helper))
        replacement = self.git("hash-object", "-w", "--stdin", input=b"bad").decode().strip()
        self.git("update-ref", "refs/replace/" + self.base, replacement)
        before = self.snapshot(metadata=True)
        hostile = {"GIT_DIR": str(self.root / "missing"), "GIT_WORK_TREE": "/", "GIT_CONFIG_COUNT": "1",
                   "GIT_CONFIG_KEY_0": "core.fsmonitor", "GIT_CONFIG_VALUE_0": str(helper),
                   "GIT_OBJECT_DIRECTORY": "/missing", "GIT_CONFIG_GLOBAL": "/missing", "GIT_EXEC_PATH": "/missing"}
        with patch.dict(os.environ, hostile):
            self.register()
        self.assertFalse(marker.exists())
        self.assertEqual(before, self.snapshot(metadata=True))

    def test_parent_traversal_across_symlink_rejected_before_probe(self):
        # Both choices have the same base, so an OID check cannot disambiguate.
        parent = self.root / "other"
        parent.mkdir()
        other = parent / "checkout"
        shutil.copytree(self.repo, other)
        nested = parent / "nested"
        nested.mkdir()
        alias = self.root / "alias"
        alias.symlink_to(nested, target_is_directory=True)
        selector = str(alias) + "/../checkout"
        self.assertEqual(Path(selector).resolve(), other)
        self.assertEqual(os.path.abspath(selector), str(self.repo))
        with patch.object(repos, "_observe", side_effect=AssertionError("probe")):
            with self.assertRaisesRegex(repos.RepositoryError, "parent traversal"):
                self.register(path=selector)
        self.assertFalse(self.authority.exists())

    def test_split_index_and_linked_worktree_rejected_without_metadata_writes(self):
        linked = self.root / "linked"
        self.git("worktree", "add", "--quiet", "--detach", str(linked), self.base)
        for checkout in (self.repo, linked):
            with self.subTest(checkout=checkout):
                self.git("update-index", "--split-index", cwd=checkout)
                git_dir = Path(self.git("rev-parse", "--absolute-git-dir", cwd=checkout).decode().strip())
                shared = list(git_dir.glob("sharedindex.*"))
                self.assertTrue(shared)
                for path in shared:
                    os.utime(path, (1000000000, 1000000000))
                before = self.snapshot(metadata=True)
                calls = []
                original = repos._run_git
                def run(executable, arguments, cwd, env, timeout):
                    calls.append(arguments)
                    return original(executable, arguments, cwd, env, timeout)
                with patch.object(repos, "_run_git", side_effect=run):
                    with self.assertRaisesRegex(repos.RepositoryError, "sharedindex"):
                        self.register(path=str(checkout))
                self.assertFalse(any(args[0] == "status" for args in calls))
                self.assertEqual(before, self.snapshot(metadata=True))
                # Leave no common-dir artifacts to mask the linked-dir check.
                self.git("update-index", "--no-split-index", cwd=checkout)
                for path in shared:
                    path.unlink(missing_ok=True)

    def test_split_index_config_and_stale_artifacts_fail_before_status(self):
        original = repos._run_git
        def no_status(executable, arguments, cwd, env, timeout):
            self.assertNotEqual(arguments[0], "status")
            return original(executable, arguments, cwd, env, timeout)
        self.git("config", "core.splitindex", "true")
        before = self.snapshot(metadata=True)
        with patch.object(repos, "_run_git", side_effect=no_status):
            with self.assertRaisesRegex(repos.RepositoryError, "split-index configuration"):
                self.register()
        self.assertEqual(before, self.snapshot(metadata=True))
        self.git("config", "core.splitindex", "false")
        artifact = self.repo / ".git" / "sharedindex.stale"
        artifact.write_bytes(b"stale")
        os.utime(artifact, (1000000000, 1000000000))
        before = self.snapshot(metadata=True)
        with patch.object(repos, "_run_git", side_effect=no_status):
            with self.assertRaisesRegex(repos.RepositoryError, "sharedindex"):
                self.register()
        self.assertEqual(before, self.snapshot(metadata=True))
        artifact.unlink()
        self.assertEqual(self.register()["status"], "registered")

    def test_case_variant_split_indexes_rejected_without_metadata_writes(self):
        probe = self.root / "case-probe"
        probe.write_bytes(b"filesystem case detection")
        alias = self.root / "CASE-PROBE"
        case_insensitive = alias.exists() and alias.samefile(probe)
        probe.unlink()
        linked = self.root / "linked"
        self.git("worktree", "add", "--quiet", "--detach", str(linked), self.base)
        original = repos._run_git
        def no_status(executable, arguments, cwd, env, timeout):
            self.assertNotEqual(arguments[0], "status", "split-index rejection must precede status")
            return original(executable, arguments, cwd, env, timeout)
        for checkout in (self.repo, linked):
            verified_workspace = self.root / (checkout.name + "-workspace")
            verified_workspace.mkdir()
            (verified_workspace / "state").mkdir()
            (verified_workspace / "coven.json").write_text("{}")
            first = repos.register_repository(verified_workspace, str(checkout), self.base,
                                              "refs/heads/delivery", repos.UNREGISTERED)
            self.git("update-index", "--split-index", cwd=checkout)
            git_dir = Path(self.git("rev-parse", "--absolute-git-dir", cwd=checkout).decode().strip())
            shared = list(git_dir.glob("sharedindex.*"))
            self.assertTrue(shared)
            for variant in (str.upper, lambda name: "ShArEdInDeX." + name.split(".", 1)[1].upper()):
                renamed = []
                for path in shared:
                    target = path.with_name(variant(path.name))
                    path.rename(target)
                    os.utime(target, (1000000000, 1000000000))
                    renamed.append(target)
                    if case_insensitive:
                        self.assertTrue(path.exists())
                        self.assertTrue(path.samefile(target))
                    else:
                        self.assertFalse(path.exists())
                with self.subTest(checkout=checkout, artifact=renamed[0].name):
                    before = self.snapshot(metadata=True)
                    with patch.object(repos, "_run_git", side_effect=no_status):
                        with self.assertRaisesRegex(repos.RepositoryError, "sharedindex"):
                            self.register(path=str(checkout))
                        result = repos.inspect_repository(verified_workspace, verify=True)
                    self.assertEqual(result["status"], "unavailable")
                    self.assertEqual(result["registration"], first["registration"])
                    self.assertEqual(result["revision"], first["revision"])
                    self.assertEqual(before, self.snapshot(metadata=True))
                for path, target in zip(shared, renamed):
                    target.rename(path)
            # Remove common-dir artifacts before testing the linked git-dir.
            self.git("update-index", "--no-split-index", cwd=checkout)
            for path in shared:
                path.unlink(missing_ok=True)
        self.assertFalse(self.authority.exists())

    def test_case_variant_promisor_suffix_rejected_before_status(self):
        artifact = self.repo / ".git" / "objects" / "pack" / "fixture.PrOmIsOr"
        artifact.write_bytes(b"")
        before = self.snapshot(metadata=True)
        original = repos._run_git
        def no_status(executable, arguments, cwd, env, timeout):
            self.assertNotEqual(arguments[0], "status")
            return original(executable, arguments, cwd, env, timeout)
        with patch.object(repos, "_run_git", side_effect=no_status):
            with self.assertRaisesRegex(repos.RepositoryError, "pack storage"):
                self.register()
        self.assertEqual(before, self.snapshot(metadata=True))
        self.assertFalse(self.authority.exists())

    def test_verified_inspection_rejects_new_split_index_without_metadata_writes(self):
        first = self.register()
        self.git("update-index", "--split-index")
        for path in (self.repo / ".git").glob("sharedindex.*"):
            os.utime(path, (1000000000, 1000000000))
        before = self.snapshot(metadata=True)
        result = repos.inspect_repository(self.workspace, verify=True)
        self.assertEqual(result["status"], "unavailable")
        self.assertEqual(result["registration"], first["registration"])
        self.assertEqual(before, self.snapshot(metadata=True))

    def test_corrupt_historical_root_tree_rejected(self):
        self.advance_head()
        object_path = self.repo / ".git" / "objects" / self.tree[:2] / self.tree[2:]
        object_path.chmod(0o600)
        object_path.write_bytes(zlib.compress(b"tree 0\0"))
        self.assertEqual(self.git("cat-file", "-t", self.tree), b"tree\n")
        before = self.snapshot(metadata=True)
        with self.assertRaisesRegex(repos.RepositoryError, "root tree content"):
            self.register()
        self.assertFalse(self.authority.exists())
        self.assertEqual(before, self.snapshot(metadata=True))

    def test_missing_historical_blob_reports_limited_availability(self):
        blob = self.git("rev-parse", "HEAD:file").decode().strip()
        current = self.advance_head()
        (self.repo / ".git" / "objects" / blob[:2] / blob[2:]).unlink()
        result = self.register()
        for observed in (result["observations"], repos.inspect_repository(self.workspace, verify=True)["observations"]):
            self.assertTrue(observed["available"])
            self.assertEqual(observed["base_object_validation"], "commit_and_root_tree_only")
            self.assertEqual(observed["complete_checkout_availability"], "unestablished")
            self.assertEqual(observed["head"], {"status": "commit", "commit": current})
        self.assertEqual(result["registration"]["base_commit"], self.base)

    def test_root_tree_structure_is_bounded_and_strict(self):
        oid = bytes.fromhex(self.base)
        for raw in (b"broken", b"100644 missing-oid\0", b"100644 ../file\0" + oid,
                    b"100644 file\0" + bytes(20), b"999999 file\0" + oid,
                    b"100644 file\0" + oid + b"100644 file\0" + oid):
            with self.subTest(raw=raw), self.assertRaises(repos.RepositoryError):
                repos._validate_tree(raw, 20)

    def test_known_retry_survives_confirmation_and_lock_failures(self):
        first = self.register()
        original_read = repos._read
        original_workspace = repos._workspace
        for point in ("reread", "workspace", "lock"):
            with self.subTest(point=point):
                reads = 0
                checks = 0
                def read(root):
                    nonlocal reads
                    reads += 1
                    if reads > 1 and point == "reread":
                        raise repos.RepositoryError("under-lock authority failure")
                    return original_read(root)
                def workspace(root):
                    nonlocal checks
                    checks += 1
                    if checks > 1 and point == "workspace":
                        raise repos.RepositoryError("under-lock structural failure")
                    return original_workspace(root)
                lock = storage.workspace_lock
                with patch.object(repos, "_read", side_effect=read), patch.object(repos, "_workspace", side_effect=workspace), patch.object(
                        storage, "workspace_lock", side_effect=storage.LockBusy("busy") if point == "lock" else lock), patch.object(
                        storage, "atomic_replace_bytes", side_effect=AssertionError("overwrite")), patch.object(
                        repos, "_observe", side_effect=AssertionError("Git")):
                    result = self.register()
                self.assertEqual(result["registration"], first["registration"])
                self.assertEqual(result["revision"], first["revision"])
                self.assertTrue(result["replayed"])
                self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.authority.write_bytes(b"null")
        with self.assertRaises(repos.RepositoryError):
            self.register()

    def test_head_advancement_is_not_registration_freshness(self):
        original = repos._observe
        calls = 0
        current = None
        def observe(request):
            nonlocal calls, current
            result = original(request)
            calls += 1
            if calls == 1:
                current = self.advance_head()
            return result
        with patch.object(repos, "_observe", side_effect=observe):
            result = self.register()
        self.assertEqual(result["registration"]["base_commit"], self.base)
        self.assertEqual(result["observations"]["head"], {"status": "commit", "commit": current})
        self.assertEqual(self.register()["observations"], {"status": "not_assessed"})

    def test_unborn_head_observation(self):
        self.git("symbolic-ref", "HEAD", "refs/heads/unborn-fixture")
        result = self.register()
        self.assertEqual(result["observations"]["head"], {"status": "unborn", "ref": "refs/heads/unborn-fixture"})

    def test_locator_replacement_within_observation_rejected(self):
        original = repos._run_git
        def replace(executable, arguments, cwd, env, timeout):
            result = original(executable, arguments, cwd, env, timeout)
            if arguments[0] == "status":
                moved = self.root / "old-checkout"
                self.repo.rename(moved)
                shutil.copytree(moved, self.repo)
            return result
        with patch.object(repos, "_run_git", side_effect=replace):
            with self.assertRaisesRegex(repos.RepositoryError, "locators changed"):
                self.register()
        self.assertFalse(self.authority.exists())

    def test_standalone_import(self):
        result = subprocess.run([sys.executable, "-I", "-S", "-B", "-c",
            "import sys; sys.path.insert(0, sys.argv[1]); import repositories; "
            "assert issubclass(repositories.RepositoryError, repositories.storage.StorageError)",
            str(Path(repos.__file__).parent)], env=repos._environment() | {"PYTHONDONTWRITEBYTECODE": "1"},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_partial_alternate_filter_and_include_rejected(self):
        for key, value in (("remote.origin.promisor", "true"), ("extensions.partialClone", "origin"),
                           ("filter.evil.clean", "touch /never"), ("include.path", "/missing"),
                           ("extensions.worktreeConfig", "true")):
            self.git("config", key, value)
            with self.subTest(key=key), self.assertRaises(repos.RepositoryError):
                self.register()
            self.git("config", "--unset", key)
        alternates = self.repo / ".git" / "objects" / "info" / "alternates"
        alternates.write_text("/missing\n")
        with self.assertRaises(repos.RepositoryError):
            self.register()
        alternates.unlink()
        (self.repo / ".git" / "objects" / "pack" / "x.promisor").touch()
        with self.assertRaises(repos.RepositoryError):
            self.register()
        self.assertFalse(self.authority.exists())

    def test_no_lazy_fetch_environment(self):
        observed = []
        original = repos._run_git
        def run(executable, arguments, cwd, env, timeout):
            observed.append(env)
            return original(executable, arguments, cwd, env, timeout)
        with patch.object(repos, "_run_git", side_effect=run):
            self.register()
        self.assertTrue(observed)
        self.assertTrue(all(e["GIT_NO_LAZY_FETCH"] == "1" and e["GIT_ALLOW_PROTOCOL"] == "" and
                            e["GIT_OPTIONAL_LOCKS"] == "0" for e in observed))

    def test_precommit_and_postcommit_failures(self):
        with patch.object(storage, "atomic_replace_bytes", side_effect=storage.StorageError("precommit")):
            with self.assertRaises(storage.StorageError):
                self.register()
        self.assertFalse(self.authority.exists())
        original = storage.atomic_replace_bytes
        def uncertain(path, data):
            original(path, data)
            raise storage.CommitUncertain(path)
        with patch.object(storage, "atomic_replace_bytes", side_effect=uncertain):
            result = self.register()
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(self.register()["delivery"]["outcome"], "durably_committed")
        with patch.object(storage, "sync_directory", side_effect=OSError("fsync")):
            self.assertEqual(self.register()["delivery"]["outcome"], "durability_uncertain")

    def test_known_publication_survives_verification_exception(self):
        original = storage.atomic_replace_bytes
        def publish(path, data):
            original(path, data)
            self.authority.unlink()
        with patch.object(storage, "atomic_replace_bytes", side_effect=publish):
            result = self.register()
        self.assertEqual(result["delivery"]["outcome"], "durability_uncertain")
        self.assertEqual(result["registration"]["base_commit"], self.base)

    def test_reobservation_rejects_locator_change(self):
        original = repos._observe
        count = 0
        def changed(request):
            nonlocal count
            frozen, status = original(request)
            count += 1
            if count == 2:
                frozen["filesystem"]["root"]["inode"] += 1
            return frozen, status
        with patch.object(repos, "_observe", side_effect=changed), self.assertRaises(repos.RepositoryError):
            self.register()
        self.assertFalse(self.authority.exists())

    def test_concurrent_registration_has_one_winner(self):
        def register(target):
            try:
                return self.register(delivery_target=target)
            except repos.RepositoryError:
                return None
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(register, ["refs/heads/one", "refs/heads/two"]))
        self.assertEqual(sum(r is not None for r in results), 1)

    def test_runner_cleans_descendants_after_successful_leader_exit(self):
        # An opened FIFO writer proves the descendant is ready. EOF after the
        # runner returns proves it no longer holds the observer, without racing
        # sleep/touch timers or requiring a prompt fixture startup on busy hosts.
        fake = self.root / "fake-git"
        fifo = self.root / "descendant-observer"
        pid_file = self.root / "descendant-pid"
        os.mkfifo(fifo)
        observer = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
        fake.write_text(f"#!{sys.executable}\n" + f'''
import os, signal
reader, writer = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(reader)
    connection = os.open({str(fifo)!r}, os.O_WRONLY)
    with open({str(pid_file)!r}, 'w') as stream:
        stream.write(str(os.getpid()))
    sink = os.open(os.devnull, os.O_WRONLY)
    os.dup2(sink, 1)
    os.dup2(sink, 2)
    os.close(sink)
    os.write(writer, b'1')
    os.close(writer)
    while True:
        signal.pause()
os.close(writer)
assert os.read(reader, 1) == b'1'
os.close(reader)
print(pid, flush=True)
''')
        fake.chmod(0o700)
        try:
            # Test-only hang watchdog, not the production probe allowance. The
            # semantic timeout/output assertions below use a controlled clock.
            output = repos._run_git(fake, ("--version",), self.root, repos._environment(), 15)
            self.assertEqual(int(output), int(pid_file.read_text()))
            self.assertTrue(select.select([observer], [], [], 3)[0], "descendant retained its observer after cleanup")
            self.assertEqual(os.read(observer, 1), b"")
        finally:
            os.close(observer)
            if pid_file.exists():
                try:
                    os.kill(int(pid_file.read_text()), signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_observation_budget_and_malformed_status_fail_closed(self):
        with patch.object(repos, "OBSERVATION_TIMEOUT", 0):
            with self.assertRaisesRegex(repos.RepositoryError, "budget"):
                self.register()
        original = repos._run_git
        def malformed(executable, arguments, cwd, env, timeout):
            if arguments[0] == "status":
                return b"?? incomplete"
            return original(executable, arguments, cwd, env, timeout)
        with patch.object(repos, "_run_git", side_effect=malformed), self.assertRaises(repos.RepositoryError):
            self.register()
        self.assertFalse(self.authority.exists())

    def test_bare_subdirectory_and_old_git_rejected(self):
        sub = self.repo / "sub"
        sub.mkdir()
        with self.assertRaises(repos.RepositoryError):
            self.register(path=str(sub))
        bare = self.root / "bare.git"
        self.git("init", "--quiet", "--bare", str(bare))
        with self.assertRaises(repos.RepositoryError):
            self.register(path=str(bare))
        with patch.object(repos, "_run_git", return_value=b"git version 2.43.0\n"):
            with self.assertRaisesRegex(repos.RepositoryError, ">= 2.48"):
                self.register()
        self.assertFalse(self.authority.exists())

    def test_original_symlink_selector_retry_does_not_follow_new_target(self):
        alias = self.root / "alias"
        alias.symlink_to(self.repo, target_is_directory=True)
        original = self.register(path=str(alias))
        alias.unlink()
        alias.symlink_to(self.root / "missing", target_is_directory=True)
        with patch.object(repos, "_observe", side_effect=AssertionError("reopened")):
            retry = self.register(path=str(alias))
        self.assertEqual(retry["registration"], original["registration"])
        self.assertTrue(retry["replayed"])
        self.assertEqual(repos.inspect_repository(self.workspace, verify=True)["status"], "unavailable")

    def test_no_fetch_or_helpers_for_missing_promisor_object(self):
        helper = self.root / "git-remote-evil"
        marker = self.root / "fetched"
        helper.write_text("#!/bin/sh\ntouch '" + str(marker) + "'\n")
        helper.chmod(0o700)
        self.git("config", "remote.origin.url", "evil::example")
        self.git("config", "remote.origin.promisor", "true")
        with patch.dict(os.environ, {"PATH": str(self.root) + os.pathsep + os.environ["PATH"]}):
            with self.assertRaises(repos.RepositoryError):
                self.register(base="f" * 40)
        self.assertFalse(marker.exists())
        self.assertFalse(self.authority.exists())

    def test_status_parser_rejects_malformed_output(self):
        for data in (b"?? name", b"XX name\0", b"?? \0", b"R  name\0", b"   name\0", b"UA\0", b"UM name\0", b"DM name\0"):
            with self.subTest(data=data), self.assertRaises(repos.RepositoryError):
                repos._status(data)
        self.assertEqual(repos._status(b"R  to\0from\0")["tracked_changes"], 1)
        self.assertEqual(repos._status(b"?? nonutf8-\xff\n\0")["untracked"], 1)

    def test_sha256_if_supported(self):
        sha = self.root / "sha256"
        sha.mkdir()
        try:
            self.git("init", "--quiet", "--object-format=sha256", cwd=sha)
        except subprocess.CalledProcessError:
            self.skipTest("Git does not support SHA256 fixtures")
        self.git("commit", "--quiet", "--allow-empty", "-m", "sha256 fixture", cwd=sha)
        base = self.git("rev-parse", "HEAD", cwd=sha).decode().strip()
        result = self.register(path=str(sha), base=base)
        self.assertEqual(result["registration"]["object_format"], "sha256")
        self.assertEqual(len(result["registration"]["base_tree"]), 64)


class GitProbeRunnerTests(unittest.TestCase):
    @contextmanager
    def scripted_probe(self, stdout=(), stderr=(), expired=False):
        # Use real owned descriptors, but scripted readiness/chunks/clock. This
        # exercises the actual accounting/cleanup loop without timing interpreter
        # and executable startup against the semantic deadline being tested.
        pipes = [os.pipe(), os.pipe()]
        streams = [os.fdopen(pair[0], "rb") for pair in pipes]
        process = SimpleNamespace(stdout=streams[0], stderr=streams[1], pid=424242,
                                  wait=mock.Mock())
        chunks = {"out": deque((*stdout, b"")), "err": deque((*stderr, b"")), "control": deque((b"0", b""))}

        class Selector:
            def __init__(self):
                self.keys = {}

            def register(self, fd, events, name):
                self.keys[fd] = SimpleNamespace(fd=fd, data=name)

            def unregister(self, fd):
                del self.keys[fd]

            def get_map(self):
                return self.keys

            def select(self, timeout):
                return [(key, None) for key in self.keys.values()]

            def close(self):
                self.keys.clear()

        selector = Selector()
        def read(fd, size):
            queue = chunks[selector.keys[fd].data]
            chunk = queue.popleft()
            if len(chunk) > size:
                queue.appendleft(chunk[size:])
            return chunk[:size]

        try:
            with patch.object(repos.selectors, "DefaultSelector", return_value=selector), \
                    patch.object(repos.subprocess, "Popen", return_value=process), \
                    patch.object(repos.os, "read", side_effect=read), \
                    patch.object(repos.os, "killpg") as kill, \
                    patch.object(repos.time, "monotonic", **({"side_effect": [0.0, 2.1]} if expired else {"return_value": 0.0})):
                yield process, kill
            kill.assert_called_once_with(process.pid, signal.SIGKILL)
            process.wait.assert_called_once_with(timeout=2)
            self.assertTrue(all(stream.closed for stream in streams))
            self.assertFalse(selector.keys)
        finally:
            for stream in streams:
                stream.close()
            for _, writer in pipes:
                os.close(writer)

    def run_probe(self):
        return repos._run_git(Path("/trusted/git"), ("--version",), Path("/"), repos._environment(), 2, max_output=1024)

    def test_deadline_expiry_cleans_spawned_process_group(self):
        with self.scripted_probe(expired=True):
            with self.assertRaisesRegex(repos.RepositoryError, "timed out"):
                self.run_probe()

    def test_combined_output_limit_cleans_group_without_waiting_for_exit(self):
        with self.scripted_probe(stdout=(b"x" * 600,), stderr=(b"y" * 600,)):
            with self.assertRaisesRegex(repos.RepositoryError, "output limit"):
                self.run_probe()

    def test_exact_combined_limit_retains_only_stdout(self):
        with self.scripted_probe(stdout=(b"x" * 600,), stderr=(b"y" * 424,)):
            self.assertEqual(self.run_probe(), b"x" * 600)

    def test_unallowlisted_arguments_never_spawn(self):
        with patch.object(repos.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(repos.RepositoryError, "not permitted"):
                repos._run_git(Path("/trusted/git"), ("config", "evil.key", "value"), Path("/"), repos._environment(), 2)
            spawn.assert_not_called()


if __name__ == "__main__":
    unittest.main()
