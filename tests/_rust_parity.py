"""Black-box native parity against the retained Python evidence implementation.

This is a migration test oracle, not a runtime Python delegation mechanism.
No application/runtime/provider/check/Git execution; only the selected Rust binary.
"""
import argparse
import contextlib
from copy import deepcopy
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runner import coven, handoffs  # noqa: E402 -- selected checkout is the oracle


def main(binary):
    cases = 0
    with tempfile.TemporaryDirectory(prefix="coven-rust-") as temporary:
        root = Path(temporary)
        source = root / "worker.json"
        def native(*args, expected=None):
            result = subprocess.run([binary, *map(str, args)], cwd=root, capture_output=True, timeout=5)
            if expected is not None:
                assert result.returncode == expected, (args, result.returncode, result.stdout, result.stderr)
            return result
        def call(*args):
            with contextlib.redirect_stdout(io.StringIO()):
                assert coven.main(list(map(str, args))) == 0
        def snapshot(directory):
            return {str(path.relative_to(directory)): ("symlink", os.readlink(path)) if path.is_symlink()
                    else ("dir",) if path.is_dir() else ("file", path.read_bytes(), path.stat().st_mtime_ns)
                    for path in directory.rglob("*")}
        revision = "tasks-sha256:"+"0"*64
        value = {"schema_version":1,"contract":handoffs.HANDOFF,"task":{"id":"task","owner":"operator","revision":revision},
            "claimed_status":"completed","summary":"Worker claim λ 🦊 é é","confidence":"medium","findings":[],
            "files_inspected":[],"files_changed":[],"checks":[],"blockers":[],"risks":[],"follow_ups":[],
            "references":[{"kind":"artifact","uri":"file:///never-followed"}],"source_claim":None}
        def validate_case(raw):
            nonlocal cases
            source.write_bytes(raw)
            try:
                expected = handoffs.validate_file(source)
            except (ValueError, OSError, handoffs.storage.StorageError):
                expected = None
            actual = native("handoff", "validate", source, "--json")
            assert (actual.returncode == 0) == (expected is not None), (cases, raw[:120], actual.stdout, actual.stderr)
            if expected is not None:
                assert json.loads(actual.stdout) == expected
                assert actual.stdout.isascii(), "untrusted output must remain safely escaped"
            cases += 1
        def encoded(data):
            return json.dumps(data, ensure_ascii=True, allow_nan=True).encode()
        for text in (value["summary"], "é", "é", "\u202eclaim", "🦊"*4096, "a"*16384, "\t\r\ntext"):
            changed = {**value, "summary": text}
            validate_case(json.dumps(changed, ensure_ascii=False).encode())
            validate_case(encoded(changed))
        for key in value:
            changed = deepcopy(value)
            del changed[key]
            validate_case(encoded(changed))
        for key, item in [("schema_version", True), ("schema_version", 1.0), ("schema_version", "1"),
                          ("schema_version", 2), ("claimed_status", "accepted"), ("summary", ""),
                          ("summary", "\ud800"), ("summary", "a"*16385), ("summary", "🦊"*4097),
                          ("summary", "\x1b[0m"), ("confidence", []), ("verified", True), ("authorization", "merge"),
                          ("files_inspected", ["../outside"]), ("files_inspected", ["a//b"]),
                          ("files_inspected", ["a/./b"]), ("files_inspected", ["a\\b"]),
                          ("files_inspected", ["same","same"]), ("files_inspected", ["."]),
                          ("references", [{"kind":"artifact","uri":"bad\nuri"}]),
                          ("checks", [{"name":"x","claimed_outcome":"passed","details":"","evidence":[],"verified":True}])]:
            validate_case(encoded({**value, key: item}))
        for raw in (b'\xff', b'\xef\xbb\xbf{}', b'{"x":NaN}', b'{"x":1e999}', b'{"x":1,"x":2}',
                    b'{"task":{"id":"a","\\u0069d":"b"}}', b'['*17+b'0'+b']'*17, encoded(value)+b' trailing'):
            validate_case(raw)
        # These would otherwise be VALID: duplicate enforcement, not the closed
        # schema, must be what rejects them (including escaped-equivalent keys).
        complete = encoded(value)
        validate_case(complete[:-1]+b',"summary":"another valid summary"}')
        for duplicate in (b'"id":"task"', b'"\\u0069d":"task"'):
            changed = complete.replace(b'"id": "task"', b'"id": "task",'+duplicate, 1)
            assert changed != complete
            validate_case(changed)
        valid = encoded(value)
        validate_case(valid+b' '*(handoffs.MAX_INPUT-len(valid)))
        validate_case(valid+b' '*(handoffs.MAX_INPUT-len(valid)+1))
        for token in ("tasks-missing:v1", "tasks-ledger:v1:"+"a"*32+":"+"b"*32+":"+"9"*100+":"+"c"*64,
                      "tasks-ledger:v1:"+"a"*32+":"+"b"*32+":01:"+"c"*64):
            changed = deepcopy(value)
            changed["task"]["revision"] = token
            validate_case(encoded(changed))
        for state in ("committed", "uncommitted", "unknown"):
            for missing in (None, "repository_id", "source_sha", "state"):
                changed = deepcopy(value)
                changed["files_changed"] = ["src/example.rs"]
                changed["source_claim"] = {"state":state,"repository_id":None,"base_sha":"a"*40,
                    "source_sha":"b"*40 if state=="committed" else None,"tree_sha":"c"*40 if state=="committed" else None}
                if missing:
                    del changed["source_claim"][missing]
                validate_case(encoded(changed))

        # Archives are genuinely produced by Python's collection protocol.
        workspace = root / "control with spaces λ 🦊"
        call("init", workspace, "--blank", "--agents", "operator")
        call("task", "add", workspace, "Collect result", "--owner", "operator")
        revision, tasks = coven.read_task_snapshot(workspace)
        value["task"] = {"id":tasks[0]["id"],"owner":"operator","revision":revision}
        archives = []
        for index, summary in enumerate(("First λ 🦊", "Second \u202e é é", "Literal \u2028\u2029 quotes \" slash \\ and tab\tnewline\n")):
            value["summary"] = summary
            if index == 2:
                value["findings"] = [{"priority":"high","claim":"Literal 🦊 evidence", "evidence":["opaque://λ"]}]
                value["checks"] = [{"name":"unit","claimed_outcome":"passed","details":"claim\twith\ncontrols", "evidence":[]}]
                value["files_changed"] = ["src/λ.rs"]
                value["source_claim"] = {"state":"committed","repository_id":"00000000-0000-0000-0000-000000000000",
                    "base_sha":"a"*40,"source_sha":"b"*40,"tree_sha":"c"*40}
            source.write_bytes((encoded(value) if index==0 else json.dumps(value,ensure_ascii=False).encode())+b'\r\n')
            collected = handoffs.collect(workspace, source, expected_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), expected_tasks_revision=revision)
            identity = collected["collection_id"]
            archives.append((identity, Path(collected["artifact_path"])))
            expected = handoffs.inspect(workspace, identity)
            assert json.loads(native("handoff","inspect",workspace,identity,"--json",expected=0).stdout) == expected
            cases += 1
        for task in (None, value["task"]["id"], "not-present"):
            flags = [] if task is None else ["--task", task]
            assert json.loads(native("handoff","list",workspace,*flags,"--json",expected=0).stdout) == handoffs.list_collections(workspace, task_id=task)
            cases += 1
        (workspace / "state/tasks.json").write_text("corrupt current tasks must not affect history")
        (workspace / "coven.json").write_text("corrupt marker contents are not parsed by history")
        (workspace / "state/.workspace.lock").unlink()
        before = snapshot(workspace)
        for identity, _ in archives:
            assert json.loads(native("handoff","inspect",workspace,identity,"--json",expected=0).stdout) == handoffs.inspect(workspace,identity)
        assert json.loads(native("handoff","list",workspace,"--json",expected=0).stdout) == handoffs.list_collections(workspace)
        assert snapshot(workspace) == before
        cases += 3
        identity, path = archives[0]
        good = path.read_bytes()
        envelope = json.loads(good)
        variants = [good[:-1], good+b'\n', json.dumps(envelope, indent=2).encode()+b'\n']
        for mutation in (lambda v:v.update(schema_version=True), lambda v:v.update(collection_id="0"*64),
                         lambda v:v["payload"].update(handoff_bytes=True),
                         lambda v:v["payload"]["task_binding"].update(owner="other")):
            changed = deepcopy(envelope)
            mutation(changed)
            variants.append(json.dumps(changed,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()+b'\n')
        for bad in variants:
            path.write_bytes(bad)
            native("handoff","inspect",workspace,identity,"--json",expected=1)
            native("handoff","list",workspace,"--task","unmatched","--json",expected=1)
            cases += 1
        path.write_bytes(good)
        # Known inert temps count toward bounds but are never promoted/read as claims.
        inert = path.parent / ".handoff-orphan.tmp"
        inert.write_text("inert")
        before = snapshot(workspace)
        assert json.loads(native("handoff","list",workspace,"--json",expected=0).stdout) == handoffs.list_collections(workspace)
        assert snapshot(workspace) == before
        cases += 1
        if os.name == "posix":
            # Python reports surrogateescaped filesystem bytes; the preview
            # explicitly refuses this reporting case instead of lossy conversion.
            non_utf8 = os.fsdecode(os.fsencode(root)+b"/moved-\xff")
            os.rename(workspace, non_utf8)
            expected = handoffs.inspect(Path(non_utf8), archives[0][0])
            assert expected["handoff"]["task"]["id"] == value["task"]["id"]
            refusal = native("handoff","inspect",non_utf8,archives[0][0],"--json",expected=1)
            assert b"non_unicode_path_unsupported" in refusal.stderr
            os.rename(non_utf8, workspace)
        # Unported effectful operations reject before workspace creation or fallback.
        absent = root / "must-not-exist"
        before = snapshot(workspace)
        for args in (("handoff","collect",absent,"--file",source), ("init",absent),
                     ("session","prompt",absent,"--execute"), ("task","assign",absent,"x","y")):
            native(*args,expected=2)
            assert not absent.exists()
            assert snapshot(workspace) == before
            cases += 1
        if os.name == "posix":
            alias = root / "leaf-alias"
            alias.symlink_to(source)
            native("handoff","validate",alias,"--json",expected=1)
            fifo = root / "fifo"
            os.mkfifo(fifo)
            native("handoff","validate",fifo,"--json",expected=1)
            cases += 2
        # A native binary remains useful without Python, Git or a shell on PATH.
        source.write_bytes(encoded(value))
        clean = subprocess.run([binary,"handoff","validate",str(source),"--json"],
            cwd=root,env={"PATH":str(root/"no-executables"),"COVEN_HOME":"/absent"},capture_output=True,timeout=5)
        assert clean.returncode == 0 and json.loads(clean.stdout)==handoffs.validate_file(source)
        cases += 1
        # Managed root aliases are allowed; the historical path is not rewritten.
        alias = root / "workspace-alias"
        alias.symlink_to(workspace, target_is_directory=True)
        assert json.loads(native("handoff","inspect",alias,archives[0][0],"--json",expected=0).stdout) == handoffs.inspect(alias,archives[0][0])
        cases += 1
        quota = root / "quota"
        quota.mkdir()
        (quota/"coven.json").write_text("marker only")
        store = quota/"output/handoffs"
        store.mkdir(parents=True)
        for index in range(256):
            (store/f".handoff-{index}.tmp").write_bytes(b"")
        assert json.loads(native("handoff","list",quota,"--json",expected=0).stdout)==handoffs.list_collections(quota)
        extra = store/".handoff-extra.tmp"
        extra.write_bytes(b"")
        native("handoff","list",quota,"--json",expected=1)
        extra.unlink()
        cases += 2
        # Inert files count toward byte limits even though not parsed as results.
        for index in range(32):
            with (store/f".handoff-{index}.tmp").open("wb") as output:
                output.truncate(handoffs.MAX_ARCHIVE)
        assert json.loads(native("handoff","list",quota,"--json",expected=0).stdout)==handoffs.list_collections(quota)
        (store/".handoff-32.tmp").write_bytes(b"x")
        native("handoff","list",quota,"--json",expected=1)
        cases += 2
        with (store/".handoff-0.tmp").open("wb") as output:
            output.truncate(handoffs.MAX_ARCHIVE+1)
        native("handoff","list",quota,"--json",expected=1)
        cases += 1
        # No namespace or workspace file changes during native refusal.
        before = snapshot(workspace)
        native("handoff","inspect",workspace,"../escape","--json",expected=1)
        assert snapshot(workspace)==before
        cases += 1
    print(json.dumps({"native_python_parity_cases":cases,"known_deferred_path_reporting_limit":1,"native_mutations_enabled":False}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    main(parser.parse_args().binary)
