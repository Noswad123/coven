"""Optional historical Python capture; never used by native tests.

Run Python 3.14.8 -I -B with --oracle-root pointing to an explicit export of
8e41fe1. After helper retirement, the current checkout is NOT a capture root.
Generated data files are written only with the explicit --capture option.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


COMMIT = "8e41fe10ab041b4355205688e3458d5b8906c80c"
HELPER = "tests/_checkpoint_parity.py"
PARENT = "tests/checkpoint_decisions.rs"
PINS = {
    HELPER: "c86dfd33e8f08ed24b82778471031d24e132e64ddbbe89fa073a3e9fff21419c",
    PARENT: "5e53c80333f6a9f13eeeb7eff3c64d9bccf11e632636a09c44ad8d59c0378875",
}
FIXTURE = "tests/retirement/fixtures/checkpoint_inspection.json"
PROVENANCE = "tests/retirement/fixtures/checkpoint_inspection_provenance.json"
REVIEW = "tests/retirement/reviews/checkpoint_inspection_oracle.json"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def pretty(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


def capture(root):
    repo = Path(__file__).resolve().parents[1]

    def git(*args):
        return subprocess.run(["git", "-C", str(repo), *args], check=True,
                              capture_output=True).stdout

    if git("rev-parse", COMMIT + "^{commit}").decode().strip() != COMMIT:
        raise ValueError("historical commit unavailable")
    # All 25 runner Python modules conservatively close over local transitive imports.
    modules = [name for name in git("ls-tree", "-r", "--name-only", COMMIT, "runner").decode().splitlines()
               if name.endswith(".py")]
    if len(modules) != 25:
        raise ValueError("re-review historical transitive source closure")
    hashes = {}
    for name in [HELPER, PARENT, *modules]:
        original = git("show", f"{COMMIT}:{name}")
        raw = (root / name).read_bytes()
        if raw != original or (name in PINS and sha(raw) != PINS[name]):
            raise ValueError(f"changed historical source: {name}")
        hashes[name] = sha(raw)
    env = {"PATH": "/usr/bin:/bin", "LC_ALL": "C.UTF-8"}
    result = subprocess.run([sys.executable, "-I", "-B", str(root / HELPER)],
                            cwd=root, env=env, check=True, capture_output=True)
    if result.stderr:
        raise ValueError(f"unexpected oracle stderr: {result.stderr!r}")
    rawrows = result.stdout.splitlines(keepends=True)
    if any(not row.endswith(b"\n") or row == b"\n" for row in rawrows):
        raise ValueError("unexpected JSONL framing")
    inventory, seen, duplicates = [], {}, {}
    counts = {"accepted": 0, "rejected": 0, "resource_refusals": 0}
    for index, rawrow in enumerate(rawrows):
        row = json.loads(rawrow)
        identity = row["id"]
        if identity in seen:
            duplicates.setdefault(identity, [seen[identity]]).append(index)
        else:
            seen[identity] = index
        counts["accepted" if row["accepted"] else "rejected"] += 1
        counts["resource_refusals"] += "expectednative_error" in row
        source = None if row["rawbyteshex"] is None else bytes.fromhex(row["rawbyteshex"])
        inventory.append({"index": index, "id": identity, "case_id": f"{index}:{identity}",
                          "row_sha256": sha(rawrow),
                          "input_sha256": None if source is None else sha(source),
                          "input_bytes": None if source is None else len(source)})
    if not (counts["accepted"] > 1500 and counts["rejected"] > 100
            and counts["resource_refusals"] == 12):
        raise ValueError(f"incomplete corpus: {counts}")
    # Keep each original serialized row byte-for-byte; only array framing is added.
    corpus = b"[\n" + b",\n".join(row[:-1] for row in rawrows) + b"\n]\n"
    provenance = {
        "schema_version": 1, "oracle_commit": COMMIT,
        "oracle_root": str(root), "reference_files_match_commit": True,
        "source_sha256": hashes, "conservative_runner_closure": modules,
        "capture_tool_sha256": sha(Path(__file__).read_bytes()),
        "interpreter": {"version": sys.version, "path": sys.executable,
                        "sha256": sha(Path(sys.executable).resolve().read_bytes()),
                        "isolated": bool(sys.flags.isolated), "no_bytecode": sys.dont_write_bytecode},
        "oracle_environment": env, "oracle_flags": ["-I", "-B"],
        "capture_command": "env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 /usr/bin/python3 -I -B tools/freeze_checkpoint_inspection.py --oracle-root <explicit-historical-export> --capture",
        "historical_source": {"path": HELPER, "commit": COMMIT, "active_consumer": False},
        "corpus": {"path": FIXTURE, "bytes": len(corpus), "sha256": sha(corpus),
                   "jsonl_sha256": sha(result.stdout), "rows": len(rawrows), **counts},
        "inventory_sha256": sha(json.dumps(inventory, sort_keys=True, ensure_ascii=False,
                                           separators=(",", ":")).encode()), "inventory": inventory,
        "historical_duplicate_ids": duplicates, "unique_original_ids": len(seen),
        "encoding": "JSON array with one untouched original JSONL row per physical line; parse each row using values::parse Authority, never the whole manifest. rawbyteshex preserves encoded record payloads exactly, including resource vectors. No deduplication, narrowing, sampling, or native-derived expectations.",
    }
    provenance_raw = pretty(provenance)
    review = {
        "schema_version": 1,
        "scope": "Checkpoint inspection integration oracle retirement only; original 941-method mapping remains 90/851, with zero new mapping claims.",
        "native_test": "rust::tests/checkpoint_decisions.rs::python_checkpoint_inspection_parity",
        "historical_helper": {"path": HELPER, "commit": COMMIT, "sha256": hashes[HELPER]},
        "original_parent_sha256": hashes[PARENT],
        "consumer_closure": "Global tracked/unignored content search before deletion found only tests/checkpoint_decisions.rs. Capture tool refers to historical source explicitly, not an active helper consumer.",
        "assertion_review": [
            "Every original fixture, ID and order preserved; historical duplicate IDs explicitly inventoried, with unique index-qualified case_id identities. Full accepted canonical view equality through values Authority, without serde_json numeric coercion.",
            "Every rejection still requires invalid_checkpoint error family unless one of the 12 explicit resource refusals, which retain exact expectednative_error equality.",
            "Original accepted >1500, rejected >100 and resource ==12 assertions retained; exact counts, corpus/JSONL/inventory and per-row/raw-input SHA-256 checks added.",
            "Complete approve/reject semantic-span bit flips, Markdown lists/fences/headings/checkbox matrices, history ordering/modification, all record types/fields/encodings, duplicate/nonfinite JSON, Unicode/control/markers/notes/identifiers retained, including semantically duplicate cases with different IDs.",
        ],
        "fixtures": [{"path": FIXTURE, "sha256": sha(corpus)},
                     {"path": PROVENANCE, "sha256": sha(provenance_raw)}],
        "corpus": provenance["corpus"],
        "recapture": "Requires explicit historical export containing pinned helper and all 25 runner Python sources. Current checkout after deletion is intentionally unusable. No copied constructor in tools.",
    }
    for path, data in [(FIXTURE, corpus), (PROVENANCE, provenance_raw), (REVIEW, pretty(review))]:
        (repo / path).write_bytes(data)
    print(json.dumps(provenance["corpus"], sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    parser.add_argument("--capture", action="store_true")
    args = parser.parse_args()
    if not args.capture:
        parser.error("pass --capture to execute the pinned historical oracle and generate data")
    if sys.version_info[:3] != (3, 14, 8) or not sys.flags.isolated or not sys.dont_write_bytecode:
        parser.error("requires Python 3.14.8 -I -B")
    if dict(os.environ) != {"PATH": "/usr/bin:/bin", "LC_ALL": "C.UTF-8"}:
        parser.error("requires clean environment: env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8")
    capture(args.oracle_root.resolve(strict=True))


if __name__ == "__main__":
    main()
