"""Optional E2 capture. Requires an explicit, byte-verified historical export.

Run with Python 3.14.8 -I -B and env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8
TMPDIR=<owned disk-backed run>. Writes only the two named generated fixtures.
"""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
CALLERS = ["src/workspace.rs", "src/collection.rs", "src/state_commands/tests.rs"]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return (json.dumps(value, ensure_ascii=True, indent=2) + "\n").encode()


def files_sha(files):
    return sha(b"".join(path.encode() + b"\0" + raw.encode() + b"\0" for path, raw in sorted(files.items())))


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    args = parser.parse_args()
    def interrupted(number, _frame):
        raise SystemExit(128 + number)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    assert set(os.environ) <= {"PATH", "LC_ALL", "TMPDIR"}
    assert os.environ["LC_ALL"] == "C.UTF-8" and Path(os.environ["TMPDIR"]).is_dir()
    repo = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != repo and not root.is_relative_to(repo) and not (root / ".git").exists()
    # The whole native/helper tree closes indirect test helpers as well as callers.
    paths = git(repo, "ls-tree", "-r", "--name-only", PIN, "runner", "src", "examples").decode().splitlines()
    paths = sorted(p for p in paths if "__pycache__" not in p and not p.endswith(".pyc"))
    runners = [p for p in paths if p.startswith("runner/") and p.endswith(".py")]
    assert len(runners) == 25
    assert sorted(p.relative_to(root).as_posix() for p in (root / "runner").rglob("*.py")) == runners
    assert not list(root.rglob("*.pyc"))
    inventory = {}
    for path in paths:
        raw = (root / path).read_bytes()
        assert raw == git(repo, "show", f"{PIN}:{path}"), path
        inventory[path] = {"bytes": len(raw), "sha256": sha(raw)}
    workspace = (root / CALLERS[0]).read_text()
    state = (root / CALLERS[2]).read_text()
    # Recover the actual original inline programs, not a copied implementation.
    inline = [ast.literal_eval('"' + s + '"') for s in re.findall(r'\.args\(\["-c", "((?:[^"\\]|\\.)*)"\]\)', workspace)]
    runtime = next(s for s in inline if "read_team_config" in s)
    constants = next(s for s in inline if "MANAGED_DIRECTORIES" in s)
    imported = next(s for s in inline if "import-tasks" in s)
    report = re.search(r'let script = r#"(.*?)"#;', state, re.S)[1]
    # Decode Rust's NBSP escape, otherwise this bounded case list is literal text.
    case_section = workspace.split("fn toml_runtime_fields_use_strings", 1)[1].split("for (text, code)", 1)[0]
    texts = re.findall(r'"((?:[^"\\]|\\.)*)"', case_section)
    texts = [ast.literal_eval('"' + s.replace(r"\u{a0}", r"\u00a0") + '"') for s in texts]
    texts = [s for s in texts if s.startswith(("[runtime]", "runtime="))]
    assert len(texts) == 14
    bootstrap = "import sys;sys.path.insert(0,sys.argv.pop(1));exec(compile(sys.argv.pop(1),'historical-inline','exec'))"
    env = dict(os.environ)

    def execute(script, *arguments):
        child = subprocess.Popen([sys.executable, "-I", "-B", "-c", bootstrap, str(root), script, *map(str, arguments)],
                                 cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            stdout, stderr = child.communicate(timeout=15)
            return dict(stdout=stdout.decode(), stdout_sha256=sha(stdout),
                        stdout_bytes=len(stdout), stderr=stderr.decode(), exit_status=child.returncode)
        finally:
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()

    def files(base):
        return {p.relative_to(base).as_posix(): p.read_bytes().decode() for p in sorted(base.rglob("*")) if p.is_file()}

    def row(identity, inputs, result):
        return dict(id=identity, inputs=inputs, input_sha256=files_sha(inputs), **result)

    rows = []
    with tempfile.TemporaryDirectory(prefix="source-workspace-") as temporary:
        base = Path(temporary)
        for index, text in enumerate(texts):
            case = base / f"runtime-{index}"
            case.mkdir()
            (case / "coven.json").write_bytes(b'{"name":"test","agents":["operator"]}')
            (case / "coven.toml").write_text(text)
            rows.append(row(f"runtime-{index}", files(case), execute(runtime, case)))
        rows.append(row("inventories", {}, execute(constants)))
        # Only nondeterministic metadata is patched: uuid.uuid4, not ledger logic.
        fixed = "import uuid\nids=iter(['a'*32,'b'*32])\nuuid.uuid4=lambda:uuid.UUID(hex=next(ids))\n"
        setup = """
from pathlib import Path
import sys
from runner import storage,task_ledger
w=Path(sys.argv[1])
with storage.workspace_lock(w,w/'state/.workspace.lock') as held:
 task_ledger.LedgerStore(w).import_tasks(storage.snapshot_revision((w/'state/tasks.json').read_bytes()),held)
"""
        fixtures = {}
        seed = b' [ {"id":"t","title":"Task","status":"custom","extension":[123456789012345678901234567890,-0.0,"\\u03bb"]} ]\n'
        assert seed == re.search(r'fs::write\(root.path\(\).join\("state/tasks.json"\), br#"(.*?)"#', state, re.S)[1].encode()
        for name, source in [("empty", b"[]\n"), ("seeded", seed)]:
            case = base / name
            (case / "state").mkdir(parents=True)
            (case / "coven.json").write_bytes(b'{"name":"test","agents":["operator"]}')
            (case / "state/tasks.json").write_bytes(source)
            (case / "state/tasks.json").chmod(0o644)
            before = files(case)
            result = execute(fixed + setup, case)
            assert result["exit_status"] == 0 and not result["stderr"]
            captured = files(case)
            captured.pop("state/.workspace.lock", None)
            fixtures[name] = captured
            rows.append(row(f"ledger-{name}", before, result | {"artifacts": captured, "artifacts_sha256": files_sha(captured)}))
            if name == "seeded":
                for pretty in [False, True]:
                    for mode in ["inspect", "import", "repair", "resume"]:
                        if mode == "inspect":
                            (case / "state/task-authority.json").unlink()
                            # Snapshot inspection must not see a pending ledger.
                            (case / "state/task-ledger").rename(case / "saved-ledger")
                        inputs = {k: v for k, v in files(case).items() if not k.startswith("saved-ledger/") and k != "state/.workspace.lock"}
                        result = execute(report, case, mode, "pretty" if pretty else "compact")
                        assert result["exit_status"] == 0 and not result["stderr"]
                        rows.append(row(f"report-{mode}-{'pretty' if pretty else 'compact'}", inputs, result))
                        if mode == "inspect":
                            (case / "saved-ledger").rename(case / "state/task-ledger")
                            (case / "state/task-authority.json").write_text(captured["state/task-authority.json"])
    assert len(rows) == 25
    corpus = encode(dict(schema_version=1, cases=rows))
    provenance = encode(dict(schema_version=1, historical_commit=PIN, source_inventory=inventory, source_inventory_sha256=sha(encode(inventory)),
                             runner_modules=25, original_callers=CALLERS, inline_programs=dict(runtime=runtime, inventories=constants, imported=imported, report=report),
                             setup_program=setup, metadata_patch=fixed, bootstrap=bootstrap,
                             metadata_patch_targets=["runner.task_ledger.uuid.uuid4 (shared uuid module)"],
                             interpreter=dict(version=sys.version, executable_sha256=sha(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                             capture_tool_sha256=sha(Path(__file__).read_bytes()), environment_keys=sorted(env),
                             corpus_sha256=sha(corpus), rows=25, runtime_cases=14, inventories=1, coherent_ledgers=2, full_reports=8,
                             policy="Independent tomllib 528-document oracle unchanged and outside legacy closure. Complete stdout includes original LF. Native expected frame hashes derive from captured payload bytes, never native encoding or actual output. No review or coverage mapping claims."))
    for name, raw in [("source_workspace.json", corpus), ("source_workspace_provenance.json", provenance)]:
        (repo / "tests/retirement/fixtures" / name).write_bytes(raw)
        print(f"{name} {len(raw)} {sha(raw)}")


if __name__ == "__main__":
    main()
