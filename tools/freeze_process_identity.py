#!/usr/bin/env python3
"""Observation-only capture from an explicit historical Git export, never native code."""
import argparse
import contextlib
import hashlib
import inspect
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile

PIN = "d6515b0ada93036b28869ed1b9ed60633a2c1bbf"
HELPERS = ["src/process/identity/tests/oracle.py", "src/process/identity/tests/which_oracle.py"]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    assert root != project and not (root / ".git").exists()
    tracked = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                       "runner", "src/process/identity.rs", "src/process/identity",
                                       "src/platform.rs", "src/platform"], cwd=project).decode().splitlines()
    pins = {}
    for path in tracked:
        reference = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == reference, path
        pins[path] = digest(reference)
    assert all(path in pins for path in HELPERS)
    assert os.confstr("CS_PATH") == os.defpath == "/bin:/usr/bin"
    sys.path.insert(0, str(root))

    def oracle(helper, name, path):
        sys.argv = [str(root / helper), str(name)]
        if path is None:
            os.environ.pop("PATH", None)
        else:
            os.environ["PATH"] = path
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            runpy.run_path(str(root / helper), run_name="__main__")
        return json.loads(output.getvalue())

    cases = [
        ("first-entry", "identity-corpus-tool", "$REL/first:$REL/second", "$REL/first/identity-corpus-tool"),
        ("reversed-order", "identity-corpus-tool", "$REL/second:$REL/first", "$REL/second/identity-corpus-tool"),
        ("empty-leading", "identity-corpus-tool", ":$REL/first", "$REL/first/identity-corpus-tool"),
        ("empty-trailing", "identity-corpus-tool", "$REL/first:", "$REL/first/identity-corpus-tool"),
        ("empty-only-segments", "identity-corpus-tool", ":", None),
        ("absolute-direct", "$ROOT/first/identity-corpus-tool", "", "$ROOT/first/identity-corpus-tool"),
        ("dot-direct", "./$REL/first/identity-corpus-tool", "", "./$REL/first/identity-corpus-tool"),
        ("repeated-separators", "identity-corpus-tool", "$REL//first", "$REL//first/identity-corpus-tool"),
        ("nonexec-before-hit", "identity-corpus-tool", "$REL/nonexec:$REL/first", "$REL/first/identity-corpus-tool"),
        ("directory-before-hit", "identity-corpus-tool", "$REL/directory:$REL/first", "$REL/first/identity-corpus-tool"),
        ("symlink", "$REL/alias", "", "$REL/alias"),
        ("dangling", "$REL/dangling", "", None),
    ]
    with tempfile.TemporaryDirectory(prefix="process-identity-") as temporary:
        work = Path(temporary)
        old_cwd = Path.cwd()
        try:
            os.chdir(work)
            fixture = work / "fixture"
            fixture.mkdir()
            tool = fixture / "tool"
            tool.write_bytes(bytes(index % 256 for index in range(200_000)))
            tool.chmod(0o700)
            raw_identity = oracle(HELPERS[0], tool, "")
            info = tool.lstat()
            assert raw_identity == dict(device=info.st_dev, inode=info.st_ino, size=200_000,
                                        mtime_ns=info.st_mtime_ns, sha256=hashlib.sha256(tool.read_bytes()).hexdigest())
            identity = dict(size=200_000, sha256=raw_identity["sha256"])
            for directory in ("first", "second", "nonexec", "directory"):
                (fixture / directory).mkdir()
            for directory in ("first", "second", "nonexec"):
                node = fixture / directory / "identity-corpus-tool"
                node.write_bytes(b"abc")
                node.chmod(0o600 if directory == "nonexec" else 0o700)
            (fixture / "directory/identity-corpus-tool").mkdir()
            (fixture / "alias").symlink_to("first/identity-corpus-tool")
            (fixture / "dangling").symlink_to("absent")
            expand = lambda text: text.replace("$ROOT", str(fixture)).replace("$REL", "fixture") if text is not None else None
            captured = []
            for case_id, name, path, expected in cases:
                observed = oracle(HELPERS[1], expand(name), expand(path))
                assert observed == expand(expected), (case_id, observed, expected)
                captured.append(dict(id=case_id, name=name, path=path, expected=expected))
            original = []
            for name, path, expected in [
                ("identity-corpus-tool", "fixture/first", "fixture/first/identity-corpus-tool"),
                ("identity-corpus-tool", "missing-identity-bin:fixture/first", "fixture/first/identity-corpus-tool"),
                ("identity-corpus-tool", "", None),
                ("fixture/first/identity-corpus-tool", "", "fixture/first/identity-corpus-tool"),
                ("identity-missing-tool", None, None),
                ("sh", None, shutil.which("sh", path="/bin:/usr/bin")),
            ]:
                observed = oracle(HELPERS[1], name, path)
                assert observed == expected
                original.append(dict(name=name, path=path, observed=observed))
        finally:
            os.chdir(old_cwd)

    corpus = dict(schema_version=1, identity=identity, which_cases=captured,
                  leaf_bindings={"/device": "fixture tool: symlink_metadata.dev() -> exact u64 integer",
                                 "/inode": "fixture tool: symlink_metadata.ino() -> exact u64 integer",
                                 "/mtime_ns": "fixture tool: i128(mtime()) * 1000000000 + i128(mtime_nsec())"})
    destination = project / "tests/retirement/fixtures/process_identity.json"
    destination.write_bytes(encoded(corpus))
    provenance = dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                      interpreter=dict(version=sys.version, executable=sys.executable,
                                       sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                      stdlib=dict(module=shutil.__file__, sha256=digest(Path(shutil.__file__).read_bytes()),
                                  which_source=inspect.getsource(shutil.which), access_check_source=inspect.getsource(shutil._access_check),
                                  confstr_CS_PATH=os.confstr("CS_PATH"), defpath=os.defpath),
                      capture_tool_sha256=digest(Path(__file__).read_bytes()), uid=os.getuid(), platform=sys.platform,
                      identity_proof="Historical helper whole five-field result equals independent lstat plus hashlib. Only three OS leaves omitted from frozen data; size/hash literal. No native results consumed.",
                      original_six_capture=original, corpus_sha256=digest(destination.read_bytes()),
                      path_binding="Only named $ROOT and $REL fixture slots in declared name/path/expected strings; no normalization of actual results.")
    provenance_path = project / "tests/retirement/fixtures/process_identity_provenance.json"
    provenance_path.write_bytes(encoded(provenance))
    fixtures = [dict(path=str(path.relative_to(project)), sha256=digest(path.read_bytes()))
                for path in (destination, provenance_path)]
    review = dict(schema_version=1, approval_status="pending_parent_review", historical_commit=PIN, mapping_delta=0,
                  retired_helpers={
                      HELPERS[0]: dict(kind="runner_oracle", sha256=pins[HELPERS[0]], fixtures=fixtures,
                                       replacement_tests=["rust::src/process/identity/tests.rs::legacy_python_oracle_matches_real_file_identity"]),
                      HELPERS[1]: dict(kind="independent_stdlib_helper", sha256=pins[HELPERS[1]], fixtures=fixtures,
                                       replacement_tests=["rust::src/process/identity/tests.rs::relative_path_directories_and_unset_default"])},
                  limitations=["Unix scope; native lookup deliberately rejects executable special files accepted by shutil.which; no universal parity claim.",
                               "Mode-0600 regular file is nonexecutable even for root; no portable directory permission-denial claim. Capture UID recorded.",
                               "Unset PATH uses first eligible node in finite /bin:/usr/bin table, observed directly with stat and access, never a frozen host shell layout.",
                               "Global audit expected to fail until parent reviews and registers retirement of which_oracle.py; no global ledger edits here."],
                  validation="Pending focused native-only run and independent parent review.")
    (project / "tests/retirement/reviews/process_identity_oracles.json").write_bytes(encoded(review))
    print(json.dumps(dict(cases=len(captured), fixture_sha256=digest(destination.read_bytes()), source_pins=len(pins))))


if __name__ == "__main__":
    main()
