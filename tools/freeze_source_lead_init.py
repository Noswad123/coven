#!/usr/bin/env python3
"""Optional capture from an explicit, pinned Git archive; never the live runner."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
HELPER = "src/init/mixed_overlap.py"
TEAM = br'{"name":"Lead Team","agents":["a","b"],"display_names":{"a":"A\n\u200b"}}'
GOAL = b"# Goal\r\n\r\nDeliver safely\r\n"
INITIAL = [br'{"agent":"historical","extension":{"x":null},"nullable":null,"until":"old"}',
           br'{"agent":"historical","set_at":"","extension":[null,{"preserve":true}]}']
PATHS = ["state/lead.json", "logs/events.jsonl", "logs/messages.jsonl", "dashboard.md"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode("ascii")


def verify(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                     "runner", "src"], cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith((".py", ".rs")))
    assert HELPER in paths and "src/lead/tests.rs" in paths and "src/init/overlap.rs" in paths
    assert sum(p.startswith("runner/") for p in paths) == 25
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in paths if p.startswith("runner/")]
    assert sorted(str(p.relative_to(root)) for p in (root / "src").rglob("*")
                  if p.suffix in (".rs", ".py")) == [p for p in paths if p.startswith("src/")]
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert not list(root.rglob("*.pyc"))
    return pins


def wait(ipc, name, child):
    deadline = time.monotonic() + 15
    while not (ipc / name).exists():
        assert child.poll() is None, child.returncode
        assert time.monotonic() < deadline, name
        time.sleep(0.01)


def stop(child):
    try:
        os.killpg(child.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait()


def capture(argv, root, environment):
    child = subprocess.Popen(argv, cwd=root, env=environment, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, start_new_session=True)
    try:
        stdout, stderr = child.communicate(timeout=30)
        assert child.returncode == 0, stderr
        return stdout, stderr
    finally:
        stop(child)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    temporary = Path(os.environ["TMPDIR"]).resolve(strict=True)
    assert not temporary.is_relative_to(project)
    pins = verify(project, root)  # All sources checked before any historical Python executes.
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8",
                       TMPDIR=str(temporary), COVEN_MULTIPLEXER="")
    # Extract the original inline programs, rather than maintaining runner copies.
    source = (root / "src/lead/tests.rs").read_text()
    programs = re.findall(r'Command::new\("python3"\).*?concat!\((.*?)\)\]\)', source, re.S)
    assert len(programs) == 2
    programs = ["".join(json.loads(s) for s in re.findall(r'"(?:[^"\\]|\\.)*"', p)) for p in programs]
    bootstrap = "import sys;sys.path.insert(0,sys.argv.pop(1));exec(compile(sys.argv.pop(1),'historical-inline','exec'))"
    workspace = temporary / "workspace"
    rows = []
    for index, message in enumerate([None, "", "custom message", None]):
        captures = []
        clear = index == 3
        paths = PATHS[1::2] if clear else PATHS
        inputs = {"coven.json": TEAM.decode(), "goal.md": GOAL.decode(),
                  "state/lead.json": INITIAL[int(clear)].decode()}
        for _ in range(2):
            workspace.mkdir()
            try:
                (workspace / "state").mkdir()
                for name, text in inputs.items():
                    (workspace / name).write_bytes(text.encode())
                argv = [sys.executable, "-I", "-B", "-c", bootstrap, str(root),
                        programs[int(clear)], str(workspace)]
                if not clear:
                    argv.append(json.dumps(message))
                stdout, stderr = capture(argv, root, environment)
                assert stderr == b""
                files = {p: (workspace / p).read_bytes().decode() for p in paths}
                parsed = json.loads(stdout)
                assert parsed["files"] == list(files.values())
                assert not clear or parsed["exists"] is False
                captures.append((stdout, files))
            finally:
                shutil.rmtree(workspace)
        assert captures[0] == captures[1]
        stdout, files = captures[0]
        stdin = encoded(dict(files=inputs, message=message, operation="clear" if clear else "set"))
        rows.append(dict(id=["set-default", "set-empty", "set-custom", "clear"][index],
                         message=message, inputs=inputs, input_utf8=stdin.decode(), input_sha256=digest(stdin),
                         captured_workspace=str(workspace), stdout_utf8=stdout.decode(), stdout_sha256=digest(stdout),
                         body=json.loads(stdout)["body"], files=files,
                         file_sha256={p: digest(s.encode()) for p, s in files.items()}))
    # Exercise the unmodified historical helper with its real process barriers.
    # This is not a claim to rerun the Rust/Python mixed test or its native half.
    marker_captures = []
    for _ in range(2):
        ipc = temporary / "ipc"
        ipc.mkdir()
        child = None
        try:
            program = "import sys,runpy;sys.path.insert(0,sys.argv[1]);from runner import coven;coven.now=lambda:'2026-11-05T12:00:00+00:00';sys.argv=sys.argv[2:];runpy.run_path(sys.argv[0],run_name='__main__')"
            with (ipc / "stdout").open("wb") as out, (ipc / "stderr").open("wb") as err:
                child = subprocess.Popen([sys.executable, "-I", "-B", "-c", program, str(root),
                                          str(root / HELPER), str(root), str(workspace), str(ipc)],
                                         cwd=root, env=environment, stdout=out, stderr=err,
                                         start_new_session=True, umask=0o022)
                wait(ipc, "preflight-passed", child)
                assert not workspace.exists()
                (workspace / "state").mkdir(parents=True)
                import fcntl
                with (workspace / "state/.workspace.lock").open("xb") as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    (ipc / "continue-preflight").write_bytes(b"go")
                    wait(ipc, "marker-published", child)
                    marker = (workspace / "coven.json").read_bytes()
                    mode = (workspace / "coven.json").stat().st_mode & 0o777
                (ipc / "continue-after-marker").write_bytes(b"go")
                assert child.wait(timeout=30) == 0, (ipc / "stderr").read_bytes()
                assert (ipc / "stderr").read_bytes() == b""
                assert (workspace / "coven.json").read_bytes() == marker
                marker_captures.append((marker, mode, (ipc / "stdout").read_bytes()))
        finally:
            if child is not None:
                stop(child)
            if workspace.exists():
                shutil.rmtree(workspace)
            shutil.rmtree(ipc)
    assert marker_captures[0] == marker_captures[1]
    marker, mode, stdout = marker_captures[0]
    assert mode == 0o644
    corpus = encoded(dict(schema_version=1, case_ids=[r["id"] for r in rows], cases=rows,
                          marker=dict(raw_utf8=marker.decode(), sha256=digest(marker), mode=mode,
                                      stdout_utf8=stdout.decode(), stdout_sha256=digest(stdout))))
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                         capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(corpus),
                         interpreter=dict(version=sys.version, executable=sys.executable,
                                          sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                         environment={**environment, "TMPDIR": "<owned disk-backed run>/tmp"},
                         case_count=4, lead_capture_count=8, helper_capture_count=2, repeat_capture_equal=True,
                         inline_program_sha256=[digest(p.encode()) for p in programs],
                         retired_helper=dict(path=HELPER, sha256=pins[HELPER]),
                         disposition="Parent-approved: Python-specific live compatibility qualification retained historically; current independent native child uses OS exclusive open, not the production reservation API.",
                         historical_probe="Unmodified helper, fixed clock, two real process barriers, held OS flock; not an execution of the historical Rust/native mixed test.",
                         binding="Only captured workspace path in expected prompt/message bytes may bind to native input root before operation. No output normalization; all other bytes opaque.",
                         counts="13 existing lead tests retained; one additional ignored init child declaration called by the existing parent, not a separate coverage claim."))
    assert not list(root.rglob("*.pyc"))
    for name, raw in [("source_lead_init.json", corpus), ("source_lead_init_provenance.json", proof)]:
        (output / name).write_bytes(raw)
        print(json.dumps(dict(path=name, bytes=len(raw), sha256=digest(raw))))


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    main()
