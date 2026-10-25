"""Offline live-heartbeat contracts, using only disposable private artifacts."""

import contextlib
import copy
import io
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock
import urllib.request

from runner import coven, djinn_live_observation as observation
from runner import djinn_observation, launch, storage, workers


ROOT = Path(__file__).resolve().parents[1]
RUNTIME = "616fed3e-ae7f-41a0-99e5-80546d659d76"
INSTANCE = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e"
OTHER = "0c1a32bf-1a23-4da1-8234-123456789abc"
SESSION = "ses_cross_language_busy"
SAMPLED = 1792861200000
SAFE_INT = 2**53 - 1
SECRET = "PRIVATE_TRANSCRIPT_OR_PROVIDER_ERROR_7c238"


def native_fixture():
    """Local copy of the shared TS schema fixture; no sibling repo at test time."""
    return {
        "schema_version": 1, "contract": "djinn.live-observation.v1",
        "runtime_id": RUNTIME, "sequence": 1, "sampled_at_ms": SAMPLED,
        "valid_for_ms": 3000, "source": "legacy_session_run_state",
        "activity_scope": "legacy_runner_ownership",
        "capabilities": {"activity": True, "idle": False, "catalog_complete": False,
                         "decision_state": False, "retry_state": False,
                         "durable_lifecycle": False, "job_completion": False},
        "authorization": "none", "dispatchable": False,
        "native_lifecycle": "unestablished", "job_completion": "unestablished",
        "instances": [{"instance_id": INSTANCE, "sessions": [
            {"session_id": SESSION, "activity": "busy"},
            {"session_id": "ses_cross_language_unknown", "activity": "unknown"}]}],
    }


def changed(value, path, replacement):
    result = copy.deepcopy(value)
    target = result
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = replacement
    return result


class Clock:
    """Replace only the adapter's time binding, never tempfile's global clock."""

    def __init__(self, wall_ms=SAMPLED + 100):
        self.wall_ms = wall_ms
        self.elapsed_ms = 0

    def time(self):
        return self.wall_ms / 1000

    def monotonic(self):
        return self.elapsed_ms / 1000


@contextlib.contextmanager
def no_native_or_workspace_io():
    with contextlib.ExitStack() as stack:
        for target, name in (
            (subprocess, "Popen"), (subprocess, "run"), (socket, "socket"),
            (urllib.request, "urlopen"), (sqlite3, "connect"),
            (djinn_observation, "inspect"), (workers, "run_session_observation_probe"),
            (workers, "run_help_probe"), (workers.shutil, "which"),
            (workers, "inspect_readiness"), (launch, "execute"),
            (coven, "load_team"), (coven, "send_to_agent_pane"),
            (storage, "workspace_lock"), (storage, "transport_gate"),
        ):
            guard = stack.enter_context(mock.patch.object(
                target, name, side_effect=AssertionError("unexpected I/O: " + name)))
            stack.callback(guard.assert_not_called)
        yield


