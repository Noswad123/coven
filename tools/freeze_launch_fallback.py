#!/usr/bin/env python3
"""Manual historical capture only: Python 3.14.8 -I -B, explicit Git export.

No launch implementation is copied. Executes only the pinned inert Rust child.
"""
import argparse
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PIN = "d6515b0ada93036b28869ed1b9ed60633a2c1bbf"
SENTINEL = "__COVEN_FALLBACK_CWD__"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def slots(value):
    """Enumerate only schema-defined CWD leaves, never search arbitrary strings."""
    result = ["/plan/root", "/plan/cwd"]
    for i, step in enumerate(value["plan"]["steps"]):
        argv = step.get("argv", [])
        index = cwd_index(argv)
        if index is not None:
            result.append(f"/plan/steps/{i}/argv/{index}")
    for i, call in enumerate(value["calls"]):
        result.append(f"/calls/{i}/cwd")
        index = cwd_index([value["plan"]["multiplexer"], *call["args"]])
        if index is not None:
            result.append(f"/calls/{i}/args/{index - 1}")
    return result


def cwd_index(argv):
    if argv[:2] in (["tmux", "new-session"], ["tmux", "new-window"]):
        assert argv[7] == "-c"
        return 8
    if argv[:3] == ["herdr", "workspace", "create"]:
        assert argv[3] == "--cwd"
        return 4
    if argv[:3] == ["herdr", "tab", "create"]:
        assert argv[5] == "--cwd"
        return 6
    return None


def leaf(value, pointer):
    parts = pointer.split("/")[1:]
    for part in parts[:-1]:
        value = value[int(part)] if isinstance(value, list) else value[part]
    return value, int(parts[-1]) if isinstance(value, list) else parts[-1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--rustc", type=Path, required=True)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.oracle_root.resolve()
    assert root != project
    tracked = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, "runner"], cwd=project).decode().splitlines()
    paths = [p for p in tracked if p.endswith(".py")]
    assert len(paths) == 25
    paths += ["src/launch.rs", "src/launch/tests.rs", "src/launch/executable.rs", "src/launch/executable/tests.rs",
              "src/launch/executable/fallback.rs", "src/launch/executable/fixture.rs",
              "src/launch/executable/native_oracle.py"]
    hashes = {}
    for path in paths:
        pinned = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == pinned, path
        hashes[path] = digest(pinned)
    sys.path.insert(0, str(root))
    from runner import launch
    records = []
    rustc = args.rustc.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="launch-fallback-") as temporary:
        work = Path(temporary)
        executable = work / "compiled-inert-backend"
        subprocess.run([str(rustc), "--edition=2024", "-O", str(root / "src/launch/executable/fixture.rs"), "-o", str(executable)], check=True)
        for backend in ("tmux", "herdr"):
            for stopping in (False, True):
                for first in ("missing-interpreter", "missing-file", "exit127"):
                    case_id = f"{backend}-{'stop' if stopping else 'launch'}-{first}"
                    cwd = work / case_id / ("workspace-stop" if stopping else "workspace-launch")
                    bad = cwd / first / backend
                    good = cwd / "compiled fixture bin" / backend
                    bad.parent.mkdir(parents=True)
                    good.parent.mkdir()
                    if first == "missing-interpreter":
                        bad.write_bytes(b"#!/nonexistent-coven-inert-interpreter\n")
                        bad.chmod(0o700)
                    elif first == "exit127":
                        bad.write_bytes(executable.read_bytes())
                        bad.chmod(0o700)
                    good.write_bytes(executable.read_bytes())
                    good.chmod(0o700)
                    spec = dict(namespace="coven-a-b", multiplexer=backend,
                                agents=[dict(id="x", command="exec worker 'literal'", starter="- $HOME ; literal")],
                                environment=dict(HOME="/frozen", PATH=f"{first}:compiled fixture bin"))
                    plan = launch.build_plan(str(cwd), **spec)
                    output = subprocess.check_output([sys.executable, "-I", "-B", str(root / "src/launch/executable/native_oracle.py"), str(root)],
                                                     input=encoded(dict(plan=plan, stopping=stopping)))
                    raw = dict(plan=plan, **json.loads(output))
                    frozen = copy.deepcopy(raw)
                    pointers = slots(raw)
                    for pointer in pointers:
                        parent, key = leaf(frozen, pointer)
                        assert parent[key] == str(cwd), pointer
                        parent[key] = SENTINEL
                    restored = copy.deepcopy(frozen)
                    for pointer in pointers:
                        parent, key = leaf(restored, pointer)
                        assert parent[key] == SENTINEL
                        parent[key] = str(cwd)
                    assert encoded(restored) == encoded(raw)
                    # Root-dependent diagnostics outside the allowlist must fail capture.
                    assert str(work) not in encoded(frozen).decode()
                    records.append(dict(id=case_id, backend=backend, stopping=stopping, first=first,
                                        inputs=spec, expected=frozen, cwd_slots=pointers, capture_cwd=str(cwd),
                                        raw_base64=base64.b64encode(encoded(raw)).decode(), raw_sha256=digest(encoded(raw)),
                                        oracle_stdout_base64=base64.b64encode(output).decode(), oracle_stdout_sha256=digest(output),
                                        slot_ledger_sha256=digest(encoded(pointers))))
        fixture_binary_hash = digest(executable.read_bytes())
    corpus = dict(schema=1, sentinel=SENTINEL, cases=records)
    destination = project / "tests/retirement/fixtures/launch_fallback.json"
    destination.write_bytes(encoded(corpus))
    review = dict(schema=1, historical_commit=PIN, source_sha256=hashes,
                  capture_environment=dict(os.environ),
                  capture_tool_sha256=digest(Path(__file__).read_bytes()),
                  interpreter=dict(version=sys.version, executable=sys.executable, sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                  rustc=dict(path=str(rustc), version=subprocess.check_output([str(rustc), "--version"]).decode(), sha256=digest(rustc.read_bytes())),
                  compiled_fixture_sha256=fixture_binary_hash, corpus_sha256=digest(destination.read_bytes()),
                  case_ids=[r["id"] for r in records], case_count=12,
                  parameterization="Only enumerated CWD leaves; actual native values are never normalized. All other fields literal.",
                  reversible_proof="Restore capture_cwd at every sentinel slot; canonical bytes equal raw_base64 and raw_sha256. Original helper stdout retained separately.",
                  mapping_delta=0, original_mapping=dict(total=941, mapped=90, unresolved=851))
    (project / "tests/retirement/reviews/launch_fallback_oracle.json").write_bytes(encoded(review))
    print(json.dumps(dict(cases=len(records), corpus_sha256=review["corpus_sha256"])))


if __name__ == "__main__":
    main()
