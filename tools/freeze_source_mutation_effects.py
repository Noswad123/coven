#!/usr/bin/env python3
"""Optional historical-only capture; never called by Rust tests.

Use Python 3.14.8 -I -B and an explicit git archive of PIN (without pyc).
Only metadata generation is fixed. Existing _apply interception is an observer
which calls the original reducer, not a business-logic replacement.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
BASE = "src/execution/mutation/"
HELPERS = [BASE + p + "oracle.py" for p in ("", "graphs/", "effects/")]
COUNTS = [2, 25, 23]


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def dump(value):
    return json.dumps(value, ensure_ascii=True, allow_nan=False)


# Run only archived sources. Determinism covers predecessor setup, not just the
# already-fixed operation under comparison. All changes are recorded verbatim.
BOOTSTRAP = '''import sys
from pathlib import Path
from datetime import datetime
from types import SimpleNamespace
from unittest.mock import patch
root, helper = Path(sys.argv[1]), sys.argv[2]
sys.path.insert(0, str(root))
from runner import task_attempts, execution_v2, coven
counter = 0
def uuid():
    global counter
    counter += 1
    return SimpleNamespace(hex=f"{counter:032x}")
now = datetime.fromisoformat("2026-11-02T01:02:03.123456+00:00")
source = (root / helper).read_text()
patches = __PATCHES__
for old, new in patches:
    assert source.count(old) == 1, old
    source = source.replace(old, new)
with patch.object(task_attempts.uuid, "uuid4", side_effect=uuid), \\
     patch.object(task_attempts, "datetime") as clock, \\
     patch.object(execution_v2, "datetime") as clock2:
    clock.now.return_value = now
    clock2.now.return_value = now
    exec(compile(source, str(root / helper), "exec"), {"__file__": str(root / helper), "coven": coven})
'''

STANDALONE_PATCH = (
    '        return {"control": control, "revision": revision, "attempt": attempt,',
    '''        persisted = (root / "state/execution/control.json").read_text()
        with storage.workspace_lock(root, root / "state/.workspace.lock") as held:
            retry = store.mutate("cancel", {"attempt_id": attempt, "reason": "  withdraw \\u03bb\\n"},
                expected_execution_revision=revision, expected_tasks_revision=None,
                request_id="cancel", source_loader=source, lock=held)
        assert (root / "state/execution/control.json").read_text() == persisted
        import io
        from contextlib import redirect_stdout
        def render(value, mode):
            output = io.StringIO()
            with redirect_stdout(output):
                status = coven.print_execution_result(value, mode)
            assert status == 0
            return output.getvalue().removesuffix("\\n")
        def native_render(value, mode):
            # Native compact CLI deliberately omits historical CLI spaces.
            return json.dumps(value, ensure_ascii=True, separators=(",", ":")) if mode else render(value, mode)
        return {"retry": retry,
                "python_rendered": [render(result, mode) for mode in (False, True)],
                "python_retry_rendered": [render(retry, mode) for mode in (False, True)],
                "rendered": [native_render(result, mode) for mode in (False, True)],
                "retry_rendered": [native_render(retry, mode) for mode in (False, True)],
                "control": control, "revision": revision, "attempt": attempt,''',
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", type=Path, required=True)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8)
    assert sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    assert not list(root.rglob("*.pyc"))
    paths = subprocess.check_output([
        "git", "ls-tree", "-r", "--name-only", PIN, "runner", "src/execution.rs",
        "src/execution/mutation.rs", "src/execution/mutation",
    ], cwd=project).decode().splitlines()
    paths = [p for p in paths if p.endswith((".py", ".rs"))]
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = sha(raw)
    assert sum(p.startswith("runner/") for p in pins) == 25
    assert all(p in pins and p.replace("oracle.py", "tests.rs") in pins for p in HELPERS)
    env = {"PATH": "/usr/bin", "LANG": "C.UTF-8", "TMPDIR": os.environ["TMPDIR"]}
    captures, patches = [], {}
    for helper, count in zip(HELPERS, COUNTS, strict=True):
        edits = [STANDALONE_PATCH] if helper == HELPERS[0] else []
        patches[helper] = edits
        bootstrap = BOOTSTRAP.replace("__PATCHES__", repr(edits))
        command = [sys.executable, "-I", "-B", "-c", bootstrap, str(root), helper]
        first = subprocess.run(command, cwd=root, env=env, capture_output=True)
        assert first.returncode == 0, first.stderr.decode()
        second = subprocess.run(command, cwd=root, env=env, capture_output=True)
        assert second.returncode == 0, second.stderr.decode()
        assert first.stdout == second.stdout and first.stderr == second.stderr == b""
        vectors = json.loads(first.stdout)
        assert len(vectors) == count
        rows = []
        for index, vector in enumerate(vectors):
            identity = (f"standalone-v{index + 1}" if helper == HELPERS[0] else
                        vector["label"] if helper == HELPERS[1] else
                        f'v{vector["schema"]}-{vector["case"]}')
            raw = dump(vector)
            assert raw.encode() in first.stdout
            inputs = {k: v for k, v in vector.items() if k not in
                      {"expected", "persisted", "retry", "rendered", "retry_rendered",
                       "python_rendered", "python_retry_rendered",
                       "effect", "result", "after", "accepted"}}
            request = dump(inputs)
            rows.append(dict(id=identity, raw=raw, sha256=sha(raw.encode()),
                             request=request, request_sha256=sha(request.encode())))
        assert len({r["id"] for r in rows}) == count
        captures.append(dict(helper=helper, count=count, rows=rows,
                             stdout=first.stdout.decode(), stdout_sha256=sha(first.stdout),
                             stdout_bytes=len(first.stdout), stderr="", exit_status=0,
                             repeated_identically=True))
    corpus = (dump(dict(schema_version=1, captures=captures)) + "\n").encode()
    proof = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                 capture_tool_sha256=sha(Path(__file__).read_bytes()),
                 interpreter=dict(version=sys.version, executable=sys.executable,
                                  sha256=sha(Path(sys.executable).read_bytes()), isolated=True,
                                  bytecode=False),
                 environment=env, bootstrap=BOOTSTRAP, exact_source_patches=patches,
                 metadata_seam="Outer uuid4 sequential 32-hex IDs; both module clocks fixed to NOW. Original inner UUID/time patches take precedence. No business logic mocked.",
                 corpus_sha256=sha(corpus), counts=COUNTS,
                 case_ids=[[row["id"] for row in c["rows"]] for c in captures],
                 proof="All sources verified before execution; each helper run twice identically. Complete raw stdout and rows retained. Opaque strings parsed only with native Authority profile. All predecessor controls independently produced by historical Python; no native output or encoder used.")
    destination = project / "tests/retirement/fixtures"
    for name, raw in [("source_mutation_effects.json", corpus),
                      ("source_mutation_effects_provenance.json", (dump(proof) + "\n").encode())]:
        (destination / name).write_bytes(raw)
        print(f"{name}: {len(raw)} bytes sha256={sha(raw)}")
    print(f"Verified {len(pins)} sources; captures {COUNTS}; repeated byte-identically")


if __name__ == "__main__":
    main()
