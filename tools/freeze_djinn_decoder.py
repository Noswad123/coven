#!/usr/bin/env python3
"""Capture the historical pure Djinn decoder, never inspect or probe a producer."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PIN = "727edc7aad55ed76e98670e0cee6449d611b2285"
HELPER = "src/djinn_observation/oracle.py"
SOURCES = [HELPER, "src/djinn_observation/tests.rs", "src/djinn_observation.rs",
           "src/djinn_observation/schema.rs", "src/observation_wire.rs"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2,
                       allow_nan=False) + "\n").encode("ascii")


def rust_json(value):
    # serde_json's default sorted object keys, compact separators, literal UTF-8.
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False)


def requests():
    """Independent literal/matrix transcription; no native decoder/output inputs."""
    base = {
        "schema_version": 1, "contract": "djinn.session.observation.v1",
        "selected_folder": "/selected", "resolved_folder": "/persisted/folder",
        "started_at": "2026-10-24T12:00:00Z", "observed_at": "2026-10-24T12:00:01+00:00",
        "assessment": "observed", "diagnostics": [],
        "capabilities": {"persisted_folder_events": True, "live_status": False,
                         "ui_sessions": False, "native_store": False},
        "authorization": "none", "dispatchable": False, "runtime_fact_freshness": "unknown",
        "native_lifecycle": "unestablished", "job_completion": "unestablished", "activity": "unknown",
        "observation": {
            "recorded_native_session_id": "agt_test_123", "recorded_created_at": "2026-10-24T11:00:00Z",
            "sources": {"manifest": {"path": "djinn.toml", "bytes": 150, "sha256": "a" * 64},
                        "folder_events": {"path": "events.jsonl", "status": "observed",
                                          "bytes": 400, "sha256": "b" * 64}},
            "event_count": 2,
            "last_event": {"kind": "session_lifecycle_updated", "event_id": "evt-2",
                           "session_id": "agt_test_123", "recorded_at": "2026-10-24T11:30:00Z"},
            "last_lifecycle": {"state": "completed", "mode": "background", "event_id": "evt-2",
                               "session_id": "agt_test_123", "recorded_at": "2026-10-24T11:30:00Z"},
            "consistency": "stable_per_file_not_transactional"}}

    def at(value, pointer):
        for key in pointer.split("/")[1:]:
            value = value[key]
        return value

    cases = [copy.deepcopy(base)]
    for pointer in ["", "/observation", "/observation/sources/manifest",
                    "/observation/sources/folder_events", "/observation/last_event",
                    "/observation/last_lifecycle"]:
        for key in sorted(at(base, pointer)):
            value = copy.deepcopy(base)
            del at(value, pointer)[key]
            cases.append(value)
    assert len(cases) == 40
    for pointer, values in [
        ("/schema_version", [True, "1", 1.0, 2, None]),
        ("/contract", ["unknown", None]), ("/selected_folder", ["/other", None]),
        ("/resolved_folder", ["/a//b", "/a/./b", "///a", "//a/b", "/", "relative", None]),
        ("/started_at", ["2026-02-30T12:00:00Z", "2026-01-01t00:00:60.12345678901234567890z",
                         "0000-02-29T00:00:00+00:00", "2026-01-01T00:00:00-00:00", "2026-01-01T00:00:00+01:00"]),
        ("/dispatchable", [0, True, None]), ("/diagnostics", [["x"], ["X"], ["x"] * 17, None]),
        ("/observation/event_count", [-1, True, 1.0, 16385, 16384, 1, 0, None]),
        ("/observation/last_lifecycle/state", ["done", "created", "running", "paused", "failed", "cancelled"]),
        ("/observation/last_lifecycle/mode", ["foreground", "daemon", None]),
        ("/observation/last_event/event_id", ["", "x\u0085", "x" * 257, "different", None]),
        ("/observation/recorded_native_session_id", ["agt_other", "agt_", "agt_\u00e9", None]),
        ("/observation/recorded_created_at", ["2026-02-29T00:00:00Z", "2024-02-29T23:59:60.1234567890123456789-23:59", None]),
        ("/observation/last_event/kind", ["message_recorded", "Session", ""]),
        ("/observation/consistency", ["transactional", None]),
    ]:
        parent, key = pointer.rsplit("/", 1)
        for replacement in values:
            value = copy.deepcopy(base)
            at(value, parent)[key] = replacement
            cases.append(value)
    assert len(cases) == 102
    for key, boolean in [("persisted_folder_events", True), ("live_status", False),
                         ("ui_sessions", False), ("native_store", False)]:
        for replacement in [int(boolean), not boolean, None, "true"]:
            value = copy.deepcopy(base)
            value["capabilities"][key] = replacement
            cases.append(value)
    for key, cap in [("manifest", 65536), ("folder_events", 4194304)]:
        for field, values in [("bytes", [-1, True, cap + 1, cap, 0]),
                              ("sha256", ["A" * 64, "g" * 64, "a" * 63, None]),
                              ("path", ["/events.jsonl", "../djinn.toml"])]:
            for replacement in values:
                value = copy.deepcopy(base)
                value["observation"]["sources"][key][field] = replacement
                cases.append(value)
    assert len(cases) == 140
    for status in ["missing", "observed"]:
        value = copy.deepcopy(base)
        facts = value["observation"]
        facts.update(event_count=None if status == "missing" else 0,
                     last_event=None, last_lifecycle=None)
        facts["sources"]["folder_events"].update(
            status=status, bytes=None if status == "missing" else 0,
            sha256=None if status == "missing" else hashlib.sha256(b"").hexdigest())
        cases.append(value)
    value = copy.deepcopy(base)
    value.update(assessment="unavailable", observation=None, resolved_folder=None,
                 diagnostics=["folder_missing"])
    cases.append(value)
    payloads = [rust_json(value) for value in cases]
    payloads.extend(["", "{} {}", "[]", "null", '{"x":1,"x":2}',
                     '{"extension":{"x":1,"x":2}}', '{"x":NaN}', '{"x":Infinity}',
                     '{"x":-Infinity}', '{"x":1e999}', '{"x":1e999,"text":"NaN"}'])
    payloads.append("[" * 33 + "0" + "]" * 33)
    for extension in [[], "x" * 4097, [0] * 4097,
                      {"path": "\u00e9" * 8192}, {"path": "\u00e9" * 8193}]:
        value = copy.deepcopy(base)
        value["extension"] = extension
        payloads.append(rust_json(value))
    payloads.append('{"extension":' + "9" * 4300 + "," + rust_json(base)[1:])
    assert len(payloads) == 161
    return [dict(payload=payload, folder="/selected") for payload in payloads]


def verify_sources(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    assert subprocess.check_output(["git", "rev-parse", f"{PIN}^{{commit}}"], cwd=project).decode().strip() == PIN
    modules = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, "runner"], cwd=project).decode().splitlines()
    modules = [path for path in modules if path.endswith(".py")]
    assert len(modules) == 25
    pins = {}
    for path in [*SOURCES, *modules]:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert sorted(str(path.relative_to(root)) for path in (root / "runner").rglob("*.py")) == sorted(modules)
    assert not list(root.rglob("*.pyc"))
    return pins


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    assert output.is_dir() and not output.is_relative_to(project)
    assert Path(os.environ["TMPDIR"]).resolve(strict=True).is_relative_to(output)
    pins = verify_sources(project, root)
    inputs = requests()
    stdin = encoded(inputs)
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
                   "TMPDIR": os.environ["TMPDIR"]}
    # The unmodified helper accepts a request file, not stdin. Its exact file
    # bytes are the recorded request stream; no payload itself is reparsed here.
    with tempfile.TemporaryDirectory(prefix="djinn-decoder-", dir=environment["TMPDIR"]) as cwd:
        request = Path(cwd) / "requests.json"
        request.write_bytes(stdin)
        captures = [subprocess.check_output([sys.executable, "-I", "-B", str(root / HELPER), str(request)],
                                           cwd=cwd, env=environment) for _ in range(2)]
    raw = captures[0]
    assert captures[1] == raw and raw.endswith(b"\n")
    results = json.loads(raw)
    assert len(results) == len(inputs) == 161
    tally = {}
    rows = []
    for index in range(161):
        result = results[index]
        if "error" in result:
            assert set(result) == {"error"} and isinstance(result["error"], str)
            outcome = result["error"]
        else:
            assert set(result) == {"assessment", "observation", "producer"}
            assert result["assessment"] in {"observed", "unavailable"}
            assert isinstance(result["producer"], dict)
            assert isinstance(result["observation"], dict) if result["assessment"] == "observed" else result["observation"] is None
            # Only bounded projected numeric fields reach serde_json in Rust.
            def bounded(value):
                if isinstance(value, dict):
                    for item in value.values():
                        bounded(item)
                elif isinstance(value, list):
                    for item in value:
                        bounded(item)
                elif type(value) in (int, float):
                    assert type(value) is int and 0 <= value <= 4194304
            bounded(result)
            outcome = result["assessment"]
        tally[outcome] = tally.get(outcome, 0) + 1
        rows.append(dict(id=index, **inputs[index], payload_sha256=digest(inputs[index]["payload"].encode()), expected=result))
    corpus = encoded(dict(schema_version=1, cases=rows))
    proof = encoded(dict(
        schema_version=1, historical_commit=PIN, source_sha256=pins,
        interpreter=dict(version=sys.version, executable=sys.executable,
                         sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
        capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(corpus),
        request_bytes=len(stdin), request_sha256=digest(stdin), request_utf8=stdin.decode("ascii"),
        stdin_bytes=0, stdin_note="Historical helper reads the recorded exact request file, not stdin.",
        stdout_bytes=len(raw), stdout_sha256=digest(raw), stdout_utf8=raw.decode("ascii"),
        repeat_capture_equal=True, case_count=161, outcome_counts=tally,
        environment={**environment, "TMPDIR": "<owned output directory>/tmp"},
        construction="Independent literal/matrix; Rust asserts every payload byte and folder before decoding. IDs 0..160 are indices, not unique payloads. No deduplication, normalization, sampling, case filter, bindings or native output.",
        profile="Depth 32 accepted / 33 refused; 4096 string characters/container entries; path strings 8192 characters, not UTF-8 bytes. 4300-digit raw unknown integer validated then discarded unprojected; never serde-decoded as input. Invalid UTF-8 byte FF is a separate native test, not these 161 UTF-8 payloads.",
        execution="Unmodified historical helper calls only adapter._decode; all 25 runner source modules plus helper/caller/production/schema/wire verified before import. Full Git export without bytecode. -B prevents writes; this is transient-cache policy, not a no-cache interpreter claim. No inspect/CLI/Djinn/provider calls."))
    for name, data in [("djinn_decoder.json", corpus), ("djinn_decoder_provenance.json", proof)]:
        (output / name).write_bytes(data)
        print(json.dumps(dict(path=name, bytes=len(data), sha256=digest(data))))
    print(json.dumps(tally, sort_keys=True))


if __name__ == "__main__":
    main()
