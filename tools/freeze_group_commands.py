#!/usr/bin/env python3
"""Optional independent capture from an explicitly exported historical tree."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

PIN = "bbda1c4f79a7c71e6785c25d7bd9d939deec6d71"
HELPER = "src/group_commands/oracle.py"
PREFIXES = ["runner", "src/group_commands.rs", "src/group_commands", "src/groups.rs",
            "src/groups", "src/values.rs", "src/values", "src/kitsune_observation.rs",
            "src/kitsune_observation", "src/observation_socket.rs", "src/observation_socket",
            "src/observation_time.rs", "src/task_time.rs", "src/platform"]
UUIDS = ["11111111-1111-4111-8111-111111111111",
         "22222222-2222-4222-8222-222222222222",
         "33333333-3333-4333-8333-333333333333"]
PATHS = ["/workspace_id", "/groups/0/group_id", "/groups/0/members/0/participant_id"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode("ascii")


def compact(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, allow_nan=False,
                      separators=(",", ":")).encode("utf-8")


def verify_sources(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN,
                                     *PREFIXES], cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith((".rs", ".py")))
    assert HELPER in paths and "src/group_commands/tests.rs" in paths
    assert len([p for p in paths if p.startswith("runner/")]) == 25
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in paths if p.startswith("runner/")]
    assert not list(root.rglob("*.pyc"))
    return pins


def spans(raw):
    # Independent structural scan; offsets are UTF-8 bytes including token quotes.
    text = raw.decode("utf-8")
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
            leaves[path] = (len(text[:at].encode("utf-8")), len(text[:end].encode("utf-8")))
            return end

    end = walk(0, "")
    assert text[end:] == "\n"
    return leaves


def slots(raw, count):
    tokens = spans(raw)
    result = []
    for path, uuid in zip(PATHS[:count], UUIDS[:count], strict=True):
        start, end = tokens[path]
        literal = json.dumps(uuid)
        assert raw[start:end] == literal.encode("ascii")
        result.append(dict(path=path, source=path, start=start, end=end, literal=literal))
    return result


def authored(process, native):
    seed = dict(schema_version=1, workspace_id=UUIDS[0], content_revision=1,
                groups=[dict(group_id=UUIDS[1], key="g", name="Group", members=[])],
                publication_counters=[])
    seed = (json.dumps(seed, ensure_ascii=True, indent=2) + "\n").encode("ascii")
    evidence = dict(provenance="kitsune.session.snapshot", observed_at="2026-11-05T00:00:00+00:00",
                    agent_kind_hint="djinn", native_reference=None, native_binding="unestablished")
    if native:
        evidence.update(native_reference=dict(source="native", agent="djinn", kind="id", value="session-\u00e9"),
                        native_binding="observed_reference")
    if process:
        evidence.update(provenance="kitsune.pane.process_info", process_evidence=dict(
            interpreter="coven.kitsune.djinn-process.v1", matches=[
                dict(pid=7, executable_family="djinn", rule="exact_posix_argv0_basename"),
                dict(pid=9, executable_family="djinn-ui", rule="exact_posix_argv0_basename")]))
    produced = dict(presentation=dict(adapter="kitsune", endpoint=dict(path="/fixture/api.sock",
                    device=18446744073709551615, inode=42), terminal_id="term", pane_id_at_binding="pane"),
                    evidence=evidence)
    return dict(seed=seed.decode("ascii"), produced=produced, participant=UUIDS[2], process=process)


def frame(streams):
    return b"".join(len(raw).to_bytes(8, "big") + raw for raw in streams)


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
    pins = verify_sources(project, root)
    # Every pin is independently corrupted in memory, before any helper execution.
    original = Path.read_bytes
    for path in pins:
        def tampered(self, selected=root / path):
            raw = original(self)
            return raw + b"!" if self == selected else raw
        with patch.object(Path, "read_bytes", tampered):
            try:
                verify_sources(project, root)
            except AssertionError as error:
                assert str(error) == path
            else:
                raise AssertionError(path)
    try:
        verify_sources(project, project)
    except AssertionError:
        pass
    else:
        raise AssertionError("active checkout accepted")
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8", TMPDIR=os.environ["TMPDIR"])
    rows = []
    for name, process, native in [("snapshot-null", False, False), ("snapshot-native", False, True),
                                  ("process-null", True, False)]:
        request = authored(process, native)
        stdin = compact(request)
        captures = [subprocess.check_output([sys.executable, "-I", "-B", str(root / HELPER)],
                    input=stdin, cwd=output, env=environment) for _ in range(2)]
        assert captures[0] == captures[1]
        stdout = captures[0]
        result = json.loads(stdout)
        raw = result["raw"].encode("utf-8")
        state = json.loads(raw)
        canonical = compact({key: state[key] for key in ["workspace_id", "content_revision", "groups"]})
        domain_digest = digest(b"coven.groups.content:v1\0" + canonical)
        assert result["sha256"] == digest(raw)
        assert result["revision"] == f"groups-v1:{UUIDS[0]}:2:{domain_digest}"
        rows.append(dict(id=name, process=process, native=native, request_utf8=stdin.decode(),
                         request_sha256=digest(stdin), seed_sha256=digest(request["seed"].encode()),
                         produced_utf8=compact(request["produced"]).decode(), stdout_utf8=stdout.decode(),
                         stdout_sha256=digest(stdout), seed_bindings=slots(request["seed"].encode(), 2),
                         raw_bindings=slots(raw, 3), canonical_content_utf8=canonical.decode(),
                         canonical_content_sha256=digest(canonical), domain_sha256=domain_digest))
    corpus = encoded(dict(schema_version=1, cases=rows))
    requests = frame([r["request_utf8"].encode() for r in rows])
    outputs = frame([r["stdout_utf8"].encode() for r in rows])
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
        interpreter=dict(version=sys.version, executable=sys.executable, sha256=digest(Path(sys.executable).read_bytes()),
                         isolated=True, bytecode=False), capture_tool_sha256=digest(Path(__file__).read_bytes()),
        corpus_sha256=digest(corpus), case_count=3, repeat_capture_equal=True,
        negative_controls=dict(individual_in_memory_pin_rejections=len(pins), active_checkout_rejected=True,
                               before_helper_execution=True),
        framing="8-byte big-endian length followed by complete stream, in case order",
        request_framed_hex=requests.hex(), request_bytes=len(requests), request_sha256=digest(requests),
        stdout_framed_hex=outputs.hex(), stdout_bytes=len(outputs), stdout_sha256=digest(outputs),
        environment={**environment, "TMPDIR": "<owned output directory>/tmp"},
        binding_instances=[dict(id=r["id"], seed=2, raw=3, total=5) for r in rows],
        construction="Independently authored seed and producer; three original tuples only; unmodified historical helper twice per request. No native output inputs."))
    for name, data in [("group_commands.json", corpus), ("group_commands_provenance.json", proof)]:
        (output / name).write_bytes(data)
        print(json.dumps(dict(path=name, bytes=len(data), sha256=digest(data))))


if __name__ == "__main__":
    main()
