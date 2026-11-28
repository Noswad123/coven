#!/usr/bin/env python3
"""Optional capture of all source group comparisons from an explicit Git export."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

PIN = "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
HELPER = "src/groups/oracle.py"
PREFIXES = ["runner", "src/groups", "src/groups.rs", "src/values", "src/values.rs",
            "src/storage.rs", "src/read_fs.rs", "src/platform", "src/platform.rs",
            "src/strict_json.rs", "src/task_time.rs"]


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode("ascii")


def pretty(value):
    return json.dumps(value, ensure_ascii=True, indent=2) + "\n"


def ident(n):
    return f"00000000-0000-4000-8000-{n:012x}"


def binding():
    # Literal input, not a Rust-generated authority or copied implementation.
    return json.loads('{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not/a/socket","device":184467440737095516160000,"inode":0},"terminal_id":"term","pane_id_at_binding":"w1:p1"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2024-W01T12Z","agent_kind_hint":null,"native_reference":null,"native_binding":"unestablished"}}')


def attachment():
    return json.loads('{"adapter":"djinn_live","source_contract":"djinn.live-observation.v1","artifact_path":"/does/not/exist/00000000-0000-4000-8000-000000000001.json","runtime_id":"00000000-0000-4000-8000-000000000001","instance_id":"00000000-0000-4000-8000-000000000002","session_id":"session!","attached_at":"20240101T120000,123456789-000000,0000009"}')


def state():
    return dict(schema_version=1, workspace_id=ident(1), content_revision=2,
                groups=[dict(group_id=ident(2), key="g", name="Group", members=[
                    dict(participant_id=ident(3), key="m", role="Role", binding=binding())])],
                publication_counters=[])


def schemas():
    base = state()
    member = base["groups"][0]["members"][0]

    def replace(m, version):
        s = copy.deepcopy(base)
        s["schema_version"] = version
        s["groups"][0]["members"] = [copy.deepcopy(m)]
        return s

    m3 = dict(member, live_attachment=None)
    cases = [base, replace(m3, 3), replace(m3, 2), replace(member, 3),
             replace(dict(member, live_attachment=attachment()), 3)]
    for pid in [True, 1.0, 0, 4294967296, 2]:
        m = copy.deepcopy(member)
        m["binding"]["evidence"] = dict(provenance="kitsune.pane.process_info", observed_at="2024-01-01T00Z",
            agent_kind_hint="djinn", native_reference=None, native_binding="unestablished",
            process_evidence=dict(interpreter="coven.kitsune.djinn-process.v1", matches=[
                dict(pid=pid, executable_family="djinn", rule="exact_posix_argv0_basename"),
                dict(pid=1, executable_family="djinn-ui", rule="exact_posix_argv0_basename")]))
        cases.append(replace(m, 2))
    duplicate = copy.deepcopy(base)
    duplicate["groups"][0]["members"].append(dict(member, key="other", participant_id=ident(999)))
    cases.extend([duplicate, replace(dict(member, participant_id=ident(2)), 1)])
    return [pretty(s) for s in cases]


def authorities():
    base = state()
    cases = [pretty(base), pretty(dict(base, workspace_id="ffffffff-ffff-ffff-ffff-ffffffffffff"))]
    cases.extend(pretty(dict(base, content_revision=v)) for v in [True, -1, 1.0, 18446744073709551616])
    cases.extend([pretty(dict(base, unknown=None)), pretty(base).replace('"groups":', '"groups": [], "groups":', 1)])
    return cases


def timestamps():
    cases = ["2024-01-01T12Z", "20240101T1200+00", "2024W011T120000Z", "2024-W01T12-00:00",
             "2020-W53-7T23:59:59.123456789+000000,0000009", "2021-W53-1T00Z", "0000-01-01T00Z",
             "9999-W52-7T00Z", "2024-02-30T00Z", "2024-01-01T23:59:60Z", "2024-01-01T24Z",
             "2024-01-01t12Z", "2024-01-01T12z", "2024-01-01T12.1Z", "2024-01-01T12:00,1Z",
             "2024-01-01T120000+00.1", "2024-01-01T120000+00:00:00.1", "2024-01-01T120000-00:00:00.0000001",
             "2024-01-01T12:00:00+00:60", "2024-01-01T00:00:00+24", "2024-01-01T00:00:00.Z", "2024-01-01T00:00:00Zextra"]
    cases.extend(f"{y:04}-W{w:02}-{d}T00Z" for y in [1, 4, 100, 400, 1900, 2000, 2024, 9999]
                 for w in [0, 1, 52, 53, 54] for d in [1, 7])
    return cases


def verify(project, root):
    assert root != project and not root.is_relative_to(project) and not (root / ".git").exists()
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, *PREFIXES], cwd=project).decode().splitlines()
    paths = sorted(p for p in paths if p.endswith((".py", ".rs")))
    assert HELPER in paths and "src/groups/tests.rs" in paths
    assert len([p for p in paths if p.startswith("runner/")]) == 25
    assert sorted(str(p.relative_to(root)) for p in (root / "runner").rglob("*.py")) == [p for p in paths if p.startswith("runner/")]
    assert not list(root.rglob("*.pyc"))
    pins = {}
    for path in paths:
        raw = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        assert (root / path).read_bytes() == raw, path
        pins[path] = digest(raw)
    return pins


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--historical-root", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    assert sys.version_info[:3] == (3, 14, 8) and sys.flags.isolated and sys.dont_write_bytecode
    project = Path(__file__).resolve().parents[1]
    root = args.historical_root.resolve(strict=True)
    output = args.output_root.resolve(strict=True)
    pins = verify(project, root)  # Before any import from the historical tree.
    environment = dict(PATH="/usr/bin:/bin", LANG="C.UTF-8", LC_ALL="C.UTF-8", TMPDIR=os.environ["TMPDIR"])
    assert Path(environment["TMPDIR"]).resolve(strict=True).is_relative_to(root.parent)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))

    def run(request):
        child = subprocess.Popen([sys.executable, "-I", "-B", str(root / HELPER)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd=root, env=environment, start_new_session=True)
        try:
            stdout, stderr = child.communicate(encoded(request), timeout=30)
            assert child.returncode == 0, stderr.decode()
            return stdout
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
            child.wait()

    rows = []

    def capture(section, index, request):
        first, second = run(request), run(request)
        assert first == second, (section, index)
        request_raw = encoded(request)
        rows.append(dict(id=f"{section}-{index:03}", request_raw=request_raw.decode("ascii"),
                         request_sha256=digest(request_raw), result_raw=first.decode("utf-8"),
                         result_sha256=digest(first)))
        return json.loads(first)

    validators = {
        "selector": ["g", "0", "g_-9", "", "A", "-g", "g.x", "a"*64, "a"*65, "\u00e9"],
        "display": ["Role", " ", "", "\u00e9"*32, "\u00e9"*33, " x ", "x\t", "\u00a0", "\u2028", "\u200b", "\ue000", "\u0378", "\U0001fae9"],
        "source": ["Coven.A_-9", "a"*64, "a"*65, "", "a b", "\u00e9", "-"]}
    a = attachment()
    validators["attachment"] = [a] + [dict(a, **{k: v}) for k, v in [
        ("runtime_id", "00000000-0000-1000-8000-000000000001"),
        ("instance_id", "00000000-0000-4000-0000-000000000002"), ("session_id", "with space"),
        ("artifact_path", "/bad/../00000000-0000-4000-8000-000000000001.json"),
        ("artifact_path", "/not-the-runtime.json"), ("attached_at", "2024-01-01T00+01"), ("extra", None)]]
    for validator, cases in validators.items():
        for i, value in enumerate(cases):
            capture("pure-" + validator, i, dict(mode="pure", validator=validator, values=[value]))
    for section, mode, cases in [("schema", "validate", schemas()), ("timestamp", "timestamp", timestamps()),
                                  ("authority", "validate", authorities())]:
        for i, value in enumerate(cases):
            capture(section, i, dict(mode=mode, values=[value]))

    # The sequence is authored from the old caller. Only historical Python outputs
    # supply subsequent raw authority, revision, participant and consumed UUID count.
    b = binding()
    b["presentation"]["endpoint"]["device"] = 7
    b = json.loads(encoded(b))  # Original caller canonicalized these bindings.
    ep = b["presentation"]["endpoint"]
    process = copy.deepcopy(b)
    process["evidence"] = dict(provenance="kitsune.pane.process_info", observed_at="2024-01-01T00Z",
        agent_kind_hint="djinn", native_reference=None, native_binding="unestablished",
        process_evidence=dict(interpreter="coven.kitsune.djinn-process.v1", matches=[
            dict(pid=1, executable_family="djinn", rule="exact_posix_argv0_basename")]))
    process = json.loads(encoded(process))
    a = json.loads(encoded(a))
    raw, start, token, participant = None, 1, "groups-uninitialized:v1", ident(3)
    for step in range(18):
        if step in [0, 1]:
            operation, args = "create_group", dict(key="g", name="Group")
        elif step in [2, 3, 6, 7, 10, 13]:
            operation = "add_member"
            args = dict(group_key="g", member_key="m", role="New role" if step == 10 else "Role",
                        binding=process if step in [6, 7] else b)
            if step in [6, 7, 10]:
                args["rebind"] = True
        elif step in [4, 5, 14, 15]:
            operation, args = "reserve_cycle", dict(group_key="g", expected_revision=token, endpoint=ep,
                source="coven", epoch="two" if step == 15 else "one", terminal_ids=["term"], allow_new_epoch=step == 15)
        elif step in [8, 9]:
            operation, args = "attach_live", dict(group_key="g", member_key="m", attachment=a,
                expected_revision=token, expected_participant_id=participant)
        elif step in [11, 12]:
            operation, args = "remove_member", dict(group_key="g", member_key="m")
        else:
            operation, args = "detach_live", dict(group_key="g", member_key="m", expected_revision=token,
                                                   expected_participant_id=participant)
        request = dict(mode="mutation", raw=raw, ids=[ident(n) for n in range(start, start+8)],
                       action=dict(operation=operation, args=args))
        result = capture("mutation", step, request)
        assert result["accepted"], step
        raw = result["raw"]
        token = result["result"]["revision"]
        members = json.loads(raw)["groups"][0]["members"]
        participant = members[0]["participant_id"] if members else ident(3)
        # UUIDs occur only in initial workspace/group and new-member declarations.
        if step == 0:
            assert json.loads(raw)["workspace_id"] == ident(start)
            assert json.loads(raw)["groups"][0]["group_id"] == ident(start+1)
            start += 2
        elif step in [2, 13]:
            assert participant == ident(start)
            start += 1
    assert len(rows) == 178 and len({r["id"] for r in rows}) == 178
    corpus = encoded(dict(schema_version=1, cases=rows)) + b"\n"
    proof = encoded(dict(schema_version=1, historical_commit=PIN, source_sha256=pins,
        capture_tool_sha256=digest(Path(__file__).read_bytes()), corpus_sha256=digest(corpus),
        case_count=178, counts=dict(pure=38, schema=12, timestamp=102, authority=8, mutation=18),
        repeat_capture_equal=True, interpreter=dict(version=sys.version, isolated=True, bytecode=False,
            executable_sha256=digest(Path(sys.executable).read_bytes())),
        environment=dict(environment, TMPDIR="<owned run>/tmp"),
        input_strategy="Entire original ordered matrix authored independently as literals and loops; no native output capture, deduplication or zip. Mutation next-state inputs come only from unmodified historical helper outputs. Both comparison setup tests now use existing Fixed UUID seam (1,2,3), retaining real filesystem publication; other SystemIo, fault and race tests unchanged.",
        result_strategy="Complete unmodified helper stdout, including all validation revisions and 18 full result objects plus exact storage strings. Eight supplied UUIDs per step remain in each full request; next start advances only on initial creation or new participant.")) + b"\n"
    for name, raw in [("source_groups.json", corpus), ("source_groups_provenance.json", proof)]:
        (output / name).write_bytes(raw)
        print(json.dumps(dict(path=name, bytes=len(raw), sha256=digest(raw))))


if __name__ == "__main__":
    main()
