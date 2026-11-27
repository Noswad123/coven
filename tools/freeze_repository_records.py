#!/usr/bin/env python3
"""Capture admission booleans from an explicit, verified historical Git export."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PIN = "b97823c7f4f30e53f768258347eaf66daec74053"
HELPER = "src/repositories/oracle.py"
PARENTS = ["src/repositories/oracle_tests.rs", "src/repositories/tests.rs",
           "src/repositories.rs", "src/repositories/record.rs", "src/repositories/timestamp.rs"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode()


def inputs():
    # Independently transcribed literal fixture declaration, not Authority::decode output.
    original = dict(schema_version=1, kind="repository-registration",
                    registration_id="12345678-1234-1234-1234-123456789abc",
                    canonical_root="/deleted", git_dir="/deleted/.git", common_dir="/deleted/.git",
                    filesystem=dict(root=dict(device=1, inode=2), git_dir=dict(device=1, inode=3),
                                    common_dir=dict(device=1, inode=3)),
                    object_format="sha1", base_commit="a" * 40, base_tree="b" * 40,
                    delivery_target="refs/heads/result", registered_at="2026-11-02T12:00:00+00:00",
                    original_request=dict(path="/deleted", base="a" * 40,
                                          delivery_target="refs/heads/result", expected_revision="repository-unregistered:v1"))
    def raw(value):
        return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    corpus = [raw(original)]
    for key, value in [
        ("schema_version", True), ("schema_version", 1.0), ("extra", 1),
        ("object_format", "sha512"), ("registered_at", "2026-02-30T12:00:00Z"),
        ("registered_at", "2026-11-02T12:00:00+01:00"), ("registered_at", "2026-11-02T12:00:00Z"),
        ("registered_at", "2026-11-02T12:00:00xZ"), ("registered_at", "2026-11-02T12:00:00.123xZ"),
        ("registered_at", "2026-11-02T12:00:00Z\0junk"), ("registered_at", "2026-11-02T12x+00:00"),
        ("canonical_root", "/a//b"), ("canonical_root", "relative"),
    ]:
        record = copy.deepcopy(original)
        record[key] = value
        corpus.append(raw(record))
    for inode in [True, 1.0, -1, 0, 2**64 - 1]:
        record = copy.deepcopy(original)
        record["filesystem"]["root"]["inode"] = inode
        corpus.append(raw(record))
    corpus.append(corpus[0].replace('"inode":2', '"inode":18446744073709551616000000000000000000000000001'))
    for path in ["/deleted", "//deleted", "///deleted", "/deleted/.", "/deleted//", "/a/../deleted"]:
        for target in ["original_request", "canonical_root"]:
            record = copy.deepcopy(original)
            if target == "original_request":
                record[target]["path"] = path
            else:
                record[target] = path
            corpus.append(raw(record))
    dates = ["2026-11-02", "20261102", "2026-W45-1", "2026W451", "2026-W45", "2026W45",
             "2026-02-30", "0000-12-31", "9999-12-31"]
    separators = ["T", " ", "x", "\U0001f98a"]
    clocks = ["12", "1200", "12:00", "12:00:00", "120000", "12:00:00.123456789", "24:00:00", "25:00:00", "12:00:00.0"]
    zones = ["Z", "+00:00", "-0000", "+00:00:00.5", "+01:00", ""]
    for date in dates:
        for separator in separators:
            for clock in clocks:
                for zone in zones:
                    record = copy.deepcopy(original)
                    record["registered_at"] = date + separator + clock + zone
                    corpus.append(raw(record))
    corpus.extend(["null", "{}", '{"x":1,"x":2}'])
    assert len(corpus) == 1 + 13 + 5 + 1 + 6 * 2 + 9 * 4 * 9 * 6 + 3 == 1979
    assert all(type(value) is str for value in corpus)
    return corpus


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    assert output.is_dir() and not output.is_relative_to(project)
    assert Path(os.environ["TMPDIR"]).resolve(strict=True).is_relative_to(output)
    assert subprocess.check_output(["git", "rev-parse", f"{PIN}^{{commit}}"], cwd=project).decode().strip() == PIN
    modules = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, "runner"], cwd=project).decode().splitlines()
    modules = [path for path in modules if path.endswith(".py")]
    assert len(modules) == 25
    pins = {}
    for path in [HELPER, *PARENTS, *modules]:
        historical = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == historical, path
        pins[path] = digest(historical)
    assert sorted(str(path.relative_to(root)) for path in (root / "runner").rglob("*.py")) == sorted(modules)
    assert not list(root.rglob("*.pyc"))
    corpus = inputs()
    request = json.dumps(corpus, ensure_ascii=True).encode()
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    # Execute the historical helper unchanged; only its import root needs bootstrapping under -I.
    bootstrap = "import sys; sys.path.insert(0,sys.argv[1]); exec(compile(open(sys.argv[2],'rb').read(),sys.argv[2],'exec'))"
    with tempfile.TemporaryDirectory(prefix="repository-observation-", dir=environment["TMPDIR"]) as cwd:
        captures = [subprocess.check_output([sys.executable, "-I", "-B", "-c", bootstrap, str(root), str(root / HELPER)],
                                            input=request, cwd=cwd, env=environment) for _ in range(2)]
    assert captures[0] == captures[1]
    expected = json.loads(captures[0])
    assert type(expected) is list and len(expected) == len(corpus)
    assert all(type(value) is bool for value in expected)
    admitted = sum(expected)
    assert 0 < admitted < len(expected)
    rows = [dict(id=index, raw=raw, admitted=expected[index]) for index, raw in enumerate(corpus)]
    inventory = [dict(id=row["id"], raw_sha256=digest(row["raw"].encode())) for row in rows]
    data = encoded(dict(schema_version=1, cases=rows))
    proof = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                 interpreter=dict(version=sys.version, executable=sys.executable,
                                  sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                 capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(data),
                 request_sha256=digest(request), stdout_sha256=digest(captures[0]), stdout_bytes=len(captures[0]),
                 repeat_capture_equal=True, cases=1979, admitted=admitted, refused=1979-admitted,
                 matrix=dict(base=1, top_fields=13, inodes=5, raw_huge_integer=1, paths=6, path_targets=2,
                             dates=9, separators=4, clocks=9, zones=6, timestamps=1944, tail=3),
                 inventory=inventory,
                 comparison="IDs 0..1978 in authored order; duplicate inputs retained. Raw strings never parsed/reserialized. Literal bool responses from historical admit() and datetime.fromisoformat; no native observations or copied validation logic. Original corpus has no nonfinite constants.",
                 environment={**environment, "TMPDIR": "<owned output directory>/tmp"})
    for name, content in [("repository_records.json", data), ("repository_records_provenance.json", encoded(proof))]:
        (output / name).write_bytes(content)
        print(json.dumps(dict(path=name, bytes=len(content), sha256=digest(content))))
    print(json.dumps(dict(cases=len(corpus), admitted=admitted, refused=1979-admitted,
                          stdout_sha256=proof["stdout_sha256"])), file=sys.stderr)


if __name__ == "__main__":
    main()
