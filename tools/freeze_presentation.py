"""Optional stdlib-only capture of the active 24d73fe presentation oracle.

Run with a clean environment and Python 3.14.8 -I -B. Emits data on stdout;
never writes checkout files or imports native tests. Not a historical recapture.
"""

import argparse
import contextlib
import hashlib
import inspect
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

PIN = "24d73fede7d8e305cc2572c623a0eb93b440e806"
SOURCES = ["src/runtime/presentation/tests.rs", "src/task_delivery/tests.rs"]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    text = json.dumps(value, ensure_ascii=True, indent=2)
    text = re.sub(r"\[\s*\d+(?:,\s*\d+)*\s*\]", lambda m: "[" + ", ".join(re.findall(r"\d+", m[0])) + "]", text)
    return (text + "\n").encode()


def capture(root):
    if sys.version_info[:3] != (3, 14, 8) or not sys.flags.isolated or not sys.dont_write_bytecode:
        raise ValueError("requires Python 3.14.8 -I -B")
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if head != PIN:
        raise ValueError("unexpected active oracle HEAD")
    # Hash and verify the entire 25-module runner closure BEFORE importing it.
    paths = sorted(p.relative_to(root).as_posix() for p in (root / "runner").glob("*.py"))
    if len(paths) != 25:
        raise ValueError("runner inventory changed")
    hashes = {}
    originals = {}
    for name in paths + SOURCES:
        raw = (root / name).read_bytes()
        committed = subprocess.check_output(["git", "-C", str(root), "show", f"{PIN}:{name}"])
        if raw != committed:
            raise ValueError(f"source changed since approved pin: {name}")
        hashes[name] = sha(raw)
        if name in SOURCES:
            originals[name] = re.findall(r"#\[test\]\s*fn (\w+)", raw.decode())
    sys.path.insert(0, str(root))
    from runner import coven, checkpoints, storage, task_ledger
    for name, module in list(sys.modules.items()):
        if name.startswith("runner.") and Path(module.__file__).resolve().parent != root / "runner":
            raise ValueError("import escaped explicit oracle root")
    functions = {}
    runner_root = root / "runner"

    def profile(frame, event, arg):
        if event == "call":
            path = Path(frame.f_code.co_filename).resolve()
            if path.parent == runner_root:
                key = f"runner/{path.name}:{frame.f_code.co_qualname}:{frame.f_code.co_firstlineno}"
                if key not in functions:
                    functions[key] = sha(inspect.getsource(frame.f_code).encode())

    lines, cases, inputs = [], {}, {}

    def save(case, text):
        if case in cases:
            raise ValueError(f"duplicate case: {case}")
        indices = []
        for line in text.splitlines(keepends=True):
            if line not in lines:
                lines.append(line)
            indices.append(lines.index(line))
        assert "".join(lines[i] for i in indices) == text
        cases[case] = indices

    def write(root, path, data):
        (root / path).write_bytes(data if isinstance(data, bytes) else data.encode())

    def standup_root(base):
        root = base / "standup"
        root.mkdir()
        (root / "state").mkdir()
        write(root, "coven.json", b'{"name":"Status Team","agents":["Beta","alpha"],"display_names":{"Beta":"Label\\n<&>*\\u200b"},"multiplexer":"tmux"}')
        return root

    def dashboard_root(base):
        root = base / "dashboard"
        root.mkdir()
        (root / "state").mkdir()
        (root / "logs").mkdir()
        write(root, "coven.json", b'{"name":"Test & coven","agents":["a","b"],"multiplexer":"herdr","display_names":{"a":"A <&>_*|"}}')
        return root

    def approve(root):
        stat = root.stat()
        write(root, "state/runtime-launch-control.json", json.dumps({
            "version": 1, "action": "runtime_launch", "workspace_id": "a" * 32,
            "epoch_id": "b" * 32, "location": {"root": str(root), "device": stat.st_dev,
            "inode": stat.st_ino}, "head": None, "claims": {}}))

    def ledger(root):
        raw = (root / "state/tasks.json").read_bytes()
        with storage.workspace_lock(root, root / "state/.workspace.lock") as guard:
            task_ledger.LedgerStore(root).import_tasks(storage.snapshot_revision(raw), guard)

    sys.setprofile(profile)
    try:
        with tempfile.TemporaryDirectory(prefix="presentation-capture-") as temporary:
            base = Path(temporary)
            root = standup_root(base)
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                assert coven.command_standup(argparse.Namespace(workspace=str(root))) == 0
            save("standup.print", stdout.getvalue())
            write(root, "state/agents.json", b'[{"id":"outside","status":"waiting","role":"old"},{"id":"Beta","status":"active","role":"dev"}]')
            write(root, "state/lead.json", b'{"agent":"historical","extra":null}')
            statuses = ["done", "completed", "BLOCKED", "awaiting", "in_progress", "assigned", "active", "review", "", "new"]
            write(root, "state/tasks.json", json.dumps([{"id": f"t{i}", "title": f"Title {i}", "status": s, "owner": "outside" if i % 2 == 0 else None} for i, s in enumerate(statuses)]))
            (root / "checkpoints").mkdir()
            write(root, "checkpoints/plan-approval.md", "# Plan\r\nStatus: approved\r\n")
            original_status = coven.agent_runtime_status
            seen = []

            def running(team, agent):
                seen.append(agent)
                return "running", "pane-" + agent

            coven.agent_runtime_status = running
            save("standup.buckets", coven.render_standup(root))
            assert seen == ["outside", "Beta", "historical"]
            coven.agent_runtime_status = original_status
            for path in ["state/lead.json", "state/tasks.json", "checkpoints/plan-approval.md"]:
                (root / path).unlink()
            approve(root)
            write(root, "state/agents.json", b'[{"id":"Beta","status":"saved","role":"dev"}]')
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                assert coven.command_agent_status(argparse.Namespace(items=[str(root)])) == 0
            save("agent_status.approved", stdout.getvalue())
            (root / "state/agents.json").unlink()
            write(root, "state/tasks.json", b'[{"id":"ledger-task","title":"Ledger authority wins","status":"done"}]')
            ledger(root)
            write(root, "state/tasks.json", b"corrupt projection")
            save("standup.ledger", coven.render_standup(root))
            assert (root / "state/tasks.json").read_bytes() == b"corrupt projection"
            # Separate fresh root: checkpoint matrix has neither ledger nor control.
            (base / "matrix").mkdir()
            root = standup_root(base / "matrix")
            (root / "checkpoints").mkdir()
            approved, _ = checkpoints.prepare("pre-delivery-review", b"# Review\r\nStatus: pending\r\n## User decision\r\n- [ ] Approved\r\n", "approved", "", decision_id="1" * 32, decided_at="test", source="cli")
            inputs["review.approved"] = approved.decode()
            for content, status in [(approved, "approved"), (approved.replace(b"# Review\r\n", b"# Changed review\r\n"), "approval_stale"), (approved.replace(b"Status: approved", b"Status: pending"), "decision_invalid"), (b"# Review\nStatus: pending\n", "pending")]:
                write(root, "checkpoints/pre-delivery-review.md", content)
                assert coven.checkpoint_status(root, "pre-delivery-review") == status
                for tasks, label in [(b"[]", "empty"), (b'[{"id":"done","title":"Complete","status":"done"}]', "done")]:
                    write(root, "state/tasks.json", tasks)
                    save(f"standup.checkpoint.{status}.{label}", coven.render_standup(root))
            root = dashboard_root(base)
            team = json.loads((root / "coven.json").read_bytes())
            team["display_names"]["a"] = "A <&>_*| \u00a0\u200d\ue000\u0378\U0010ffff\U0001f98a"
            write(root, "coven.json", json.dumps(team))
            write(root, "state/tasks.json", b'[{"id":"t","title":"Raw | *title*\\nnext","status":"raw|status","owner":null}]')
            write(root, "state/agents.json", b'[{"id":"a","status":"idle","role":"raw | role"},{"id":"b","status":"ready","role":"builder"}]')
            write(root, "coven.toml", "[runtime]\nmultiplexer = ' tmux '\n")
            (root / "checkpoints").mkdir()
            write(root, "checkpoints/plan-approval.md", b"# Plan\r\nStatus: approved\r\n## User decision\r\n- [x] Approved\r\n")
            write(root, "checkpoints/pre-delivery-review.md", b"# Review\rStatus: pending\r")
            for i, lead in enumerate([b"null", b'{"agent":"a"}', b'{"agent":"b","set_at":"","until":"raw | until"}']):
                write(root, "state/lead.json", lead)
                for j, goal in enumerate([b" \r\n# Goal\r\n\rMixed\r\nnewlines\r ", b"# Goal \r\n ", b"# Goalkeeper\rtext", b"  no heading\r\n"]):
                    write(root, "goal.md", goal)
                    save(f"dashboard.lead{i}.goal{j}", coven.render_dashboard(root))
            (root / "goal.md").unlink()
            (root / "state/lead.json").unlink()
            save("dashboard.missing", coven.render_dashboard(root))
            ledger(root)
            write(root, "state/tasks.json", b"not a projection")
            save("dashboard.ledger", coven.render_dashboard(root))
            assert (root / "state/tasks.json").read_bytes() == b"not a projection"
            (base / "checkpoint").mkdir()
            root = dashboard_root(base / "checkpoint")
            (root / "checkpoints").mkdir()
            approved, _ = checkpoints.prepare("plan-approval", b"# Plan\r\nStatus: pending\r\n## User decision\r\n- [ ] Approved\r\n", "approved", "", decision_id="1" * 32, decided_at="test", source="cli")
            inputs["plan.approved"] = approved.decode()
            for index, (content, status) in enumerate([(approved, "approved"), (approved.replace(b"# Plan\r\n", b"# Changed plan\r\n"), "approval_stale"), (approved.replace(b"Status: approved", b"Status: pending"), "decision_invalid"), (approved.replace(b"\r\n", b"\n"), "approval_stale")]):
                write(root, "checkpoints/plan-approval.md", content)
                assert coven.checkpoint_status(root, "plan-approval") == status
                save(f"dashboard.checkpoint.{index}", coven.render_dashboard(root))
    finally:
        sys.setprofile(None)
    if not functions:
        raise ValueError("missing reference function provenance")
    if len(cases) != 30 or len(inputs) != 2:
        raise ValueError("case inventory changed")
    corpus = {"encoding": "UTF-8 literal line pool; concatenate indexed lines without separators", "lines": lines, "cases": cases, "inputs": inputs}
    provenance = {
        "oracle_commit": PIN, "oracle_selection": "explicit --oracle-root, current approved active source, not historical e389a086",
        "python": sys.version, "isolated": True, "bytecode_disabled": True,
        "capture_command": "env -i PATH=/usr/bin:/bin HOME=<home> TMPDIR=<disk-backed-run> COVEN_MULTIPLEXER= /usr/bin/python3 -I -B tools/freeze_presentation.py --oracle-root <explicit-root>",
        "capture_tool_sha256": sha(Path(__file__).read_bytes()), "source_sha256_before_import": hashes,
        "original_rust_methods": originals, "original_rust_test_counts": {k: len(v) for k, v in originals.items()},
        "reference_function_source_sha256": dict(sorted(functions.items())),
        "case_count": len(cases), "case_ids": list(cases), "matrix_counts": {"standup_single": 4, "standup_checkpoints": 8, "dashboard_lead_goal": 12, "dashboard_missing_and_ledger": 2, "dashboard_checkpoints": 4},
        "corpus_sha256": sha(encoded(corpus)), "corpus_bytes": len(encoded(corpus)),
        "output_sha256": {key: sha("".join(lines[i] for i in indices).encode()) for key, indices in cases.items()},
        "input_sha256": {key: sha(text.encode()) for key, text in inputs.items()},
        "volatile_inputs": "decision_id=11111111111111111111111111111111; decided_at=test; source=cli. Actual resolved root/device/inode required only by control admission; ledger UUIDs are ephemeral reference inputs. None appears in expected outputs or fixtures.",
        "scope": "Comparison evidence for seven existing native tests only; no new mapping of original Python test methods and no historical fixture recapture. Native I/O and assertion obligations remain in their callers."
    }
    for path, value in [("tests/retirement/fixtures/presentation.json", corpus), ("tests/retirement/fixtures/presentation_provenance.json", provenance)]:
        print(f"*** Add File: {path}")
        for line in encoded(value).decode().splitlines():
            print("+" + line)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", required=True, type=Path)
    args = parser.parse_args()
    print("*** Begin Patch")
    capture(args.oracle_root.resolve(strict=True))
    print("*** End Patch")
