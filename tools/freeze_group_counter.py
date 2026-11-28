#!/usr/bin/env python3
"""Capture complete counter authority bytes from an explicit historical export."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PIN = "727edc7aad55ed76e98670e0cee6449d611b2285"
HELPER = "src/group_publisher/counter_oracle.py"
PREFIXES = ["runner", "src/group_publisher", "src/group_publisher.rs", "src/groups",
            "src/groups.rs", "src/kitsune_observation", "src/kitsune_observation.rs",
            "src/observation_socket", "src/observation_socket.rs", "src/observation_time.rs",
            "src/task_time.rs", "src/values", "src/values.rs"]
MEMBER = "/groups/0/members/0"
ENDPOINT = MEMBER + "/binding/presentation/endpoint"
UUIDS = ["/workspace_id", "/groups/0/group_id", MEMBER + "/participant_id"]
STAMP = MEMBER + "/binding/evidence/observed_at"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode("ascii")


def inventory(project):
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                     *PREFIXES], cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith(".rs") or p.endswith(".py"))
    assert HELPER in paths and "src/group_publisher/native_tests.rs" in paths
    assert len([p for p in paths if p.startswith("runner/")]) == 25
    return paths


def verify_sources(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    paths = inventory(project)
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in paths if p.startswith("runner/")]
    assert not list(root.rglob("*.pyc"))
    return pins


def initial(reordered, maximum, prior):
    # Authored whole authority, independent of native generation/output. Binding
    # objects follow the original serde-sorted producer; endpoint order is explicit.
    endpoint = dict(path="/group-counter-capture/socket", device=314159, inode=271828)
    if maximum:
        endpoint.update(device=18446744073709551615, inode=18446744073709551615)
    declaration = {k: endpoint[k] for k in (["inode", "path", "device"] if reordered else ["path", "device", "inode"])}
    state = dict(schema_version=1, workspace_id="11111111-1111-4111-8111-111111111111",
                 content_revision=2, groups=[dict(group_id="22222222-2222-4222-8222-222222222222",
                 key="team", name="Review Team", members=[dict(
                     participant_id="33333333-3333-4333-8333-333333333333", key="member", role="Reviewer",
                     binding=dict(evidence=dict(agent_kind_hint="test-agent", native_binding="unestablished",
                                               native_reference=None, observed_at="2026-11-27T12:34:56.123456+00:00",
                                               provenance="kitsune.session.snapshot"),
                                  presentation=dict(adapter="kitsune", endpoint=declaration,
                                                    pane_id_at_binding="p", terminal_id="terminal")))])],
                 publication_counters=[dict(endpoint={k: endpoint[k] for k in ["inode", "path", "device"]},
                                            source="coven" if prior else "other", epoch="epoch",
                                            watermarks=dict(terminal=18446744073709551614 if prior else 18446744073709551615))])
    return (json.dumps(state, ensure_ascii=True, indent=2) + "\n").encode("ascii")


def spans(raw):
    """Independent structural token spans; offsets include quotes, never formatting."""
    text = raw.decode("ascii")
    decoder = json.JSONDecoder()
    leaves = {}

    def walk(at, path):
        while text[at].isspace():
            at += 1
        if text[at] in "{[":
            obj = text[at] == "{"
            close = "}" if obj else "]"
            at += 1
            index = 0
            while True:
                while text[at].isspace() or text[at] == ",":
                    at += 1
                if text[at] == close:
                    return at + 1
                if obj:
                    key, at = decoder.raw_decode(text, at)
                    assert isinstance(key, str) and "/" not in key and "~" not in key
                    while text[at].isspace():
                        at += 1
                    assert text[at] == ":"
                    at += 1
                else:
                    key = str(index)
                at = walk(at, path + "/" + key)
                index += 1
        else:
            _, end = decoder.raw_decode(text, at)
            assert path not in leaves
            leaves[path] = (at, end)
            return end

    end = walk(0, "")
    assert text[end:] == "\n"
    return leaves


def bindings(raw, maximum, counters):
    paths = [(p, p) for p in [*UUIDS, STAMP]]
    for target in [ENDPOINT, *[f"/publication_counters/{i}/endpoint" for i in range(counters)]]:
        for key in (["path"] if maximum else ["path", "device", "inode"]):
            paths.append((target + "/" + key, ENDPOINT + "/" + key))
    tokens = spans(raw)
    slots = []
    for path, source in paths:
        start, end = tokens[path]
        literal = raw[start:end].decode("ascii")
        # Unique path guards, not global replacement of repeated literals.
        slots.append(dict(path=path, source=source, start=start, end=end, literal=literal))
    assert len({s["path"] for s in slots}) == len(slots)
    return slots


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    assert output.is_dir() and not output.is_relative_to(project)
    assert Path(os.environ["TMPDIR"]).resolve(strict=True).is_relative_to(output)
    pins = verify_sources(project, root)  # Complete verification before any helper import.
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8", TMPDIR=os.environ["TMPDIR"])
    rows = []
    for policy in ["primitive", "publisher"]:
        for reordered in [False, True]:
            for maximum in [False, True]:
                for prior in [False, True]:
                    before = initial(reordered, maximum, prior)
                    captures = []
                    for repeat in range(2):
                        with tempfile.TemporaryDirectory(prefix="counter-capture-", dir=environment["TMPDIR"]) as cwd:
                            workspace = Path(cwd)
                            (workspace / "state").mkdir()
                            (workspace / "coven.json").write_bytes(b"{}")
                            (workspace / "state/groups.json").write_bytes(before)
                            raw = subprocess.check_output([sys.executable, "-I", "-B", str(root / HELPER),
                                                           str(workspace), "coven", policy], cwd=cwd, env=environment)
                            assert raw == (workspace / "state/groups.json").read_bytes()
                            captures.append(raw)
                    assert captures[0] == captures[1] and captures[0].endswith(b"\n")
                    raw = captures[0]
                    request = dict(policy=policy, reordered_declaration=reordered, maximum_stats=maximum,
                                   prior_stream=prior, source="coven", before=before.decode("ascii"))
                    rows.append(dict(id=f"{policy}-r{int(reordered)}-m{int(maximum)}-p{int(prior)}",
                                     request=request, request_sha256=digest(encoded(request)),
                                     before_sha256=digest(before), stdout=raw.decode("ascii"), stdout_sha256=digest(raw),
                                     before_bindings=bindings(before, maximum, 1),
                                     stdout_bindings=bindings(raw, maximum, 1 if prior else 2)))
    assert len(rows) == 16 and len({row["id"] for row in rows}) == 16
    corpus = encoded(dict(schema_version=1, cases=rows))
    requests = encoded([r["request"] for r in rows])
    stdout = b"".join(r["stdout"].encode("ascii") for r in rows)
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
                         interpreter=dict(version=sys.version, executable=sys.executable,
                                          sha256=digest(Path(sys.executable).read_bytes()), isolated=True, bytecode=False),
                         capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(corpus),
                         request_utf8=requests.decode("ascii"), request_bytes=len(requests), request_sha256=digest(requests),
                         stdout_utf8=stdout.decode("ascii"), stdout_bytes=len(stdout), stdout_sha256=digest(stdout),
                         case_count=16, repeat_capture_equal=True, stdin_bytes=0,
                         environment={**environment, "TMPDIR": "<owned output directory>/tmp"},
                         construction="Authored literal whole authority and explicit 2x2x2x2 matrix; no native outputs. Unmodified helper writes only complete post-authority stdout. Each initial authority restored independently for two captures. Both original capture policies unchanged.",
                         binding_instances=[dict(id=r["id"], before=len(r["before_bindings"]),
                                                 stdout=len(r["stdout_bindings"]),
                                                 total=len(r["before_bindings"])+len(r["stdout_bindings"])) for r in rows]))
    for name, data in [("group_counter.json", corpus), ("group_counter_provenance.json", proof)]:
        (output / name).write_bytes(data)
        print(json.dumps(dict(path=name, bytes=len(data), sha256=digest(data))))


if __name__ == "__main__":
    main()
