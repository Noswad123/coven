#!/usr/bin/env python3
"""Capture only the explicitly exported historical worker unit oracle."""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

PIN = "6e92c180ca19fed05950bdb08ea8a3e7b10f5526"
HELPER = "src/workers/oracle.py"
CASES = ["complete", "version", "missing", "missing-failure", "missing-parse"]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    paths = subprocess.check_output([
        "git", "ls-tree", "-r", "--name-only", PIN, "runner", "examples",
        "src/workers.rs", "src/workers", "src/process.rs", "src/process/identity.rs",
    ], cwd=project).decode().splitlines()
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert HELPER in pins and len([p for p in pins if p.startswith("runner/") and p.endswith(".py")]) == 25

    # Data frames mirror the existing Rust vector construction, not parser logic.
    vectors = []
    for route in [[], ["session"], ["session", "run"]]:
        lines = ["Usage: djinn " + " ".join([*route, "[OPTIONS]"]), "", "Commands:"]
        lines += ["  " + name + "  Description" for name in ["session", "ask", "session"]]
        lines += ["", "Options:"]
        lines += ["  " + name + "  Description" for name in ["--help", "--version", "--help"]]
        good = "\n".join([*lines, ""])
        for text in [good, good[:-1], good.replace("Usage:", "\x1b[1mUsage:\x1b[0m"),
                     good.replace("  --version  Description", "          --version is prose")]:
            vectors.append(dict(data=list(text.encode()), route=route))
    source = (root / "src/workers/tests.rs").read_text()
    explicit = source.split("fn help_parser_matches_legacy_oracle_vectors()", 1)[1].split("for text in [", 1)[1].split("] {", 1)[0]
    texts = [ast.literal_eval(line.strip().removesuffix(",")) for line in explicit.splitlines() if line.strip()]
    assert len(texts) == 18
    vectors += [dict(data=list(text.encode()), route=[]) for text in texts]
    vectors.append(dict(data=[255], route=[]))
    assert len(vectors) == 31
    env = {key: value for key, value in os.environ.items() if key not in {"COVEN_HOME", "COVEN_COMMAND"}}

    def oracle(arguments, data=None):
        return subprocess.check_output([sys.executable, "-I", "-B", str(root / HELPER), *arguments],
                                       input=data, env=env, cwd=root)

    parser_raw = oracle(["--parse"], json.dumps(vectors).encode())
    parsed = json.loads(parser_raw)
    assert len(parsed) == len(vectors)
    parser_cases = [dict(input=item, expected=parsed[i]) for i, item in enumerate(vectors)]
    captures, reports = [], []
    with tempfile.TemporaryDirectory(prefix="worker-capture-") as temporary:
        workspace = Path(temporary).resolve()
        executable = workspace / "fixture-é\n"
        payload = b"#!/bin/sh\nexit 99\n"
        executable.write_bytes(payload)
        executable.chmod(0o700)
        info = executable.lstat()
        for case in CASES:
            raw = oracle([str(workspace), str(executable), case])
            record = json.loads(raw)
            template = json.loads(raw)
            ledger = []

            def slot(pointer, binding, expected):
                parts = pointer.strip("/").split("/")
                node = template
                for part in parts[:-1]:
                    node = node[int(part)] if isinstance(node, list) else node[part]
                key = int(parts[-1]) if isinstance(node, list) else parts[-1]
                assert node[key] == expected, (pointer, node[key], expected)
                ledger.append(dict(pointer=pointer, binding=binding, captured=expected))
                node[key] = None

            slot("/report/context/workspace", "workspace", str(workspace))
            for field in ["selected_path", "resolved_path"]:
                slot(f"/report/executable/{field}", "executable", str(executable))
            for identity in ["identity", "identity_after"]:
                assert record["report"]["executable"][identity]["size"] == len(payload) == 18
                assert record["report"]["executable"][identity]["sha256"] == digest(payload)
                for field, value in [("device", info.st_dev), ("inode", info.st_ino), ("mtime_ns", info.st_mtime_ns)]:
                    slot(f"/report/executable/{identity}/{field}", field, value)
            for i, probe in enumerate(record["report"]["probes"]):
                slot(f"/report/probes/{i}/argv/0", "executable", str(executable))
                assert re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,6})?\+00:00", probe["observed_at"])
            # The printable-ASCII root is deliberately a fixture precondition.
            assert all(32 <= ord(c) < 127 for c in str(workspace))
            segment = str(workspace) + "/fixture-é\\u000a"
            assert record["human"].count(segment) == 1
            before, after = record["human"].split(segment)
            template["human"] = [before, after]
            # Rehydrate every declared slot and prove the entire raw record roundtrips.
            proof = json.loads(json.dumps(template))
            for entry in ledger:
                parts = entry["pointer"].strip("/").split("/")
                node = proof
                for part in parts[:-1]:
                    node = node[int(part)] if isinstance(node, list) else node[part]
                node[int(parts[-1]) if isinstance(node, list) else parts[-1]] = entry["captured"]
            proof["human"] = before + segment + after
            assert proof == record
            reports.append(dict(id=case, expected=template, bindings=ledger))
            captures.append(dict(id=case, stdout=raw.decode(), stdout_sha256=digest(raw), record=record,
                                 human_segment=segment, roundtrip_equal=True, binding_slots=len(ledger)))
    corpus = dict(schema_version=1, parser=parser_cases, reports=reports,
                  observed_at_exception="/report/probes/*/observed_at only; required valid UTC timestamp before removal",
                  human_binding="Printable ASCII canonical runtime root + literal /fixture-é\\u000a; exactly one segment")
    destination = project / "tests/retirement/fixtures/worker_readiness.json"
    destination.write_bytes(encoded(corpus))
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      interpreter=dict(version=sys.version, executable=sys.executable,
                                       sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      capture_tool_sha256=digest(Path(__file__).read_bytes()),
                      parser_stdout=parser_raw.decode(), parser_stdout_sha256=digest(parser_raw),
                      report_captures=captures, corpus_sha256=digest(destination.read_bytes()),
                      parser_accepted=sum(item is not None for item in parsed),
                      parser_rejected=sum(item is None for item in parsed),
                      fixture=dict(size=len(payload), sha256=digest(payload)),
                      proof="Whole decoded raw records rehydrated from templates using only explicit ledger slots and one declared human segment. No native output consumed.")
    provenance_path = project / "tests/retirement/fixtures/worker_readiness_provenance.json"
    provenance_path.write_bytes(encoded(provenance))
    fixtures = [dict(path=str(path.relative_to(project)), bytes=path.stat().st_size, sha256=digest(path.read_bytes()))
                for path in [destination, provenance_path]]
    review = dict(schema_version=1, approval_status="pending_parent_review", historical_commit=PIN, mapping_delta=0,
                  retired_helpers={HELPER: dict(kind="runner_oracle", sha256=pins[HELPER], fixtures=fixtures,
                      replacement_tests=["rust::src/workers/tests.rs::help_parser_matches_legacy_oracle_vectors",
                                         "rust::src/workers/tests.rs::python_oracle_matches_complete_reports_and_exact_human_renderer"])},
                  scope="Only two unit-test constructors. Independent src/workers/fixture.py and tests/workers_readiness_native.rs remain active; independent registry unchanged (4+1=5).",
                  bindings="Exact pointer ledger checked before binding; workspace via canonicalize, executable from declared fixture, both identity objects via direct symlink_metadata integers. Literal size/hash remain frozen. Human root restricted to printable ASCII, Unicode/newline suffix unchanged.",
                  comparison="Whole actual JSON unchanged except the explicit legacy probes[i].observed_at removal; actual timestamps separately validated. Exact human and exit code.",
                  consumer_closure="Pending scoped replacement and helper deletion; historical regeneration requires explicit pinned Git export, never active fallback.",
                  validation="Pending full workers::tests:: normal and strong Python-filesystem-unavailable runs; no original mapping or policy approval claimed.")
    (project / "tests/retirement/reviews/worker_readiness_oracles.json").write_bytes(encoded(review))
    print(json.dumps(dict(fixtures=fixtures, source_pins=len(pins), parser_accepted=provenance["parser_accepted"],
                          parser_rejected=provenance["parser_rejected"], report_cases=len(reports),
                          binding_slots=[len(r["bindings"]) for r in reports])))


if __name__ == "__main__":
    main()
