#!/usr/bin/env python3
"""Capture E1 fixtures only from an explicit, verified a249 Git archive."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
CALLERS = ["src/actions/mutation/tests.rs", "src/checkpoint_guidance/tests.rs"]
IDS = ["inventory", "reviewed", "completed", "failed", "import", "imported_inputs",
       "reapproved", "unknown_ack", "guidance_yes", "guidance_test", "inventory_herdr"]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return (json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n").encode()


def capture(root, work, scripts, guidance_script):
    # Imports occur only AFTER the entire historical closure is verified in main.
    sys.path.insert(0, str(root))
    from runner import actions, coven
    import uuid
    # Identity/timestamp sources only, never validators or business logic.
    # Original targets: actions.py:345-346,447; task_ledger.py:413;
    # coven.py:306 (uuid.uuid4, now). Explicit decision IDs/times remain unchanged.
    counter = iter(range(1, 1000))
    uuid.uuid4 = lambda: uuid.UUID(int=next(counter))
    coven.now = lambda: "2025-01-01T00:00:00Z"
    records = []

    def run(case, script, data=b"", argument=None):
        old_argv, old_stdin, old_stdout, old_stderr = sys.argv, sys.stdin, sys.stdout, sys.stderr
        stream = io.BytesIO()
        output = io.TextIOWrapper(stream, encoding="utf-8", write_through=True)
        errors = io.StringIO()
        try:
            sys.argv = ["historical-inline", str(work) if argument is None else argument]
            sys.stdin = io.TextIOWrapper(io.BytesIO(data), encoding="utf-8")
            sys.stdout = output
            sys.stderr = errors
            exec(compile(script, "historical-inline", "exec"), {})
            output.flush()
            raw = stream.getvalue()
        finally:
            sys.argv, sys.stdin, sys.stdout, sys.stderr = old_argv, old_stdin, old_stdout, old_stderr
        files = {str(p.relative_to(work)): p.read_bytes().decode("utf-8") for p in sorted(work.rglob("*"))
                 if p.is_file()}
        records.append(dict(id=case, input=data.decode(), input_sha256=sha(data),
                            stdout=raw.decode(), stdout_sha256=sha(raw), stdout_bytes=len(raw),
                            exit_status=0, stderr=errors.getvalue(), files=files,
                            files_sha256={p: sha(s.encode()) for p, s in files.items()}))
        return raw

    for name in ["state", "prompts", "agents", "checkpoints", "logs"]:
        (work / name).mkdir()
    (work / "coven.json").write_bytes(b'{"name":"test","agents":["alice"],"multiplexer":"tmux","opencode_command":"oc --flag"}')
    (work / "state/tasks.json").write_bytes(b"[]")
    (work / "prompts/alice.md").write_text("Role <literal> `code` \u96ea\n")
    (work / "prompts/alice.start.txt").write_text("Start\nnow <literal> `code` \U0001f98a")
    store = actions.ActionStore(work)
    control = {"version": 1, "action": "runtime_launch", "workspace_id": "a" * 32,
               "epoch_id": "b" * 32, "location": store.location(), "head": None, "claims": {}}
    store._write(control)
    action_id = "c" * 32
    inputs = coven.runtime_launch_inputs(work)
    plan = coven.frozen_launch_plan(control, action_id, inputs)
    with coven.storage.workspace_lock(work, work / "state/.workspace.lock") as lock:
        bundle = store.make_bundle(inputs, plan, action_id, lock)
        store.publish_bundle(bundle, lock)
    cp = "runtime-launch-" + action_id
    cp_path = work / "checkpoints" / (cp + ".md")
    cp_path.write_bytes(actions.checkpoint_bytes(bundle))
    run("inventory", scripts[1], actions.canonical(bundle))
    os.environ["COVEN_MULTIPLEXER"] = "herdr"
    herdr_inputs = coven.runtime_launch_inputs(work)
    herdr_bundle = bundle | {"inputs": herdr_inputs, "plan": coven.frozen_launch_plan(control, action_id, herdr_inputs)}
    run("inventory_herdr", scripts[1], actions.canonical(herdr_bundle))
    herdr_record = records.pop()
    del os.environ["COVEN_MULTIPLEXER"]
    view = json.loads(run("reviewed", scripts[0], json.dumps(cp).encode()))
    refs = [action_id, actions.digest(bundle), view["revision"], view["decision"]["decision_id"]]
    with coven.storage.workspace_lock(work, work / "state/.workspace.lock") as lock:
        claimed, replayed = store.claim(*refs, inputs, lock)
        assert not replayed
    claimed_control = (work / actions.CONTROL).read_bytes()
    for failed in [False, True]:
        (work / actions.CONTROL).write_bytes(claimed_control)
        report = dict(operation="runtime_launch", status="transport_failed_or_partial" if failed else "transport_completed",
                      completed_steps=1, handles={}, diagnostics=[], outcome_unknown=failed)
        with coven.storage.workspace_lock(work, work / "state/.workspace.lock") as lock:
            store.record_outcome(action_id, claimed["claim_id"], report, lock)
        run("failed" if failed else "completed", scripts[2], json.dumps(action_id).encode())
    run("import", scripts[3])
    run("imported_inputs", scripts[4])
    run("reapproved", scripts[5], json.dumps(action_id).encode())
    (work / actions.CONTROL).write_bytes(claimed_control)
    request = dict(zip(["id", "manifest", "checkpoint", "decision"], refs, strict=True), claim=claimed["claim_id"])
    run("unknown_ack", scripts[6], actions.canonical(request))
    for name in ["yes", "test"]:
        run("guidance_" + name, guidance_script, argument=name)
    records.append(herdr_record)
    assert [c["id"] for c in records] == IDS
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated
    assert sys.dont_write_bytecode
    tool = Path(__file__).resolve()
    project = tool.parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                    "runner", "src/actions.rs", "src/actions", "src/checkpoint_guidance.rs",
                                    "src/checkpoint_guidance", "src/checkpoints.rs", "src/checkpoints", "src/launch.rs",
                                    "src/values"], cwd=project).decode().splitlines()
    pins = {}
    paths = [p for p in paths if "__pycache__" not in p and not p.endswith(".pyc")]
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = sha(raw)
    assert sum(p.startswith("runner/") and p.endswith(".py") for p in pins) == 25
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").glob("*.py")) == sorted(p for p in pins if p.startswith("runner/") and p.endswith(".py"))
    assert not list(root.rglob("*.pyc"))
    original = (root / CALLERS[0]).read_text()
    guidance_original = (root / CALLERS[1]).read_text().split("fn approved", 1)[1].split("#[test]", 1)[0]
    concat = guidance_original.split("concat!(", 1)[1].split("), id]", 1)[0]
    guidance_script = "".join(json.loads(s) for s in re.findall(r'^\s+(".*"),?$', concat, re.M))
    assert guidance_script.startswith("import sys; from runner import checkpoints\n")
    helper = "fn oracle" + original.split("fn oracle", 1)[1].split("pub(super) fn fixture", 1)[0]
    scripts = [json.loads(s) for s in re.findall(r'^        +("import .*"),$', original, re.M)]
    assert len(scripts) == 7
    # The inventory-input script has a shorter indent but is included above.
    assert scripts[2].startswith("import sys,json")
    assert scripts[3].startswith("import sys;from pathlib")
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    os.environ.clear()
    os.environ.update(env)
    os.chdir(root)
    work = Path(os.environ["TMPDIR"]) / "source-actions-workspace"
    work.mkdir()
    try:
        first = capture(root, work, scripts, guidance_script)
        for child in work.iterdir():
            shutil.rmtree(child) if child.is_dir() else child.unlink()
        second = capture(root, work, scripts, guidance_script)
        assert first == second, "repeated capture differs"
        assert not list(root.rglob("*.pyc"))
        corpus = encode(dict(schema_version=1, case_ids=IDS, cases=first))
        inventory_sha = sha(encode(pins))
        provenance = encode(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                                 source_inventory_sha256=inventory_sha,
                                 inline_scripts=scripts, inline_scripts_sha256=sha(encode(scripts)),
                                 inline_script_sha256=[sha(s.encode()) for s in scripts],
                                 helper_original=helper, helper_original_sha256=sha(helper.encode()),
                                 guidance_original="fn approved" + guidance_original,
                                 guidance_original_sha256=sha(("fn approved" + guidance_original).encode()),
                                 guidance_script=guidance_script, guidance_script_sha256=sha(guidance_script.encode()),
                                 capture_tool_sha256=sha(tool.read_bytes()), corpus_sha256=sha(corpus),
                                 interpreter=dict(version=sys.version, executable=sys.executable,
                                                  sha256=sha(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                                 environment=env, case_ids=IDS, repeated_capture_identical=True,
                                 metadata_patches=["uuid.uuid4 (actions enable/claim, ledger import, event IDs)", "coven.now (event timestamps)"],
                                 parent_batch_gate="pending; no per-slice Cargo invocation"))
        destination = project / "tests/retirement/fixtures"
        (destination / "source_actions.json").write_bytes(corpus)
        (destination / "source_actions_provenance.json").write_bytes(provenance)
        print(json.dumps(dict(cases=len(first), source_pins=len(pins), corpus_sha256=sha(corpus),
                              provenance_sha256=sha(provenance), source_inventory_sha256=inventory_sha,
                              repeated_capture_identical=True)))
    finally:
        shutil.rmtree(work)


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    main()
