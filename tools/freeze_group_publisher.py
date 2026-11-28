#!/usr/bin/env python3
"""Capture both publisher engine consumers from an explicit pinned Git export."""
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
HELPER = "src/group_publisher/oracle.py"
PREFIXES = ["runner", "src/group_publisher", "src/group_publisher.rs", "src/groups",
            "src/groups.rs", "src/kitsune_observation", "src/kitsune_observation.rs",
            "src/observation_socket", "src/observation_socket.rs", "src/observation_time.rs",
            "src/task_time.rs", "src/values", "src/values.rs"]
MODES = ["success", "reservation_error", "reservation_uncertain", "invalid_reservation",
         "reject", "lost_ack", "unsent", "unknown"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2,
                       allow_nan=False) + "\n").encode("ascii")


def compact(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def framed(streams):
    # Length prefixes frame the complete raw streams, without delimiter ambiguity.
    return b"".join(len(raw).to_bytes(8, "big") + raw for raw in streams)


def inventory(project):
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                     *PREFIXES], cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith((".rs", ".py")))
    assert len(paths) == 53
    assert len([p for p in paths if p.startswith("runner/")]) == 25
    assert all(p in paths for p in [HELPER, "src/group_publisher/tests.rs",
                                   "src/group_publisher/live_tests.rs",
                                   "src/group_publisher/model.rs", "src/group_publisher/engine.rs"])
    return paths


def verify_sources(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    pins = {}
    for path in inventory(project):
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in pins if p.startswith("runner/")]
    assert not list(root.rglob("*.pyc"))
    return pins


def request(live, mode):
    """Independent transcription of Fixture::new(2, live), status and unknown edit."""
    members = []
    panes = []
    participants = []
    for i in range(2):
        attachment = dict(adapter="djinn_live", source_contract="djinn.live-observation.v1",
                          runtime_id="runtime", instance_id="instance", session_id="session",
                          artifact_path="/unused/runtime.json", attached_at="2026-10-23T00:00:00+00:00")
        member = dict(participant_id=f"participant-{i}", key=f"member-{i}", role=f"Role {i}",
                      binding=dict(presentation=dict(adapter="kitsune", endpoint=dict(path="/unused/socket", device=1, inode=2),
                                                     terminal_id=f"terminal-{i}", pane_id_at_binding=f"pane-{i}"),
                                   evidence=dict(provenance="kitsune.session.snapshot", observed_at="2026-10-23T00:00:00+00:00",
                                                 agent_kind_hint="test-agent", native_reference=None, native_binding="unestablished")))
        if live:
            member["live_attachment"] = copy.deepcopy(attachment)
        members.append(member)
        panes.append(dict(pane_id=f"pane-{i}", terminal_id=f"terminal-{i}", workspace_id="w", tab_id="t",
                          agent="test-agent", agent_session=None))
        participants.append(dict(participant_id=f"participant-{i}", key=f"member-{i}", role=f"Role {i}",
                                 live_attachment=attachment if live else None,
                                 association_basis="explicit_declaration" if live else "unattached",
                                 assessment="observed", activity="unknown" if mode == "unknown" else "busy",
                                 activity_scope="legacy_runner_ownership", freshness="fresh_sample", expires_in_ms=3000,
                                 source=dict(contract="djinn.live-observation.v1", runtime_id="runtime", instance_id="instance",
                                             session_id="session", sequence=1, sampled_at_ms=100000, valid_for_ms=3000,
                                             source="legacy_session_run_state", activity_scope="legacy_runner_ownership",
                                             capabilities=dict(activity=True, idle=False, catalog_complete=False,
                                                               decision_state=False, retry_state=False, durable_lifecycle=False,
                                                               job_completion=False)),
                                 authorization="none", dispatchable=False, native_lifecycle="unestablished", job_completion="unestablished"))
    identity = dict(group_id="group-id", key="team", name="Review Team")
    return dict(mode=mode, live=live, group=dict(**identity, members=members),
                fresh=dict(endpoint=dict(path="/unused/socket", device=1, inode=2), epoch="epoch-1", sources=["coven"],
                           valid_until_unix_ms=True, panes=panes, agents=copy.deepcopy(panes)),
                status=dict(schema_version=1, contract="coven.group.status.v1", assessment="observed", revision="revision",
                            group=identity, authorization="none", dispatchable=False, native_lifecycle="unestablished",
                            job_completion="unestablished", participants=participants))


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
    pins = verify_sources(project, root)  # All pins verified before helper execution.
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8", TMPDIR=os.environ["TMPDIR"])
    rows = []
    for live in [False, True]:
        for mode in MODES:
            stdin = compact(request(live, mode))
            captures = []
            for _ in range(2):
                with tempfile.TemporaryDirectory(prefix="publisher-capture-", dir=environment["TMPDIR"]) as cwd:
                    captures.append(subprocess.check_output([sys.executable, "-I", "-B", str(root / HELPER)],
                                                            input=stdin, cwd=cwd, env=environment))
            assert captures[0] == captures[1]
            stdout = captures[0]
            stdout.decode("utf-8", errors="strict")
            rows.append(dict(id=f"{'live' if live else 'static'}-{mode}", live=live, mode=mode,
                             input_utf8=stdin.decode("utf-8"), input_sha256=digest(stdin),
                             stdout=stdout.decode("utf-8"), stdout_sha256=digest(stdout)))
    assert len(rows) == len({r["id"] for r in rows}) == 16
    corpus = encoded(dict(schema_version=1, cases=rows))
    streams = {}
    for name, field in [("stdin", "input_utf8"), ("stdout", "stdout")]:
        raw = framed([r[field].encode("utf-8") for r in rows])
        streams[name + "_stream_hex"] = raw.hex()
        streams[name + "_stream_bytes"] = len(raw)
        streams[name + "_stream_sha256"] = digest(raw)
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                         interpreter=dict(version=sys.version, executable=sys.executable,
                                          sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                         capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(corpus),
                         case_count=16, repeat_capture_equal=True, helper_invocations=32, dynamic_bindings=0,
                         matrix=dict(live=[False, True], modes=MODES),
                         environment={**environment, "TMPDIR": "<owned output directory>/tmp"},
                         stream_framing="Ordered records: 8-byte unsigned big-endian raw byte length followed by full raw bytes; hex transport only.",
                         construction="Independent literal Fixture/new/group/attach/status/source transcription and explicit 2x8 matrix. Original two members, clock, controlled effects, reserve and send fault distinctions, prepare twice retained. No native outputs, bindings, normalization, deduplication, sampling or fallback.",
                         **streams))
    for name, raw in [("group_publisher_engine.json", corpus), ("group_publisher_engine_provenance.json", proof)]:
        (output / name).write_bytes(raw)
        print(json.dumps(dict(path=name, bytes=len(raw), sha256=digest(raw))))


if __name__ == "__main__":
    main()
