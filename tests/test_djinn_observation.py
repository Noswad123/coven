"""Offline persisted Djinn observation contracts; never invoke native Djinn."""

import contextlib
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from runner import coven, djinn_observation as observation, storage, workers


ROOT = Path(__file__).resolve().parents[1]
EMPTY_HASH = hashlib.sha256(b"").hexdigest()
SECRET = "PRIVATE_PRODUCER_TRANSCRIPT_7c238"


def native_fixture(folder):
    """The producer envelope uses folder records, not a live native session."""
    return {
        "schema_version": 1, "contract": "djinn.session.observation.v1",
        "selected_folder": folder, "resolved_folder": folder,
        "started_at": "2026-10-24T12:00:00Z",
        "observed_at": "2026-10-24T12:00:01+00:00",
        "assessment": "observed", "diagnostics": [],
        "capabilities": {"persisted_folder_events": True, "live_status": False,
                         "ui_sessions": False, "native_store": False},
        "authorization": "none", "dispatchable": False,
        "runtime_fact_freshness": "unknown", "native_lifecycle": "unestablished",
        "job_completion": "unestablished", "activity": "unknown",
        "observation": {
            "recorded_native_session_id": "agt_test_123",
            "recorded_created_at": "2026-10-24T11:00:00Z",
            "sources": {
                "manifest": {"path": "djinn.toml", "bytes": 150, "sha256": "a" * 64},
                "folder_events": {"path": "events.jsonl", "status": "observed",
                                  "bytes": 400, "sha256": "b" * 64}},
            "event_count": 2,
            "last_event": {"kind": "session_lifecycle_updated", "event_id": "evt-2",
                           "session_id": "agt_test_123", "recorded_at": "2026-10-24T11:30:00Z"},
            "last_lifecycle": {"state": "completed", "mode": "background",
                               "event_id": "evt-2", "session_id": "agt_test_123",
                               "recorded_at": "2026-10-24T11:30:00Z"},
            "consistency": "stable_per_file_not_transactional"}}


def probe_result(payload, *, status="ok", code=0, stderr=b""):
    if not isinstance(payload, bytes):
        payload = json.dumps(payload, ensure_ascii=True).encode("utf-8")
    return workers.ProbeResult(status, code, payload, stderr, 1.0)


def change(value, path, replacement):
    result = copy.deepcopy(value)
    target = result
    for key in path[:-1]:
        target = target[key]
    target[path[-1]] = replacement
    return result


class DjinnObservationTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.folder = str(self.root)
        self.executable = self.root / "private-producer"
        self.executable.write_bytes(b"#!/bin/sh\nexit 99\n")
        self.executable.chmod(0o700)
        self.fixture = native_fixture(self.folder)

    def inspect(self, payload=None, **kwargs):
        result = probe_result(self.fixture if payload is None else payload)
        with mock.patch.object(workers, "run_session_observation_probe", return_value=result) as probe:
            report = observation.inspect(str(self.executable), self.folder, **kwargs)
        self.assertEqual(probe.call_count, 1)
        return report

    def rejected(self, payload, diagnostic="schema_error", **kwargs):
        report = self.inspect(payload, **kwargs)
        self.assertEqual(report["assessment"], "unavailable")
        self.assertEqual(report["diagnostics"], [diagnostic])
        self.assertIsNone(report["observation"])
        self.assertIsNone(report["producer"])
        self.assertNotIn(SECRET, json.dumps(report))
        return report

    def test_persisted_lifecycle_never_establishes_runtime_or_authority(self):
        self.assertEqual(observation.ADAPTER_CONTRACT, "coven.djinn.observation.v1")
        self.assertEqual(observation.NATIVE_CONTRACT, "djinn.session.observation.v1")
        for state in ("created", "running", "paused", "completed", "failed", "cancelled"):
            with self.subTest(state=state):
                value = change(self.fixture, ("observation", "last_lifecycle", "state"), state)
                report = self.inspect(value)
                self.assertEqual(report["assessment"], "observed")
                self.assertEqual(report["observation"]["last_lifecycle"]["state"], state)
                self.assertEqual(report["native_lifecycle"], "unestablished")
                self.assertEqual(report["job_completion"], "unestablished")
                self.assertEqual(report["activity"], "unknown")
                self.assertEqual(report["runtime_fact_freshness"], "unknown")
                self.assertEqual(report["authorization"], "none")
                self.assertIs(report["dispatchable"], False)
                self.assertEqual(report, json.loads(json.dumps(report, allow_nan=False)))

    def test_absent_event_envelope_timestamps_and_ids_remain_none(self):
        value = copy.deepcopy(self.fixture)
        facts = value["observation"]
        facts["recorded_created_at"] = None
        facts["recorded_native_session_id"] = None
        for field in ("last_event", "last_lifecycle"):
            for key in ("recorded_at", "session_id", "event_id"):
                facts[field][key] = None
        report = self.inspect(value)
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["observation"], facts)
        self.assertIsNotNone(report["observed_at"])
        self.assertIsNone(report["observation"]["last_event"]["recorded_at"])

    def test_required_arguments_and_validation_precede_filesystem_or_process_io(self):
        cases = [{"folder": v} for v in (None, Path(self.folder), "", "relative", "/", "/a/",
                 "/a/../b", "/a\x00b", "/a\nb", "/a\x7fb", "/a\x85b", "/\ud800",
                 "/" + "x" * 4096, "/" + "\u00e9" * 2048)]
        cases += [{"executable": v} for v in (None, "djinn", "", Path(self.folder), "/a/../bin")]
        cases += [{"timeout": v} for v in (0, -1, 31, True, "3", float("nan"), float("inf"), 10 ** 1000)]
        cases += [{"max_output_bytes": v} for v in (1023, 1048577, True, 1024.0)]
        with mock.patch.object(Path, "resolve", side_effect=AssertionError("filesystem I/O")), \
                mock.patch.object(workers, "run_session_observation_probe") as probe:
            for options in cases:
                args = {"executable": str(self.executable), "folder": self.folder, **options}
                with self.subTest(options=options), self.assertRaises(ValueError):
                    observation.inspect(**args)
            for args in ((), (str(self.executable),)):
                with self.subTest(args=args), self.assertRaises(TypeError):
                    observation.inspect(*args)
        probe.assert_not_called()

    def test_one_explicit_probe_no_discovery_workspace_or_native_fallback(self):
        with mock.patch.object(workers.shutil, "which", side_effect=AssertionError("PATH discovery")), \
                mock.patch.object(workers, "run_help_probe", side_effect=AssertionError("help fallback")), \
                mock.patch.object(workers, "inspect_readiness", side_effect=AssertionError("readiness")), \
                mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("workspace")), \
                mock.patch.object(workers, "run_session_observation_probe", return_value=probe_result(b"bad")) as probe:
            report = observation.inspect(str(self.executable), self.folder)
        self.assertEqual(report["assessment"], "unavailable")
        probe.assert_called_once_with(executable=self.executable, folder=self.folder, timeout=3.0,
                                      max_output_bytes=131072, cwd="/",
                                      env={k: v for k, v in os.environ.items() if k in workers.ENV_KEYS})
        self.assertEqual(report["request"], {"method": "session.observe"})

    def test_strict_json_rejects_malformed_duplicates_nonfinite_depth_and_size(self):
        cases = [(b"\xff", "invalid_utf8"), (b"", "invalid_json"), (b"{} {}", "invalid_json"),
                 (b'{"x":1,"x":2}', "duplicate_key"),
                 (b'{"extension":{"x":1,"x":2}}', "duplicate_key"),
                 (b'{"x":NaN}', "nonfinite_json"), (b'{"x":Infinity}', "nonfinite_json"),
                 (b'{"x":-Infinity}', "nonfinite_json"), (b'{"x":1e999}', "schema_error"),
                 (b"[" * 33 + b"0" + b"]" * 33, "nesting_limit"),
                 (b"[]", "schema_error"), (b"null", "schema_error"),
                 (json.dumps({**self.fixture, "extension": [0] * 4097}).encode(), "schema_error"),
                 (json.dumps({**self.fixture, "extension": "x" * 4097}).encode(), "schema_error"),
                 (b"x" * 131073, "output_limit")]
        for payload, diagnostic in cases:
            with self.subTest(diagnostic=diagnostic, prefix=payload[:40]):
                self.rejected(payload, diagnostic)

    def test_contract_versions_fail_incompatible_not_observed(self):
        for field, value in (("contract", "djinn.session.observation.v2"), ("schema_version", 2)):
            with self.subTest(field=field):
                report = self.inspect(change(self.fixture, (field,), value))
                self.assertEqual(report["assessment"], "incompatible")
                self.assertEqual(report["diagnostics"], ["unsupported_contract"])
                self.assertIsNone(report["observation"])
        for field, value in (("schema_version", True), ("schema_version", "1"), ("contract", None)):
            with self.subTest(field=field, value=value):
                self.rejected(change(self.fixture, (field,), value))

    def test_capabilities_require_exact_boolean_keys_and_values(self):
        for key, expected in self.fixture["capabilities"].items():
            for invalid in (int(expected), not expected, None, "true"):
                with self.subTest(key=key, invalid=invalid):
                    self.rejected(change(self.fixture, ("capabilities", key), invalid))
        for caps in ({}, {**self.fixture["capabilities"], "execute": False}, None, []):
            with self.subTest(caps=caps):
                self.rejected(change(self.fixture, ("capabilities",), caps))

    def test_non_authoritative_envelope_fields_are_not_advisory(self):
        for field, values in {
            "authorization": ["allowed", None], "dispatchable": [True, 0, None],
            "runtime_fact_freshness": ["fresh"], "native_lifecycle": ["running"],
            "job_completion": ["success"], "activity": ["working"],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.rejected(change(self.fixture, (field,), value))

    def test_required_fields_folder_binding_and_producer_timestamps(self):
        for key in self.fixture:
            value = copy.deepcopy(self.fixture)
            del value[key]
            with self.subTest(missing=key):
                self.rejected(value)
        for field, values in {
            "selected_folder": ["/different", None],
            "resolved_folder": [None, "relative", "/a/../b", "/a//b", "/a/"],
            "started_at": [None, "yesterday", "2026-02-30T00:00:00Z", "2026-10-24T12:00:00+01:00"],
            "observed_at": [None, "2026-10-24T25:00:00Z", "2026-10-24T12:00:00"],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.rejected(change(self.fixture, (field,), value))

    def test_source_paths_bounds_digests_and_counts(self):
        for key in self.fixture["observation"]:
            value = copy.deepcopy(self.fixture)
            del value["observation"][key]
            with self.subTest(missing_fact=key):
                self.rejected(value)
        for source, cap in (("manifest", 65536), ("folder_events", 4194304)):
            for field, values in {"path": ["/events.jsonl", "../djinn.toml", "other"],
                                  "bytes": [-1, True, 1.5, cap + 1, None],
                                  "sha256": ["A" * 64, "a" * 63, "g" * 64, None]}.items():
                for value in values:
                    with self.subTest(source=source, field=field, value=value):
                        self.rejected(change(self.fixture, ("observation", "sources", source, field), value))
            value = change(self.fixture, ("observation", "sources", source, "bytes"), cap)
            self.assertEqual(self.inspect(value)["assessment"], "observed")
        for count in (-1, True, 1.0, 16385, None):
            with self.subTest(count=count):
                self.rejected(change(self.fixture, ("observation", "event_count"), count))
        self.assertEqual(self.inspect(change(self.fixture, ("observation", "event_count"), 16384))["assessment"], "observed")

    def test_missing_and_empty_event_streams_have_distinct_evidence(self):
        for status, count, size, digest in (("missing", None, None, None), ("observed", 0, 0, EMPTY_HASH)):
            value = copy.deepcopy(self.fixture)
            facts = value["observation"]
            facts.update(event_count=count, last_event=None, last_lifecycle=None)
            facts["sources"]["folder_events"].update(status=status, bytes=size, sha256=digest)
            with self.subTest(status=status):
                self.assertEqual(self.inspect(value)["observation"], facts)
                for field, invalid in (("event_count", 1), ("last_event", self.fixture["observation"]["last_event"]),
                                       ("last_lifecycle", self.fixture["observation"]["last_lifecycle"])):
                    self.rejected(change(value, ("observation", field), invalid))
                self.rejected(change(value, ("observation", "sources", "folder_events", "sha256"), "c" * 64))
        value = copy.deepcopy(self.fixture)
        value["observation"]["sources"]["manifest"].update(bytes=0, sha256=EMPTY_HASH)
        self.assertEqual(self.inspect(value)["assessment"], "observed")
        self.rejected(change(value, ("observation", "sources", "manifest", "sha256"), "a" * 64))

    def test_crossfield_event_and_lifecycle_consistency(self):
        cases = [(("last_lifecycle",), None), (("last_event",), None),
                 (("last_lifecycle", "event_id"), "different"),
                 (("last_lifecycle", "recorded_at"), None),
                 (("last_lifecycle", "session_id"), None),
                 (("sources", "folder_events", "status"), "unknown"),
                 (("sources", "folder_events", "bytes"), 0),
                 (("consistency",), "transactional")]
        for path, value in cases:
            with self.subTest(path=path):
                self.rejected(change(self.fixture, ("observation", *path), value))
        value = change(self.fixture, ("observation", "last_event", "kind"), "message_recorded")
        self.rejected(value)  # It cannot share the previous lifecycle record's ID.
        value["observation"]["last_event"]["event_id"] = "evt-3"
        self.assertEqual(self.inspect(value)["assessment"], "observed")
        self.rejected(change(value, ("observation", "event_count"), 1))

    def test_native_identity_mismatch_invalid_and_nullable_identities(self):
        for path in (("recorded_native_session_id",), ("last_event", "session_id"), ("last_lifecycle", "session_id")):
            for invalid in ("agt_other", "ui-123", "agt_", "agt_\u00e9", 123, "agt_a\n"):
                with self.subTest(path=path, invalid=invalid):
                    self.rejected(change(self.fixture, ("observation", *path), invalid))
        value = change(self.fixture, ("observation", "recorded_native_session_id"), None)
        self.assertEqual(self.inspect(value)["assessment"], "observed")
        for path in (("last_event", "event_id"), ("last_lifecycle", "event_id")):
            for invalid in ("", "x\x1b", "x" * 257):
                with self.subTest(path=path, invalid=invalid):
                    self.rejected(change(self.fixture, ("observation", *path), invalid))

    def test_native_unavailable_is_sanitized_and_cannot_contain_facts(self):
        value = {**self.fixture, "assessment": "unavailable", "observation": None,
                 "resolved_folder": None, "diagnostics": ["folder_missing"]}
        report = self.inspect(value)
        self.assertEqual(report["assessment"], "unavailable")
        self.assertEqual(report["diagnostics"], ["native_unavailable"])
        self.assertIsNone(report["observation"])
        for field, invalid in (("diagnostics", []), ("diagnostics", [SECRET]),
                               ("diagnostics", ["x"] * 17), ("diagnostics", "missing"),
                               ("observation", self.fixture["observation"]), ("assessment", "incompatible")):
            with self.subTest(field=field, invalid=invalid):
                self.rejected(change(value, (field,), invalid))

    def test_event_shapes_lifecycle_modes_and_recorded_timestamps(self):
        for field in ("last_event", "last_lifecycle"):
            for invalid in ([], "event", 1):
                with self.subTest(field=field, invalid=invalid):
                    self.rejected(change(self.fixture, ("observation", field), invalid))
            for key in self.fixture["observation"][field]:
                value = copy.deepcopy(self.fixture)
                del value["observation"][field][key]
                with self.subTest(field=field, missing=key):
                    self.rejected(value)
        for path, invalid in ((("last_event", "kind"), "SessionUpdated"),
                              (("last_event", "kind"), ""),
                              (("last_lifecycle", "state"), "done"),
                              (("last_lifecycle", "mode"), "daemon"),
                              (("last_event", "recorded_at"), "2026-13-24T00:00:00Z"),
                              (("last_lifecycle", "recorded_at"), "2026-10-24T00:00:00+24:00"),
                              (("recorded_created_at",), "2026-10-24T00:00:00+00:60")):
            with self.subTest(path=path, invalid=invalid):
                self.rejected(change(self.fixture, ("observation", *path), invalid))
        for mode in (None, "foreground", "background"):
            with self.subTest(mode=mode):
                value = change(self.fixture, ("observation", "last_lifecycle", "mode"), mode)
                self.assertEqual(self.inspect(value)["assessment"], "observed")
        for timestamp in ("2026-10-24T11:30:00.1234567890Z",
                          "2026-10-24T11:30:00.12345678901234567890+01:00"):
            value = copy.deepcopy(self.fixture)
            value["observation"]["recorded_created_at"] = timestamp
            for field in ("last_event", "last_lifecycle"):
                value["observation"][field]["recorded_at"] = timestamp
            report = self.inspect(value)
            self.assertEqual(report["assessment"], "observed")
            self.assertEqual(report["observation"]["last_event"]["recorded_at"], timestamp)

    def test_raw_output_errors_and_transcript_extensions_are_not_echoed(self):
        value = copy.deepcopy(self.fixture)
        for target in (value, value["observation"], value["observation"]["last_event"],
                       value["observation"]["last_lifecycle"], value["observation"]["sources"]["manifest"]):
            target["transcript"] = {"raw_stdout": SECRET, "error": SECRET}
        report = self.inspect(value)
        self.assertEqual(report["assessment"], "observed")
        self.assertEqual(report["observation"], self.fixture["observation"])
        self.assertNotIn(SECRET, json.dumps(report))
        for result in (probe_result(self.fixture, stderr=SECRET.encode()),
                       probe_result(SECRET.encode(), status="nonzero", code=9, stderr=SECRET.encode()),
                       probe_result(self.fixture, status="timeout", code=None),
                       probe_result(self.fixture, status="output_limit", code=None),
                       probe_result(self.fixture, status="spawn_error", code=None)):
            with self.subTest(status=result.status), mock.patch.object(workers, "run_session_observation_probe", return_value=result):
                report = observation.inspect(str(self.executable), self.folder)
                self.assertEqual(report["assessment"], "observed" if result.status == "ok" else "unavailable")
                self.assertNotIn(SECRET, json.dumps(report))
        with mock.patch.object(workers, "run_session_observation_probe", side_effect=OSError(SECRET)):
            report = observation.inspect(str(self.executable), self.folder)
        self.assertEqual(report["diagnostics"], ["executable_unavailable"])
        self.assertNotIn(SECRET, json.dumps(report))

    def test_probe_result_shape_and_combined_output_budget(self):
        for result in (None, {}, workers.ProbeResult([], 0, b"{}", b"", 0),
                       workers.ProbeResult("ok", False, b"{}", b"", 0),
                       workers.ProbeResult("ok", 0, "{}", b"", 0),
                       probe_result(b"x" * 1024, stderr=b"y")):
            with self.subTest(result=type(result).__name__), mock.patch.object(workers, "run_session_observation_probe", return_value=result):
                report = observation.inspect(str(self.executable), self.folder, max_output_bytes=1024)
                self.assertEqual(report["assessment"], "unavailable")
                self.assertIsNone(report["observation"])

    def test_unavailable_nonexecutables_never_probe_or_discover(self):
        self.executable.chmod(0o600)
        with mock.patch.object(workers, "run_session_observation_probe") as probe, \
                mock.patch.object(workers.shutil, "which", side_effect=AssertionError("discovery")):
            for path in (self.root / "missing", self.root, self.executable):
                with self.subTest(path=path):
                    report = observation.inspect(str(path), self.folder)
                    self.assertEqual(report["assessment"], "unavailable")
                    self.assertIsNone(report["observation"])
        probe.assert_not_called()

    def test_fingerprint_preprobe_metadata_or_alias_change_prevents_spawn(self):
        alias = self.root / "alias"
        alias.symlink_to(self.executable)
        replacement = self.root / "replacement"
        replacement.write_bytes(self.executable.read_bytes())
        replacement.chmod(0o700)
        original = workers._identity
        for kind in ("metadata", "alias"):
            def identify(path):
                identity = original(path)
                if kind == "metadata":
                    info = path.stat()
                    os.utime(path, ns=(info.st_atime_ns, info.st_mtime_ns + 1000000000))
                else:
                    alias.unlink()
                    alias.symlink_to(replacement)
                return identity
            with self.subTest(kind=kind), mock.patch.object(workers, "_identity", side_effect=identify), \
                    mock.patch.object(workers, "run_session_observation_probe") as probe:
                report = observation.inspect(str(alias), self.folder)
            self.assertEqual(report["diagnostics"], ["executable_changed"])
            probe.assert_not_called()

    def test_postprobe_metadata_content_and_alias_change_reject_producer(self):
        replacement = self.root / "replacement"
        replacement.write_bytes(self.executable.read_bytes())
        replacement.chmod(0o700)
        for kind in ("metadata", "same_metadata_content", "alias"):
            alias = self.root / ("alias-" + kind)
            alias.symlink_to(self.executable)
            def mutate(**kwargs):
                info = self.executable.stat()
                if kind == "metadata":
                    os.utime(self.executable, ns=(info.st_atime_ns, info.st_mtime_ns + 1000000000))
                elif kind == "same_metadata_content":
                    self.executable.write_bytes(b"X" * info.st_size)
                    os.utime(self.executable, ns=(info.st_atime_ns, info.st_mtime_ns))
                else:
                    alias.unlink()
                    alias.symlink_to(replacement)
                return probe_result(self.fixture)
            with self.subTest(kind=kind), mock.patch.object(workers, "run_session_observation_probe", side_effect=mutate) as probe:
                report = observation.inspect(str(alias), self.folder)
            self.assertEqual(report["diagnostics"], ["executable_changed"])
            self.assertIsNone(report["observation"])
            probe.assert_called_once()

    def test_explicit_aliases_resolve_without_cached_positive_after_failure(self):
        alias = self.root / "alias"
        alias.symlink_to(self.executable)
        with mock.patch.object(workers, "run_session_observation_probe", side_effect=[probe_result(self.fixture), probe_result(b"bad")]) as probe:
            first = observation.inspect(str(alias), self.folder)
            second = observation.inspect(str(alias), self.folder)
        self.assertEqual(first["assessment"], "observed")
        self.assertEqual(first["executable"]["selected_path"], str(alias))
        self.assertEqual(first["executable"]["resolved_path"], str(self.executable))
        self.assertEqual(first["executable"]["identity"]["sha256"], hashlib.sha256(self.executable.read_bytes()).hexdigest())
        self.assertEqual(second["assessment"], "unavailable")
        self.assertIsNone(second["observation"])
        self.assertEqual(probe.call_count, 2)
        self.assertTrue(all(c.kwargs["executable"] == self.executable for c in probe.call_args_list))


class DjinnTransportAndCommandTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.folder = self.root / "folder with spaces \u00e9"
        self.folder.mkdir()
        self.fixture = native_fixture(str(self.folder))
        self.executable = self.root / "private producer"
        self.record = self.root / "producer-record.json"

    def script(self, body=None, value=None):
        if body is None:
            payload = json.dumps(self.fixture if value is None else value).encode()
            body = ("import json\n"
                    f"assert sys.argv[1:] == ['session', 'observe', {str(self.folder)!r}, '--json']\n"
                    "assert sys.stdin.buffer.read() == b''\n"
                    f"with open({str(self.record)!r}, 'w') as stream:\n"
                    "    json.dump({'argv': sys.argv, 'env': dict(os.environ), 'cwd': os.getcwd()}, stream)\n"
                    f"os.write(1, {payload!r})\n")
        self.executable.write_text(f"#!{sys.executable}\nimport os, sys, time\n" + body, encoding="utf-8")
        self.executable.chmod(0o700)
        return self.executable

    def probe(self, **kwargs):
        return workers.run_session_observation_probe(self.executable, str(self.folder),
            cwd="/", env={}, timeout=kwargs.get("timeout", 0.5), max_output_bytes=kwargs.get("limit", 1024))

    def command(self, *args, expected=0):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = coven.main(["observe", "djinn", "--executable", str(self.executable),
                               "--folder", str(self.folder), *args])
        self.assertEqual(code, expected)
        return json.loads(output.getvalue()), output.getvalue()

    def test_real_producer_fixed_argv_stdin_environment_and_no_shell(self):
        self.script()
        denied = {key: SECRET for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY",
                  "AWS_SECRET_ACCESS_KEY", "PROVIDER_TOKEN", "DJINN_CONFIG", "DJINN_HOME",
                  "XDG_CONFIG_HOME", "KITSUNE_SOCKET_PATH", "KITSUNE_CLIENT_SOCKET_PATH",
                  "COVEN_HOME", "COVEN_COMMAND", "COVEN_WORKSPACE", "PYTHONPATH")}
        allowed = {"PATH": "/not-a-native-discovery-path", "HOME": str(self.root),
                   "SHELL": "/bin/false", "LANG": "C", "LC_ALL": "C", "TERM": "dumb"}
        real_popen = workers.subprocess.Popen
        with mock.patch.dict(os.environ, {**allowed, **denied}, clear=True), \
                mock.patch.object(workers.subprocess, "Popen", wraps=real_popen) as popen:
            report = observation.inspect(str(self.executable), str(self.folder), timeout=2)
        self.assertEqual(report["assessment"], "observed")
        recorded = json.loads(self.record.read_text())
        self.assertEqual(recorded["argv"], [str(self.executable), "session", "observe", str(self.folder), "--json"])
        self.assertEqual(recorded["cwd"], "/")
        self.assertFalse(set(recorded["env"]) & set(denied))
        for key, value in allowed.items():
            self.assertEqual(recorded["env"][key], value)
        popen.assert_called_once()
        args, options = popen.call_args
        self.assertEqual(args[0][:4], [sys.executable, "-I", "-S", "-c"])
        self.assertEqual(args[0][-5:], recorded["argv"])
        self.assertEqual(options["stdin"], subprocess.DEVNULL)
        self.assertTrue(options["close_fds"])
        self.assertTrue(options["start_new_session"])
        self.assertNotIn("shell", options)
        self.assertEqual(options["env"], allowed)
        self.assertNotIn(SECRET, json.dumps(report))

    def test_fixed_wrapper_rejects_invalid_folders_and_extra_controls_before_spawn(self):
        with mock.patch.object(workers.subprocess, "Popen") as popen:
            for folder in (None, "relative", "--extra", "/", "/a/", "/a/../b", "/a\nb", "/\ud800"):
                with self.subTest(folder=folder), self.assertRaises(ValueError):
                    workers.run_session_observation_probe(self.executable, folder, timeout=1,
                                                         max_output_bytes=1024, cwd="/", env={})
            for extra in ({"arguments": ("--extra",)}, {"shell": True}):
                with self.subTest(extra=extra), self.assertRaises(TypeError):
                    workers.run_session_observation_probe(self.executable, str(self.folder), timeout=1,
                                                         max_output_bytes=1024, cwd="/", env={}, **extra)
        popen.assert_not_called()
        self.assertEqual(workers.HELP_PROBE_ALLOWLIST, frozenset({
            ("--help",), ("--version",), ("session", "--help"), ("session", "run", "--help"),
            ("session", "status", "--help"), ("session", "watch", "--help"),
            ("ask", "--help"), ("agents", "--help")}))
        with mock.patch.object(workers.subprocess, "Popen") as popen, self.assertRaises(ValueError):
            workers.run_help_probe(self.executable, ("session", "observe", str(self.folder), "--json"),
                                   timeout=1, max_output_bytes=1024, cwd="/", env={})
        popen.assert_not_called()

    def test_real_wrapper_timeout_output_limits_and_spawn_failure(self):
        self.script("os.close(1); os.close(2); time.sleep(10)\n")
        result = self.probe(timeout=0.15)
        self.assertEqual(result.status, "timeout")
        self.assertLess(result.duration_ms, 2000)
        for body, expected in (("os.write(1, b'x'*512); os.write(2, b'y'*512)\n", "ok"),
                               ("os.write(1, b'x'*600); os.write(2, b'y'*600)\n", "output_limit"),
                               ("os.write(2, b'x'*1000000)\n", "output_limit")):
            with self.subTest(expected=expected):
                self.script(body)
                result = self.probe(timeout=1)
                self.assertEqual(result.status, expected)
                self.assertEqual(len(result.stdout) + len(result.stderr), 1024)
        self.executable.unlink()
        self.assertEqual(self.probe(timeout=1).status, "spawn_error")

    @unittest.skipUnless(hasattr(os, "fork"), "POSIX descendant cleanup")
    def test_fixed_wrapper_kills_descendants_with_open_or_closed_pipes(self):
        for closed in (False, True):
            with self.subTest(closed=closed):
                marker = self.root / ("late-" + str(closed))
                self.script("pid = os.fork()\nif pid == 0:\n"
                            + ("    os.close(1); os.close(2)\n" if closed else "")
                            + f"    time.sleep(0.8)\n    open({str(marker)!r}, 'w').close()\n    os._exit(0)\n"
                            + "os.write(1, b'done'); os._exit(0)\n")
                result = self.probe(timeout=0.2)
                self.assertEqual(result.status, "ok" if closed else "timeout")
                time.sleep(0.9)
                self.assertFalse(marker.exists())

    def test_cli_required_explicit_selectors_and_rejects_control_arguments(self):
        cases = [[], ["--executable", str(self.executable)], ["--folder", str(self.folder)]]
        base = ["--executable", str(self.executable), "--folder", str(self.folder)]
        cases += [base + extra for extra in (["--extra", "status"], ["--workspace", self.root.as_posix()],
                                             ["--method", "session.run"], ["--shell"], ["--arguments=status"])]
        with mock.patch.object(observation, "inspect", side_effect=AssertionError("implicit dispatch")) as inspect, \
                mock.patch.dict(os.environ, {"DJINN_FOLDER": str(self.folder), "DJINN_EXECUTABLE": str(self.executable)}):
            for args in cases:
                with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                    coven.main(["observe", "djinn", *args])
                self.assertEqual(raised.exception.code, 2)
        inspect.assert_not_called()

    def test_cli_exit_codes_ascii_output_and_invalid_arguments_before_io(self):
        for assessment, code in (("observed", 0), ("unavailable", 1), ("incompatible", 2)):
            for compact in (False, True):
                with self.subTest(assessment=assessment, compact=compact):
                    value = copy.deepcopy(self.fixture)
                    if assessment == "unavailable":
                        value.update(assessment="unavailable", observation=None, diagnostics=["folder_missing"])
                    elif assessment == "incompatible":
                        value["contract"] = "djinn.session.observation.v2"
                    self.script(value=value)
                    # Exercise command mapping without another real subprocess per format.
                    with mock.patch.object(workers, "run_session_observation_probe", return_value=probe_result(value)):
                        report, output = self.command(*(["--json"] if compact else []), expected=code)
                    self.assertEqual(report["assessment"], assessment)
                    self.assertTrue(output.isascii())
                    self.assertIn("\\u00e9", output)
                    self.assertEqual("\n" in output.rstrip("\n"), not compact)
        for options in (("--timeout", "nan"), ("--timeout", "0"), ("--timeout", "31"),
                        ("--max-output-bytes", "1"), ("--folder", "relative"), ("--executable", "djinn")):
            with self.subTest(options=options), mock.patch.object(Path, "resolve", side_effect=AssertionError("I/O")), \
                    mock.patch.object(workers.subprocess, "Popen") as popen, contextlib.redirect_stderr(io.StringIO()), \
                    self.assertRaises(SystemExit) as raised:
                coven.main(["observe", "djinn", "--executable", str(self.executable),
                            "--folder", str(self.folder), *options])
            self.assertEqual(raised.exception.code, 1)
            popen.assert_not_called()

    def test_actual_bin_corrupt_workspace_independence_and_explicit_folder_alias(self):
        workspace = self.root / "corrupt-workspace"
        workspace.mkdir()
        files = ("coven.json", "state/tasks.json", "state/agents.json", "state/task-ledger.json",
                 "state/task-authority.json", "state/task-ledger/manifest.json",
                 "state/task-ledger/frames/00000000000000000000.frame",
                 "state/execution/control.json", "state/execution.json", "state/task-attempts.json", "state/groups.json",
                 "state/runtime-launch-control.json", "state/repository.json")
        for relative in files:
            path = workspace / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"corrupt\xff\x00" + relative.encode())
        def snapshot():
            return {str(p.relative_to(workspace)): p.read_bytes() if p.is_file() else None
                    for p in workspace.rglob("*")}
        before = snapshot()
        alias = self.root / "folder alias"
        alias.symlink_to(self.folder, target_is_directory=True)
        env = {key: value for key, value in os.environ.items()
               if key not in {"COVEN_HOME", "COVEN_COMMAND", "PYTHONPATH"}}
        env.update(PYTHONDONTWRITEBYTECODE="1", KITSUNE_SOCKET_PATH="/never-use",
                   DJINN_CONFIG="/never-use", COVEN_WORKSPACE=str(workspace))
        for assessment, code in (("observed", 0), ("unavailable", 1), ("incompatible", 2)):
            value = native_fixture(str(alias))
            value["resolved_folder"] = str(self.folder)
            if assessment == "unavailable":
                value.update(assessment="unavailable", observation=None, diagnostics=["folder_missing"])
            elif assessment == "incompatible":
                value["schema_version"] = 2
            # The fake enforces the selected alias rather than discovering a capsule.
            payload = json.dumps(value).encode()
            self.script(f"assert sys.argv[1:] == ['session', 'observe', {str(alias)!r}, '--json']\n"
                        "assert sys.stdin.buffer.read() == b''\n"
                        "assert not any(k.startswith(('COVEN_', 'KITSUNE_', 'DJINN_')) for k in os.environ)\n"
                        f"os.write(1, {payload!r})\n")
            with self.subTest(assessment=assessment):
                response = subprocess.run([str(ROOT / "bin/coven"), "observe", "djinn", "--executable",
                    str(self.executable), "--folder", str(alias), "--json"], cwd=workspace,
                    env=env, capture_output=True, timeout=10)
                self.assertEqual(response.returncode, code, response.stderr.decode(errors="replace"))
                report = json.loads(response.stdout)
                self.assertEqual(report["assessment"], assessment)
                self.assertTrue(response.stdout.isascii())
                self.assertEqual(response.stderr, b"")
                self.assertEqual(snapshot(), before)
        self.script()
        with contextlib.chdir(workspace), \
                mock.patch.object(coven, "load_team", side_effect=AssertionError("workspace load")), \
                mock.patch.object(storage, "workspace_lock", side_effect=AssertionError("workspace lock")), \
                mock.patch.object(storage, "transport_gate", side_effect=AssertionError("transport gate")):
            self.command("--json")
        self.assertEqual(snapshot(), before)


if __name__ == "__main__":
    unittest.main()
