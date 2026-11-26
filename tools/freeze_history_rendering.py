#!/usr/bin/env python3
"""Observe the pinned historical helper; emit data only, never rendering logic."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PIN = "ca3b19d87c955d6415231b1c4ebbbc591195a105"
HELPER = "src/workspace_commands/history/oracle.py"
PARENTS = ["src/workspace_commands/history.rs", "src/workspace_commands/history/tests.rs"]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path,
                        help="Existing owned capture directory outside the checkout")
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    assert output.is_dir() and not output.is_relative_to(project)
    assert Path(os.environ["TMPDIR"]).resolve(strict=True).is_relative_to(output)
    assert subprocess.check_output(["git", "rev-parse", f"{PIN}^{{commit}}"], cwd=project).decode().strip() == PIN
    modules = subprocess.check_output([
        "git", "ls-tree", "-r", "--name-only", PIN, "runner",
    ], cwd=project).decode().splitlines()
    modules = [path for path in modules if path.endswith(".py")]
    assert len(modules) == 25
    paths = [*PARENTS, HELPER, *modules]
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    # Ignore tracked bytecode and reject extra importable modules in the export.
    assert sorted(str(path.relative_to(root)) for path in (root / "runner").rglob("*.py")) == sorted(modules)
    assert not list((root / "runner").rglob("*.pyc"))
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
                   "TMPDIR": os.environ["TMPDIR"]}
    with tempfile.TemporaryDirectory(prefix="history-observation-", dir=environment["TMPDIR"]) as cwd:
        captures = [subprocess.check_output(
            [sys.executable, "-I", "-B", str(root / HELPER)], cwd=cwd, env=environment,
        ) for _ in range(2)]
    raw = captures[0]
    assert captures[1] == raw
    assert raw.endswith(b"\n") and raw.count(b"\n") == 232
    lines = raw.decode("ascii").splitlines(keepends=True)
    inventory = []
    for index, line in enumerate(lines):
        row = json.loads(line)
        record = index < 225
        assert set(row) == ({"record", "events", "messages"} if record else {"raw", "text"})
        assert isinstance(row["record"], dict) if record else row["raw"] is None or isinstance(row["raw"], str)
        inventory.append(digest(line.encode("ascii")))
    # The original 14 values x 16 field/default/fallback combinations, plus {}.
    assert json.loads(lines[0])["record"] == {}
    assert len(str(json.loads(lines[193])["record"]["ts"])) == 101
    assert [json.loads(line)["raw"] for line in lines[225:]] == [
        None, "", "\r", "\r\n", "a\rb\r\nc\n", "\n\n", "\x00\x1b\U0001f98a\u2028\u0085",
    ]
    corpus = dict(schema_version=1, jsonl_lines=lines)
    corpus_bytes = encoded(corpus)
    proof = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                 interpreter=dict(version=sys.version, executable=sys.executable,
                                  sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                 capture_tool_sha256=digest(Path(__file__).read_bytes()),
                 corpus_sha256=digest(corpus_bytes), stdout_sha256=digest(raw), stdout_bytes=len(raw),
                 repeat_capture_equal=True, record_rows=225, raw_rows=7, pretty_outputs=450, raw_outputs=7,
                 row_sha256=inventory, case_ids="Zero-based row indices 0..231; 0..224 record/events/messages, 225..231 raw/text",
                 comparison="Unmodified complete historical stdout JSONL, including its final LF; no deduplication, normalization, bindings or native outputs. Each line is parsed independently with LegacySnapshot by Rust; outer JSON never decodes a record.",
                 environment={**environment, "TMPDIR": "<owned output directory>/tmp"},
                 source="Runner-dependent historical oracle despite its former independent_python test name; all 25 runner modules and helper/Rust parents verified against Git before execution.")
    # These are capture artifacts, not checkout edits. Apply generated data with apply_patch.
    for name, data in [("history_rendering.json", corpus_bytes),
                       ("history_rendering_provenance.json", encoded(proof))]:
        (output / name).write_bytes(data)
        print(json.dumps(dict(path=name, bytes=len(data), sha256=digest(data))))


if __name__ == "__main__":
    main()
