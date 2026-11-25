"""Optional historical byte capture, never required by native tests.

Use Python 3.14.8 -I -B, a clean environment and an explicit historical export.
Emits an apply_patch patch on stdout; does not write checkout files. Export the
paths in reference_paths() from PIN with git archive, not a new worktree.
"""

import argparse
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys

PIN = "8e41fe10ab041b4355205688e3458d5b8906c80c"
HELPERS = ["src/workspace_assets/oracle.py", "src/workspace_assets/init_oracle.py"]
RUST = ["src/workspace_assets.rs", "src/workspace_assets/tests.rs",
        "src/workspace_assets/init_tests.rs", "src/workspace_assets/tests/bootstrap.rs",
        "src/workspace_assets/tests/bootstrap_fixture.rs"]


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args])


def reference_paths(repo):
    paths = git(repo, "ls-tree", "-r", "--name-only", PIN,
                "runner", "examples/interactive-demo").decode().splitlines()
    runners = [p for p in paths if p.startswith("runner/") and p.endswith(".py")]
    assets = [p for p in paths if p.startswith("examples/")]
    if len(runners) != 25 or len(assets) != 7:
        raise ValueError("unexpected historical source inventory")
    return sorted(runners + assets + HELPERS + RUST)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, indent=2) + "\n").encode()


def pack(value):
    # Lossless line/text interning, not output or input normalization.
    lines, texts, line_indices, text_indices = [], [], {}, {}

    def visit(item):
        if isinstance(item, str):
            if item not in text_indices:
                refs = []
                for line in item.splitlines(keepends=True):
                    if line not in line_indices:
                        line_indices[line] = len(lines)
                        lines.append(line)
                    refs.append(line_indices[line])
                assert "".join(lines[i] for i in refs) == item
                text_indices[item] = len(texts)
                texts.append(refs)
            return {"$text": text_indices[item]}
        if isinstance(item, list):
            return [visit(v) for v in item]
        if isinstance(item, dict):
            return {k: visit(v) for k, v in item.items()}
        return item

    data = visit(value)

    def rows(items):
        return "[\n" + ",\n".join(json.dumps(v, ensure_ascii=True) for v in items) + "\n]"

    rendered = rows(data) if isinstance(data, list) else (
        "{\n" + ",\n".join(json.dumps(k) + ":" + (rows(v) if isinstance(v, list)
                            else json.dumps(v, ensure_ascii=True)) for k, v in data.items()) + "\n}")
    return ("{\n\"lines\":" + rows(lines) + ",\n\"texts\":" + rows(texts)
            + ",\n\"data\":" + rendered + "\n}\n").encode()


def capture(root, repo):
    if sys.version_info[:3] != (3, 14, 8) or not sys.flags.isolated or not sys.dont_write_bytecode:
        raise ValueError("requires Python 3.14.8 -I -B")
    if set(os.environ) - {"PATH", "LC_ALL", "TMPDIR"} or os.environ.get("LC_ALL") != "C.UTF-8":
        raise ValueError("requires env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 TMPDIR=<owned-run>")
    if git(repo, "rev-parse", f"{PIN}^{{commit}}").decode().strip() != PIN:
        raise ValueError("reference commit mismatch")
    hashes = {}
    # Close the entire local import/asset surface BEFORE executing either helper.
    paths = reference_paths(repo)
    modules = [path for path in paths if path.startswith("runner/")]
    actual = sorted(path.relative_to(root).as_posix() for path in (root / "runner").rglob("*.py"))
    if actual != modules or any((root / "runner").rglob("*.pyc")):
        raise ValueError("export exactly the pinned runner sources, without bytecode or extra modules")
    for path in paths:
        raw = (root / path).read_bytes()
        if raw != git(repo, "show", f"{PIN}:{path}"):
            raise ValueError(f"historical source mismatch: {path}")
        hashes[path] = sha(raw)
    sys.path.insert(0, str(root))
    previous = Path.cwd()
    os.chdir(root)
    captures = []
    try:
        for helper in HELPERS:
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                runpy.run_path(str(root / helper), run_name="__main__")
            captures.append(json.loads(output.getvalue()))
        for name, module in sys.modules.items():
            if name.startswith("runner.") and Path(module.__file__).resolve().parent != root / "runner":
                raise ValueError("runner import escaped historical tree")
    finally:
        os.chdir(previous)
    assets, init = captures
    assert len(assets) == 22
    assert len(init["teams"]) == 48 and len(init["agents"]) == 13
    assert len(init["checkpoints"]) == 2 and len(init["defaults"]) == 3
    # Index IDs distinguish duplicate input rows without collapsing any entries.
    for index, case in enumerate(assets):
        case["case_id"] = f"asset-{index:02}"
    for index, case in enumerate(init["teams"]):
        case["case_id"] = f"team-{index:02}"
    files = {"workspace_assets.json": pack(assets), "workspace_init.json": pack(init)}
    provenance = {
        "oracle_commit": PIN, "python": "3.14.8", "flags": ["-I", "-B"],
        "python_executable_sha256": sha(Path(sys.executable).resolve().read_bytes()),
        "capture_tool": "tools/freeze_workspace_assets.py",
        "capture_tool_sha256": sha(Path(__file__).read_bytes()),
        "source_sha256": hashes,
        "fixtures": {name: {"bytes": len(raw), "sha256": sha(raw)} for name, raw in files.items()},
        "inventory": {"assets": 22, "launches": 44, "teams": 48, "agents": 13,
                      "defaults": 3, "checkpoints": 2, "example_assets": 7},
        "count_correction": "Source-validated original init matrix is 4*3*2*2=48, not the originally requested 96. Both empty-roster display-name variants remain separate rows.",
        "encoding": "Recursive $text indices into texts of line indices; concatenate without normalization; object keys unchanged.",
        "expectations": "Only unchanged pinned Python helpers; no Rust outputs. Exact original input index cycling/order retained. No path/time/UUID normalization.",
        "environment": "Clean PATH=/usr/bin:/bin LC_ALL=C.UTF-8 TMPDIR=<owned-run>; COVEN_EXAMPLES_DIR and COVEN_HOME absent. Historical cwd supplies all seven example assets; no host paths recorded. /tmp roots are literal input strings only.",
    }
    files["workspace_assets_provenance.json"] = encoded(provenance)
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", type=Path, required=True)
    parser.add_argument("--repository", type=Path, required=True,
                        help="repository containing pinned Git objects, not the import root")
    parser.add_argument("--emit", choices=["workspace_assets.json", "workspace_init.json",
                                         "workspace_assets_provenance.json"])
    args = parser.parse_args()
    files = capture(args.historical_root.resolve(strict=True), args.repository.resolve(strict=True))
    print("*** Begin Patch")
    for name, raw in files.items():
        if args.emit and name != args.emit:
            continue
        print(f"*** Add File: tests/retirement/fixtures/{name}")
        for line in raw.decode().splitlines():
            print("+" + line)
    print("*** End Patch")


if __name__ == "__main__":
    main()
