#!/usr/bin/env python3
"""Optional capture; requires an explicit, unmodified git archive of PIN."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
from unittest.mock import patch

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
PREFIXES = ["runner", "src/group_live.rs", "src/group_live", "src/groups.rs", "src/groups",
            "src/djinn_live_observation.rs", "src/djinn_live_observation", "src/kitsune_observation.rs",
            "src/kitsune_observation", "src/values", "src/observation_wire.rs", "src/observation_wire",
            "src/observation_socket.rs", "src/observation_socket", "src/observation_time.rs",
            "src/platform", "src/storage.rs", "src/storage", "src/task_time.rs",
            "src/error.rs", "src/read_fs.rs", "src/strict_json.rs", "src/unicode.rs",
            "src/private_access.rs", "src/canonical.rs"]
RUNTIME = "616fed3e-ae7f-41a0-99e5-80546d659d76"
INSTANCE = "e3c0e2ce-544c-4f1e-a489-5d3e0cf1978e"
IDS = [f"{i:08d}-1111-4111-8111-111111111111" for i in range(1, 5)]
STAMP = "2026-10-24T12:00:00+00:00"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode()


def compact(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True,
                      separators=(",", ":")).encode()


def verify(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, *PREFIXES],
                                    cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith((".rs", ".py", ".c", ".h")))
    assert len([p for p in paths if p.startswith("runner/")]) == 25
    for caller in ["group_live", "djinn_live_observation", "kitsune_observation"]:
        assert f"src/{caller}/tests.rs" in paths
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in paths if p.startswith("runner/")]
    assert not list(root.rglob("*.pyc"))
    return pins


def source():
    return dict(schema_version=1, contract="djinn.live-observation.v1", runtime_id=RUNTIME,
                sequence=1, sampled_at_ms=1792861200000, valid_for_ms=3000,
                source="legacy_session_run_state", activity_scope="legacy_runner_ownership",
                capabilities=dict(activity=True, idle=False, catalog_complete=False,
                                  decision_state=False, retry_state=False, durable_lifecycle=False,
                                  job_completion=False), authorization="none", dispatchable=False,
                native_lifecycle="unestablished", job_completion="unestablished",
                instances=[dict(instance_id=INSTANCE, sessions=[dict(session_id="selected", activity="busy")])])


def kitsune_cases():
    mapping = dict(pane_id="opaque:p", terminal_id="terminal", workspace_id="workspace", tab_id="tab")
    generic = dict(version="test", protocol=18, panes=[mapping])
    agent = dict(mapping, focused=False, agent_status="done", revision=4, agent="djinn",
                 agent_session=dict(source="native", agent="djinn", kind="id", value="private-id"))
    strict = dict(version="test", protocol=18, panes=[agent], agents=[agent], workspaces=[], tabs=[], layouts=[])
    cases = []
    for original in [strict, generic]:
        for malformed in [False, True]:
            s = json.loads(json.dumps(original))
            if malformed:
                s.update(agents="malformed", annotations=None)
            for mode in [False, True]:
                cases.append(dict(kind="snapshot", generic=mode, result=dict(type="session_snapshot", snapshot=s)))
    for argv in [["/a/djinn", "secret"], ["djinn-ui"], ["node", "djinn"], ["djinn/"], ["DJINN"], None, ["djinn", "\0"]]:
        cases.append(dict(kind="process", result=dict(type="pane_process_info", process_info=dict(
            pane_id="opaque:p", foreground_processes=[dict(pid=7, name="unused", argv=argv)]))))
    bad = json.loads(json.dumps(strict))
    bad["agents"][0]["revision"] = 1.0
    cases.append(dict(kind="snapshot", generic=False, result=dict(type="session_snapshot", snapshot=bad)))
    assert len(cases) == 16
    return cases


def capture(root, output):
    # Only this worker imports historical code, after the parent has checked every pin.
    sys.path.insert(0, str(root))
    from runner import groups, group_live, group_publisher
    from runner import djinn_live_observation as live
    from runner import kitsune_observation as o, kitsune_process_observation as p
    import socket
    import uuid

    class Clock:
        def monotonic(self): return 0
        def time(self): return 1792861200.1

    class FixedDateTime(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return cls(2026, 10, 24, 12, 0, 0, tzinfo=datetime.timezone.utc)

    rows = []

    def row(name, request, result, raw=None):
        stdin = compact(request)
        stdout = (json.dumps(result, ensure_ascii=False, allow_nan=False, indent=2) + "\n").encode()
        value = dict(id=name, input_utf8=stdin.decode(), input_sha256=digest(stdin),
                     stdout_utf8=stdout.decode(), stdout_sha256=digest(stdout))
        if raw is not None:
            state = json.loads(raw)
            canonical = compact({k: state[k] for k in ["workspace_id", "content_revision", "groups"]})
            revision = f"groups-v1:{state['workspace_id']}:{state['content_revision']}:{digest(b'coven.groups.content:v1\0' + canonical)}"
            assert groups.read_groups(workspace)["revision"] == revision
            value.update(raw_utf8=raw.decode(), raw_sha256=digest(raw),
                         canonical_utf8=canonical.decode(), canonical_sha256=digest(canonical), revision=revision)
        rows.append(value)

    directory = output / "capture"
    directory.mkdir(exist_ok=True, mode=0o700)
    artifact = directory / f"{RUNTIME}.json"
    artifact.touch(mode=0o600, exist_ok=True)
    with patch.object(live, "time", Clock()), patch.object(group_live, "time", Clock()), patch.object(group_live.datetime, "datetime", FixedDateTime):
        cases = []
        for activity in ["busy", "unknown", "idle"]:
            value = source()
            value["instances"][0]["sessions"][0]["activity"] = activity
            cases.append(value)
        for key, value in [("schema_version", 2), ("schema_version", True), ("schema_version", 1.0),
                           ("contract", "future"), ("contract", None), ("runtime_id", INSTANCE),
                           ("sampled_at_ms", 1792861200101), ("sampled_at_ms", 1792861197100),
                           ("sequence", True), ("sequence", 9007199254740992), ("instances", [])]:
            case = source()
            case[key] = value
            cases.append(case)
        case = source()
        case["instances"][0]["sessions"] = []
        cases.append(case)
        case = source()
        case["instances"][0]["sessions"].append(dict(session_id="other", activity="idle"))
        cases.append(case)
        assert len(cases) == 16
        for i, value in enumerate(cases):
            artifact.write_bytes(compact(value))
            artifact.chmod(0o600)
            row(f"djinn-{i:02}", dict(value=value, mode=0o600), live.inspect(str(artifact), RUNTIME, INSTANCE, "selected"))
        for mode in [0o600, 0o640]:
            artifact.write_bytes(b"{}")
            artifact.chmod(mode)
            result = live.inspect(str(artifact), RUNTIME, INSTANCE, "selected")
            row(f"djinn-mode-{mode:o}", dict(value={}, mode=mode), result)
        artifact.chmod(0o600)
        for huge in [False, True]:
            workspace = directory / ("huge" if huge else "normal")
            workspace.mkdir(exist_ok=True)
            (workspace / "state").mkdir(exist_ok=True)
            (workspace / "coven.json").write_bytes(b"deliberately not parsed")
            state_path = workspace / "state/groups.json"
            state_path.unlink(missing_ok=True)
            ids = iter(IDS)
            with patch.object(groups.uuid, "uuid4", lambda: uuid.UUID(next(ids))):
                groups.create_group(workspace, "review", "Review")
                endpoint = dict(path="/tmp/unused.sock", device=int("1234567890" * 10) if huge else 1,
                                inode=int("1234567890" * 10) if huge else 2)
                binding = group_publisher.binding_from_snapshot(dict(endpoint=endpoint, observed_at=STAMP,
                    agents=[dict(pane_id="pane", terminal_id="terminal", workspace_id="w", tab_id="t",
                                 agent="djinn", agent_session=None)]), "pane")
                groups.add_member(workspace, "review", "member", "Reviewer", binding)
                row(f"seed-{int(huge)}", dict(huge=huge), dict(binding_utf8=compact(binding).decode()), state_path.read_bytes())
                groups.remove_member(workspace, "review", "member")
                groups.add_member(workspace, "review", "member", "Reviewer", binding)
            predecessor = state_path.read_bytes()
            row(f"predecessor-{int(huge)}", dict(huge=huge), {}, predecessor)
            value = source()
            value["instances"][0]["sessions"][0]["activity"] = "unknown"
            artifact.write_bytes(compact(value))
            for attempt in range(2):
                result = group_live.attach(workspace, "review", "member", str(artifact), RUNTIME, INSTANCE,
                                           "selected", reattach=True)
                row(f"attach-{int(huge)}-{attempt}", dict(huge=huge, attempt=attempt, file=str(artifact), value=value),
                    result, state_path.read_bytes())
            if not huge:
                # Original status constructor uses the ordinary two-mutation seed.
                seed = next(r for r in rows if r["id"] == "seed-0")["raw_utf8"]
                state_path.write_text(seed)
                group_live.attach(workspace, "review", "member", str(artifact), RUNTIME, INSTANCE, "selected")
                row("group-status", dict(file=str(artifact), value=value), group_live.status(workspace, "review"), state_path.read_bytes())

    for i, case in enumerate(kitsune_cases()):
        payload = json.dumps(dict(id="test", result=case["result"])).encode()
        try:
            if case["kind"] == "process":
                result = p._processes(payload, "test", "opaque:p")
            else:
                (o._pane_snapshot if case["generic"] else o._snapshot)(payload, "test")
                result = "ok"
        except o._Failure as error:
            result = dict(error=error.code)
        row(f"kitsune-{i:02}", case, result)
    real = directory / "real"
    real.mkdir(exist_ok=True)
    alias = directory / "alias"
    if not alias.exists():
        alias.symlink_to(real)
    (real / "regular").write_bytes(b"not a socket")
    sockets = []
    try:
        for name, mode in [("private.sock", 0o600), ("unsafe.sock", 0o660)]:
            path = real / name
            path.unlink(missing_ok=True)
            listener = socket.socket(socket.AF_UNIX)
            sockets.append(listener)
            listener.bind(str(path))
            listener.listen()
            path.chmod(mode)
        link = real / "leaf-link"
        link.unlink(missing_ok=True)
        link.symlink_to(real / "private.sock")
        for parent in [real, alias]:
            for leaf in ["missing.sock", "regular", "leaf-link", "unsafe.sock", "missing/./ancestors/socket"]:
                for mode in [False, True]:
                    selected = str(parent) + "/" + leaf
                    result = p.inspect(selected, "opaque:p") if mode else o.inspect(selected)
                    row(f"endpoint-{parent.name}-{leaf.split('/')[0]}-{int(mode)}",
                        dict(socket=selected, process=mode),
                        {k: result[k] for k in ["endpoint", "assessment", "diagnostics"]})
        for listener in sockets:
            listener.setblocking(False)
            try:
                listener.accept()
            except BlockingIOError:
                pass
            else:
                raise AssertionError("unexpected endpoint contact")
    finally:
        for listener in sockets:
            listener.close()
    assert len(rows) == 63
    return encoded(dict(schema_version=1, capture_root=str(directory), cases=rows))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--fixtures-root", type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    assert not output.is_relative_to(project)
    assert Path(os.environ["TMPDIR"]).resolve(strict=True).is_relative_to(output)
    pins = verify(project, root)
    if args.worker:
        sys.stdout.buffer.write(capture(root, output))
        return
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8", TMPDIR=os.environ["TMPDIR"])
    children = []

    def stop(signum=None, frame=None):
        for child in children:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
            child.wait()
        if signum is not None:
            raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    captures = []
    try:
        for _ in range(2):
            child = subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()),
                "--historical-root", str(root), "--output-root", str(output), "--worker"],
                cwd=output, env=environment, stdout=subprocess.PIPE, start_new_session=True)
            children.append(child)
            raw, _ = child.communicate()
            assert child.returncode == 0
            captures.append(raw)
    finally:
        stop()
    assert captures[0] == captures[1], "repeat capture differs"
    corpus = captures[0]
    rows = json.loads(corpus)["cases"]
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
        interpreter=dict(version=sys.version, executable=sys.executable, sha256=digest(Path(sys.executable).read_bytes()),
                         isolated=True, bytecode=False), capture_tool_sha256=digest(Path(__file__).read_bytes()),
        corpus_sha256=digest(corpus), case_count=len(rows), repeat_capture_equal=True,
        case_ids=[r["id"] for r in rows], environment={**environment, "TMPDIR": "<owned run>/output/tmp"},
        seams=["runner.djinn_live_observation.time", "runner.group_live.time",
               "runner.group_live.datetime.datetime.now", "runner.groups.uuid.uuid4 (seed construction only)"],
        construction="Independently authored original matrices; all source pins checked before imports; no native output inputs. No business logic patches. Same real artifact inode across repeats; only expected-side metadata binding at replay."))
    for name, raw in [("source_live_observation.json", corpus), ("source_live_observation_provenance.json", proof)]:
        (output / name).write_bytes(raw)
        if args.fixtures_root is not None:
            destination = args.fixtures_root.resolve(strict=True)
            assert destination == project / "tests/retirement/fixtures"
            (destination / name).write_bytes(raw)
        print(json.dumps(dict(path=name, bytes=len(raw), sha256=digest(raw))))


if __name__ == "__main__":
    main()