@unittest.skipUnless(os.name == "posix", "Private POSIX artifact contract")
class DjinnLiveObservationTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.root.chmod(0o700)
        self.path = self.root / (RUNTIME + ".json")
        self.fixture = native_fixture()
        self.clock = Clock()
        patch = mock.patch.object(observation, "time", self.clock)
        patch.start()
        self.addCleanup(patch.stop)
        self.write(self.fixture)

    def write(self, value):
        payload = value if isinstance(value, bytes) else json.dumps(value).encode("utf-8")
        self.path.write_bytes(payload)
        self.path.chmod(0o600)

    def inspect(self, **options):
        args = dict(file_path=str(self.path), runtime_id=RUNTIME,
                    instance_id=INSTANCE, session_id=SESSION)
        args.update(options)
        return observation.inspect(**args)

    def assert_unknown(self, report, diagnostic=None, assessment="unavailable"):
        self.assertEqual(report["assessment"], assessment)
        self.assertEqual(report["activity"], "unknown")
        self.assertEqual(report["freshness"], "unknown")
        self.assertEqual(report["expires_in_ms"], 0)
        self.assertIsNone(report["source"])
        self.assertEqual(report["authorization"], "none")
        self.assertIs(report["dispatchable"], False)
        self.assertEqual(report["native_lifecycle"], "unestablished")
        self.assertEqual(report["job_completion"], "unestablished")
        self.assertNotIn(SECRET, json.dumps(report))
        if diagnostic is not None:
            self.assertEqual(report["diagnostics"], [diagnostic])

    def reject(self, value, diagnostic="schema_error", assessment="unavailable"):
        self.write(value)
        report = self.inspect()
        self.assert_unknown(report, diagnostic, assessment)
        return report

    def argv(self):
        return ["observe", "djinn-live", "--file", str(self.path), "--runtime", RUNTIME,
                "--instance", INSTANCE, "--session", SESSION]

    def command(self, extra=(), expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(self.argv() + list(extra)), expected)
        self.assertTrue(output.getvalue().isascii())
        self.assertNotIn("\x1b", output.getvalue())
        return json.loads(output.getvalue()), output.getvalue()

    def test_shared_fixture_busy_and_honest_unknown_are_not_authority(self):
        self.assertEqual(len(self.fixture), 14)
        for session, activity in ((SESSION, "busy"), ("ses_cross_language_unknown", "unknown")):
            with self.subTest(activity=activity):
                report = self.inspect(session_id=session)
                self.assertEqual(report["assessment"], "observed")
                self.assertEqual(report["activity"], activity)
                self.assertEqual(report["freshness"], "fresh_sample")
                self.assertEqual(report["expires_in_ms"], 2900)
                self.assertEqual(report["adapter_contract"], "coven.djinn.live-observation.v1")
                self.assertEqual(report["selection"], dict(runtime_id=RUNTIME, instance_id=INSTANCE,
                                                           session_id=session))
                self.assertEqual(report["attention"], "unknown")
                self.assertEqual(report["activity_scope"], "legacy_runner_ownership")
                self.assertEqual(report["authorization"], "none")
                self.assertIs(report["dispatchable"], False)
                self.assertEqual(report["native_lifecycle"], "unestablished")
                self.assertEqual(report["job_completion"], "unestablished")
                expected = {k: self.fixture[k] for k in (
                    "contract", "runtime_id", "sequence", "sampled_at_ms", "valid_for_ms",
                    "source", "activity_scope", "capabilities")}
                expected.update(instance_id=INSTANCE, session_id=session)
                self.assertEqual(report["source"], expected)
                self.assertEqual(report, json.loads(json.dumps(report, allow_nan=False)))
                limitations = " ".join(report["limitations"]).lower()
                for phrase in ("health", "idle", "authentication", "rollback", "syscalls", "completion"):
                    self.assertIn(phrase, limitations)

    def test_invalid_arguments_precede_lstat_open_or_any_io(self):
        cases = [{"file_path": v} for v in (
            None, self.path, "", "relative", "/", str(self.path) + "/", "/a/../" + self.path.name,
            "/a\x00b", "/a\nb", "/a\x7fb", "/a\x85b", "/\ud800",
            "/" + "x" * 4096, "/" + "\u00e9" * 2048, str(self.root / "other.json"))]
        invalid_uuids = (None, "", 1, RUNTIME.upper(), RUNTIME.replace("-", ""),
                         "616fed3e-ae7f-11a0-99e5-80546d659d76",
                         "616fed3e-ae7f-41a0-79e5-80546d659d76")
        cases += [{key: v} for key in ("runtime_id", "instance_id") for v in invalid_uuids]
        cases += [{"session_id": v} for v in (None, 1, "", "x" * 129, "a b", "\t", "\n",
                                                           "\x1b", "\x7f", "\u00e9", "\ud800")]
        cases += [{"timeout": v} for v in (0, -1, 1.001, True, False, "1", None,
                                                          float("nan"), float("inf"), 10**1000)]
        with no_native_or_workspace_io(), contextlib.ExitStack() as stack:
            for target, name in ((os, "lstat"), (os, "stat"), (os, "open"),
                                 (os.path, "realpath"), (__import__("builtins"), "open")):
                stack.enter_context(mock.patch.object(target, name, side_effect=AssertionError(name)))
            for options in cases:
                with self.subTest(options=options), self.assertRaises(ValueError):
                    self.inspect(**options)
            for args in ((), (str(self.path),), (str(self.path), RUNTIME, INSTANCE)):
                with self.subTest(args=args), self.assertRaises(TypeError):
                    observation.inspect(*args)
            with self.assertRaises(TypeError):
                self.inspect(max_output_bytes=65536)

    def test_timeout_positive_at_most_one_and_not_a_syscall_interrupt(self):
        for timeout in (0.001, 0.5, 1, 1.0):
            with self.subTest(timeout=timeout):
                self.assertEqual(self.inspect(timeout=timeout)["assessment"], "observed")
        decode = observation._decode

        def slow_decode(*args):
            self.clock.elapsed_ms += 1100
            return decode(*args)

        with mock.patch.object(observation, "_decode", side_effect=slow_decode) as decoded:
            self.assert_unknown(self.inspect(), "timeout")
        decoded.assert_called_once()

    def test_frozen_root_and_nested_shapes_reject_missing_and_extra_fields(self):
        for path in ((), ("instances", 0), ("instances", 0, "sessions", 0)):
            target = self.fixture
            for key in path:
                target = target[key]
            for key in target:
                replacement = copy.deepcopy(target)
                del replacement[key]
                with self.subTest(path=path, missing=key):
                    self.reject(changed(self.fixture, path, replacement) if path else replacement)
            replacement = {**target, "raw_payload": SECRET}
            with self.subTest(path=path, extra=True):
                self.reject(changed(self.fixture, path, replacement) if path else replacement)
        for path, values in ((("instances",), (None, {}, "instances")),
                             (("instances", 0), (None, [], True)),
                             (("instances", 0, "sessions"), (None, {}, "sessions")),
                             (("instances", 0, "sessions", 0), (None, [], True))):
            for value in values:
                with self.subTest(path=path, value=value):
                    self.reject(changed(self.fixture, path, value))

    def test_unsupported_contract_and_schema_types(self):
        for field, value in (("schema_version", 2), ("schema_version", 0),
                             ("contract", "djinn.live-observation.v2"),
                             ("contract", "djinn.session.observation.v1")):
            with self.subTest(field=field, value=value):
                self.reject(changed(self.fixture, (field,), value), "unsupported_contract", "incompatible")
        for field, value in (("schema_version", True), ("schema_version", 1.0),
                             ("schema_version", "1"), ("contract", None)):
            with self.subTest(field=field, value=value):
                self.reject(changed(self.fixture, (field,), value))

    def test_exact_capabilities_are_booleans_not_numeric_or_advisory(self):
        for key, expected in self.fixture["capabilities"].items():
            for invalid in (int(expected), not expected, None, "true"):
                with self.subTest(key=key, invalid=invalid):
                    self.reject(changed(self.fixture, ("capabilities", key), invalid))
            caps = dict(self.fixture["capabilities"])
            del caps[key]
            self.reject(changed(self.fixture, ("capabilities",), caps))
        for caps in (None, [], {}, {**self.fixture["capabilities"], "execute": False}):
            with self.subTest(caps=caps):
                self.reject(changed(self.fixture, ("capabilities",), caps))

    def test_source_scope_ttl_and_non_authority_are_fixed(self):
        for field, values in {
            "source": ["legacy_run_state", "v2", None],
            "activity_scope": ["global", None], "valid_for_ms": [2999, 3001, True, 3000.0],
            "authorization": ["allowed", None], "dispatchable": [True, 0, None],
            "native_lifecycle": ["running", "completed", None],
            "job_completion": ["success", True, None],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.reject(changed(self.fixture, (field,), value))

    def test_javascript_safe_integer_sequence_and_sample_boundaries(self):
        for field in ("sequence", "sampled_at_ms"):
            invalids = [-1, True, False, 1.0, "1", None, SAFE_INT + 1]
            if field == "sequence":
                invalids.append(0)
            for invalid in invalids:
                with self.subTest(field=field, invalid=invalid):
                    self.reject(changed(self.fixture, (field,), invalid))
        for sequence in (1, SAFE_INT):
            self.write(changed(self.fixture, ("sequence",), sequence))
            self.assertEqual(self.inspect()["source"]["sequence"], sequence)
        for sampled in (0, SAFE_INT):
            with self.subTest(sampled=sampled):
                # Leave room for float seconds to round at the JS integer ceiling.
                self.clock.wall_ms = sampled + 100
                self.write(changed(self.fixture, ("sampled_at_ms",), sampled))
                self.assertEqual(self.inspect()["assessment"], "observed")

    def test_all_source_uuids_require_canonical_v4_rfc_variant(self):
        for path in (("runtime_id",), ("instances", 0, "instance_id")):
            for invalid in (None, 1, "", RUNTIME.upper(), RUNTIME.replace("-", ""),
                            "616fed3e-ae7f-11a0-99e5-80546d659d76",
                            "616fed3e-ae7f-41a0-79e5-80546d659d76", "{" + RUNTIME + "}"):
                with self.subTest(path=path, invalid=invalid):
                    self.reject(changed(self.fixture, path, invalid))

    def test_opaque_session_ids_and_activities_validate_unselected_entries(self):
        for invalid in (None, 1, "", "x" * 129, "a b", "a\n", "\x00", "\x1b", "\x7f", "\u00e9", "\ud800"):
            with self.subTest(invalid=invalid):
                self.reject(changed(self.fixture, ("instances", 0, "sessions", 1, "session_id"), invalid))
        for invalid in ("idle", "done", "working", "stopped", "v2", None, True, {}, []):
            with self.subTest(activity=invalid):
                self.reject(changed(self.fixture, ("instances", 0, "sessions", 1, "activity"), invalid))
        for session in ("!", "~", "x" * 128, "folder/native:opaque?x=[y]"):
            self.write(changed(self.fixture, ("instances", 0, "sessions", 0, "session_id"), session))
            self.assertEqual(self.inspect(session_id=session)["activity"], "busy")
        value = copy.deepcopy(self.fixture)
        value["instances"].append({"instance_id": OTHER, "sessions": [
            {"session_id": "bad id", "activity": "busy"}]})
        self.reject(value)
        self.assert_unknown(self.inspect(instance_id=OTHER), "schema_error")
        self.assert_unknown(self.inspect(session_id="missing"), "schema_error")

    def test_duplicate_ids_and_exact_instance_session_total_limits(self):
        value = copy.deepcopy(self.fixture)
        value["instances"].append(copy.deepcopy(value["instances"][0]))
        self.reject(value)
        value = copy.deepcopy(self.fixture)
        value["instances"][0]["sessions"].append(copy.deepcopy(value["instances"][0]["sessions"][0]))
        self.reject(value)
        value = copy.deepcopy(self.fixture)
        value["instances"] += [{"instance_id": f"00000000-0000-4000-8000-{i:012x}",
                                "sessions": []} for i in range(15)]
        self.write(value)
        self.assertEqual(self.inspect()["assessment"], "observed")
        value["instances"].append({"instance_id": OTHER, "sessions": []})
        self.reject(value)
        value = copy.deepcopy(self.fixture)
        value["instances"][0]["sessions"] = [{"session_id": SESSION, "activity": "busy"}] + [
            {"session_id": f"s{i}", "activity": "unknown"} for i in range(63)]
        value["instances"].append({"instance_id": OTHER, "sessions": [
            {"session_id": f"s{i}", "activity": "unknown"} for i in range(64)]})
        self.write(value)
        self.assertEqual(self.inspect()["assessment"], "observed")
        value["instances"][1]["sessions"].append({"session_id": "extra", "activity": "busy"})
        self.reject(value)

    def test_missing_noop_and_restart_never_follow_or_infer_idle(self):
        for instances, diagnostic in (([], "instance_not_observed"),
                                     ([{"instance_id": INSTANCE, "sessions": []}], "session_not_observed"),
                                     ([{"instance_id": OTHER, "sessions": self.fixture["instances"][0]["sessions"]}],
                                      "instance_not_observed")):
            with self.subTest(diagnostic=diagnostic):
                self.reject(changed(self.fixture, ("instances",), instances), diagnostic)
        self.reject(changed(self.fixture, ("runtime_id",), OTHER), "runtime_mismatch")
        self.write(changed(self.fixture, ("instances", 0, "instance_id"), OTHER))
        self.assert_unknown(self.inspect(), "instance_not_observed")
        self.assertEqual(self.inspect(instance_id=OTHER)["assessment"], "observed")
        new_path = self.root / (OTHER + ".json")
        value = changed(self.fixture, ("runtime_id",), OTHER)
        new_path.write_text(json.dumps(value), encoding="utf-8")
        new_path.chmod(0o600)
        self.path.unlink()
        self.assert_unknown(self.inspect(), "io_error")
        self.assertEqual(self.inspect(file_path=str(new_path), runtime_id=OTHER)["assessment"], "observed")

    def test_strict_json_utf8_duplicates_nonfinite_depth_and_corruption(self):
        for payload, diagnostic in (
            (b"\xff", "invalid_utf8"), (b"", "invalid_json"), (b"{} {}", "invalid_json"),
            (b'{"x":1,"x":2}', "duplicate_key"),
            (b'{"instances":[{"session_id":1,"session_id":2}]}', "duplicate_key"),
            (b'{"x":NaN}', "nonfinite_json"), (b'{"x":Infinity}', "nonfinite_json"),
            (b'{"x":-Infinity}', "nonfinite_json"), (b'{"x":1e999}', "schema_error"),
            (b"[" * 33 + b"0" + b"]" * 33, "nesting_limit"),
            (b"[]", "schema_error"), (b"null", "schema_error"),
            (SECRET.encode(), "invalid_json"),
        ):
            with self.subTest(diagnostic=diagnostic, payload=payload[:40]):
                self.reject(payload, diagnostic)
        self.write(self.fixture)
        with mock.patch.object(os, "open", side_effect=OSError(SECRET)):
            self.assert_unknown(self.inspect(), "io_error")

    def test_exact_fixed_byte_budget_and_bounded_read_while_growing(self):
        payload = json.dumps(self.fixture).encode()
        self.write(payload + b" " * (65536 - len(payload)))
        self.assertEqual(self.inspect()["assessment"], "observed")
        with mock.patch.object(os, "open", side_effect=AssertionError("oversize must precede open")):
            self.reject(b"x" * 65537, "response_limit")
        self.write(self.fixture)
        fdopen = os.fdopen
        sizes = []

        class GrowingStream:
            def __init__(inner, fd, mode):
                inner.stream = fdopen(fd, mode)

            def __enter__(inner):
                return inner

            def __exit__(inner, *args):
                inner.stream.close()

            def fileno(inner):
                return inner.stream.fileno()

            def read(inner, size):
                sizes.append(size)
                with self.path.open("ab") as writer:
                    writer.write(b" " * 70000)
                return inner.stream.read(size)

        with mock.patch.object(os, "fdopen", GrowingStream):
            self.assert_unknown(self.inspect(), "response_limit")
        self.assertEqual(sizes, [65537])

    def test_private_file_and_parent_modes_are_exact(self):
        for mode in (0o000, 0o400, 0o640, 0o644, 0o660, 0o700, 0o1600):
            with self.subTest(file_mode=oct(mode)):
                self.path.chmod(mode)
                self.assert_unknown(self.inspect(), "unsafe_artifact")
        self.path.chmod(0o600)
        try:
            for mode in (0o000, 0o500, 0o710, 0o750, 0o755, 0o1700):
                with self.subTest(parent_mode=oct(mode)):
                    self.root.chmod(mode)
                    self.assert_unknown(self.inspect(), "unsafe_directory")
        finally:
            self.root.chmod(0o700)

    def test_wrong_uid_leaf_symlink_hardlink_fifo_and_missing_rejected(self):
        lstat = os.lstat

        def wrong_uid(path, *args, **kwargs):
            info = lstat(path, *args, **kwargs)
            if str(path) == selected:
                return SimpleNamespace(**{name: (info.st_uid + 1 if name == "st_uid" else getattr(info, name))
                                           for name in ("st_mode", "st_uid", "st_dev", "st_ino", "st_nlink",
                                                        "st_size", "st_mtime_ns", "st_ctime_ns")})
            return info

        for selected, diagnostic in ((str(self.path), "unsafe_artifact"), (str(self.root), "unsafe_directory")):
            with self.subTest(selected=selected), mock.patch.object(os, "lstat", side_effect=wrong_uid):
                self.assert_unknown(self.inspect(), diagnostic)
        other = self.root / "original"
        self.path.rename(other)
        self.path.symlink_to(other)
        self.assert_unknown(self.inspect(), "unsafe_artifact")
        self.path.unlink()
        os.link(other, self.path)
        self.assert_unknown(self.inspect(), "unsafe_artifact")
        self.path.unlink()
        os.mkfifo(self.path, 0o600)
        with mock.patch.object(os, "open", side_effect=AssertionError("must not open FIFO")):
            self.assert_unknown(self.inspect(), "unsafe_artifact")
        self.path.unlink()
        self.path.mkdir(mode=0o700)
        self.assert_unknown(self.inspect(), "unsafe_artifact")
        self.path.rmdir()
        self.assert_unknown(self.inspect(), "io_error")

    def test_parent_canonical_identity_and_no_symlink_parent(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        self.assert_unknown(self.inspect(file_path=str(alias / self.path.name)), "unsafe_directory")
        other = self.root / "other"
        other.mkdir(mode=0o700)
        with mock.patch.object(os.path, "realpath", return_value=str(other)):
            self.assert_unknown(self.inspect(), "directory_changed")

    def test_fd_and_path_stamps_checked_before_decode(self):
        fstat = os.fstat
        lstat = os.lstat
        decode = observation._decode
        for phase in ("before_fd", "after_fd", "after_path"):
            for field in ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns"):
                calls = 0

                def mismatch(selected, *args, **kwargs):
                    nonlocal calls
                    info = (lstat if phase == "after_path" else fstat)(selected, *args, **kwargs)
                    if phase == "after_path" and str(selected) != str(self.path):
                        return info
                    calls += 1
                    if calls != (1 if phase == "before_fd" else 2):
                        return info
                    names = ("st_mode", "st_uid", "st_nlink", "st_dev", "st_ino", "st_size",
                             "st_mtime_ns", "st_ctime_ns")
                    return SimpleNamespace(**{name: getattr(info, name) + (name == field) for name in names})

                with self.subTest(phase=phase, field=field), \
                        mock.patch.object(os, "lstat" if phase == "after_path" else "fstat", side_effect=mismatch), \
                        mock.patch.object(observation, "_decode", wraps=decode) as decoded:
                    self.assert_unknown(self.inspect(), "artifact_changed")
                self.assertEqual(decoded.call_count, 0 if phase == "before_fd" else 1)

    def test_file_replacement_and_metadata_change_during_decode_discard_evidence(self):
        decode = observation._decode
        for kind in ("replace", "size", "mtime", "ctime", "mode"):
            self.write(self.fixture)

            def mutate(*args):
                result = decode(*args)
                info = self.path.stat()
                if kind == "replace":
                    replacement = self.root / "replacement"
                    replacement.write_bytes(self.path.read_bytes())
                    replacement.chmod(0o600)
                    os.replace(replacement, self.path)
                elif kind == "size":
                    with self.path.open("ab") as stream:
                        stream.write(b" ")
                elif kind == "mtime":
                    os.utime(self.path, ns=(info.st_atime_ns, info.st_mtime_ns + 1000000000))
                elif kind == "ctime":
                    self.path.chmod(0o640)
                    self.path.chmod(0o600)
                    os.utime(self.path, ns=(info.st_atime_ns, info.st_mtime_ns))
                else:
                    self.path.chmod(0o640)
                return result

            with self.subTest(kind=kind), mock.patch.object(observation, "_decode", side_effect=mutate):
                self.assert_unknown(self.inspect())

    def test_directory_replacement_or_privacy_change_during_decode_rejected(self):
        decode = observation._decode
        private = self.root / "private"
        private.mkdir(mode=0o700)
        self.path = private / self.path.name
        self.write(self.fixture)

        def replace(*args):
            result = decode(*args)
            private.rename(self.root / "old-private")
            private.mkdir(mode=0o700)
            os.link(self.root / "old-private" / self.path.name, self.path)
            (self.root / "old-private" / self.path.name).unlink()
            return result

        with mock.patch.object(observation, "_decode", side_effect=replace):
            self.assert_unknown(self.inspect(), "artifact_changed")

        def chmod(*args):
            result = decode(*args)
            private.chmod(0o750)
            return result

        with mock.patch.object(observation, "_decode", side_effect=chmod):
            self.assert_unknown(self.inspect(), "unsafe_directory")

    def test_source_freshness_future_stale_touch_and_copy_never_renew(self):
        for age, diagnostic in ((-1, "future_sample"), (3000, "stale_sample"), (60000, "stale_sample")):
            self.clock.wall_ms = SAMPLED + age
            with self.subTest(age=age):
                self.assert_unknown(self.inspect(), diagnostic)
        os.utime(self.path, None)
        self.assert_unknown(self.inspect(), "stale_sample")
        payload = self.path.read_bytes()
        self.path.unlink()
        self.write(payload)
        self.assert_unknown(self.inspect(), "stale_sample")
        self.clock.wall_ms = SAMPLED + 2999
        self.assertEqual(self.inspect()["expires_in_ms"], 1)

    def test_wall_time_at_end_plus_monotonic_deduction_and_deadline(self):
        decode = observation._decode
        for wall_advance, elapsed, timeout, expected in (
            (400, 200, 1, 2300), (0, 200, 1, 2700),
            (0, 1000, 1, "timeout"), (0, 50, 0.05, "timeout"),
            (2800, 100, 1, "stale_sample"),
        ):
            self.clock.wall_ms = SAMPLED + 100
            self.clock.elapsed_ms = 0

            def advance(*args):
                result = decode(*args)
                self.clock.wall_ms += wall_advance
                self.clock.elapsed_ms += elapsed
                return result

            with self.subTest(expected=expected), mock.patch.object(observation, "_decode", side_effect=advance):
                report = self.inspect(timeout=timeout)
            if isinstance(expected, int):
                self.assertEqual(report["assessment"], "observed")
                self.assertEqual(report["expires_in_ms"], expected)
            else:
                self.assert_unknown(report, expected)

    def test_remaining_ttl_decreases_no_positive_cache_or_saved_report_import(self):
        first = self.inspect()
        self.clock.wall_ms += 800
        second = self.inspect()
        self.assertEqual(first["expires_in_ms"] - second["expires_in_ms"], 800)
        self.reject(first, "schema_error")
        self.reject(b"broken", "invalid_json")
        self.path.unlink()
        self.assert_unknown(self.inspect(), "io_error")
        self.write(self.fixture)
        self.assertEqual(self.inspect()["expires_in_ms"], second["expires_in_ms"])

    def test_cli_explicit_selectors_and_invalid_options_never_do_io(self):
        base = self.argv()
        cases = [["observe", "djinn-live"]]
        for flag in ("--file", "--runtime", "--instance", "--session"):
            index = base.index(flag)
            cases.append(base[:index] + base[index + 2:])
        cases += [base + extra for extra in (["--folder", str(self.root)], ["--socket", "/s"],
                  ["--method", "session.run"], ["--max-output-bytes", "65536"], ["--follow"],
                  ["--import", str(self.path)], ["--workspace", str(self.root)])]
        with no_native_or_workspace_io(), \
                mock.patch.object(observation, "inspect", side_effect=AssertionError("unexpected dispatch")):
            for args in cases:
                with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), \
                        self.assertRaises(SystemExit) as raised:
                    coven.main(args)
                self.assertEqual(raised.exception.code, 2)
        for extra in (["--timeout", "nan"], ["--timeout", "0"], ["--timeout", "1.01"],
                      ["--file", "relative"], ["--runtime", OTHER], ["--session", "a b"]):
            with self.subTest(extra=extra), no_native_or_workspace_io(), \
                    mock.patch.object(os, "lstat", side_effect=AssertionError("lstat")), \
                    mock.patch.object(os, "open", side_effect=AssertionError("open")), \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                coven.main(base + extra)
            self.assertEqual(raised.exception.code, 1)

    def test_cli_exit_codes_ascii_json_and_human_output(self):
        for activity in ("busy", "unknown"):
            for compact in (False, True):
                value = changed(self.fixture, ("instances", 0, "sessions", 0, "activity"), activity)
                self.write(value)
                with self.subTest(activity=activity, compact=compact), no_native_or_workspace_io():
                    report, output = self.command(["--json"] if compact else [])
                self.assertEqual(report["activity"], activity)
                self.assertEqual("\n" in output.rstrip("\n"), not compact)
        for payload, code, diagnostic in ((b"bad", 1, "invalid_json"),
                (changed(self.fixture, ("schema_version",), 2), 2, "unsupported_contract")):
            self.write(payload)
            with no_native_or_workspace_io():
                report, _ = self.command(["--json"], expected=code)
            self.assert_unknown(report, diagnostic, "incompatible" if code == 2 else "unavailable")

    def test_no_fallback_workspace_reads_writes_or_inherited_config_dependency(self):
        workspace = self.root / "corrupt-workspace"
        workspace.mkdir(mode=0o700)
        files = ("coven.json", "state/tasks.json", "state/agents.json", "state/task-ledger.json",
                 "state/task-attempts.json", "state/groups.json", "state/execution.json",
                 "state/runtime-launch-control.json", "state/repository.json")
        for name in files:
            path = workspace / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"corrupt\xff\x00" + name.encode())
        before = {str(p.relative_to(workspace)): p.read_bytes() if p.is_file() else None
                  for p in workspace.rglob("*")}
        info = self.path.stat()
        directory_info = self.root.stat()
        payload = self.path.read_bytes()
        env = {key: SECRET for key in ("DJINN_CONFIG", "DJINN_HOME", "XDG_CONFIG_HOME",
               "KITSUNE_SOCKET_PATH", "KITSUNE_CLIENT_SOCKET_PATH", "COVEN_WORKSPACE")}
        real_open = os.open

        def readonly(path, flags, *args, **kwargs):
            self.assertEqual(str(path), str(self.path))
            self.assertEqual(flags & os.O_ACCMODE, os.O_RDONLY)
            self.assertFalse(flags & (os.O_CREAT | os.O_TRUNC | os.O_APPEND))
            self.assertTrue(flags & os.O_NOFOLLOW)
            self.assertTrue(flags & os.O_NONBLOCK)
            return real_open(path, flags, *args, **kwargs)

        with contextlib.chdir(workspace), mock.patch.dict(os.environ, env), \
                no_native_or_workspace_io(), mock.patch.object(os, "open", side_effect=readonly) as opened, \
                mock.patch.object(Path, "read_bytes", side_effect=AssertionError("workspace read")), \
                mock.patch("builtins.open", side_effect=AssertionError("implicit file read/write")):
            self.assertEqual(self.inspect()["assessment"], "observed")
            self.command(["--json"])
        self.assertEqual(opened.call_count, 2)
        self.assertEqual(before, {str(p.relative_to(workspace)): p.read_bytes() if p.is_file() else None
                                  for p in workspace.rglob("*")})
        self.assertEqual(self.path.read_bytes(), payload)
        after = self.path.stat()
        for field in ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns", "st_mode", "st_uid", "st_nlink"):
            self.assertEqual(getattr(info, field), getattr(after, field), field)
            self.assertEqual(getattr(directory_info, field), getattr(self.root.stat(), field), field)
        self.path.unlink()
        with no_native_or_workspace_io():
            self.assert_unknown(self.inspect(), "io_error")
            self.command(["--json"], expected=1)

    def test_actual_wrapper_dynamic_fresh_sample_ascii_and_corrupt_workspace(self):
        workspace = self.root / "workspace with spaces \u00e9"
        workspace.mkdir(mode=0o700)
        corrupt = workspace / "coven.json"
        corrupt.write_bytes(b"corrupt\xff\x00")
        env = {k: v for k, v in os.environ.items() if k not in {"COVEN_HOME", "COVEN_COMMAND", "PYTHONPATH"}}
        env.update(PYTHONDONTWRITEBYTECODE="1", DJINN_CONFIG="/never-use", DJINN_HOME="/never-use",
                   KITSUNE_SOCKET_PATH="/never-use", KITSUNE_CLIENT_SOCKET_PATH="/never-use",
                   COVEN_WORKSPACE=str(workspace))
        for kind, code in (("busy", 0), ("unknown", 0), ("stale", 1), ("v2", 2)):
            value = native_fixture()
            value["sampled_at_ms"] = int(time.time() * 1000) - (60000 if kind == "stale" else 0)
            if kind == "unknown":
                value["instances"][0]["sessions"][0]["activity"] = "unknown"
            if kind == "v2":
                value["schema_version"] = 2
            self.write(value)
            with self.subTest(kind=kind):
                response = subprocess.run([str(ROOT / "bin/coven"), *self.argv(), "--json"],
                    cwd=workspace, env=env, capture_output=True, timeout=10)
                self.assertEqual(response.returncode, code, response.stderr.decode(errors="replace"))
                self.assertEqual(response.stderr, b"")
                self.assertTrue(response.stdout.isascii())
                report = json.loads(response.stdout)
                self.assertNotIn(b"\x1b", response.stdout)
                if code == 0:
                    self.assertEqual(report["activity"], kind)
                    self.assertGreater(report["expires_in_ms"], 0)
                    self.assertLessEqual(report["expires_in_ms"], 3000)
                else:
                    self.assert_unknown(report, assessment="incompatible" if code == 2 else "unavailable")
                self.assertEqual(corrupt.read_bytes(), b"corrupt\xff\x00")
                self.assertEqual(list(workspace.iterdir()), [corrupt])


if __name__ == "__main__":
    unittest.main()
