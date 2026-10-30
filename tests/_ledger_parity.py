"""Real Python task-authority histories and adversarial native replay checks."""
import argparse
import contextlib
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.set_int_max_str_digits(4300)
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from runner import coven, storage, task_ledger as ledger


def main(binary):
    cases=0
    with tempfile.TemporaryDirectory(prefix="native-ledger-") as temporary:
        root=Path(temporary)
        def python(*args):
            out,err=io.StringIO(),io.StringIO()
            with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
                try: status=coven.main(list(map(str,args)))
                except SystemExit as error: status=error.code
            return status,out.getvalue(),err.getvalue()
        def invoke(*args):
            status,out,error=python(*args)
            assert status==0,(args,status,out,error)
            return json.loads(out) if out.lstrip().startswith("{") else out
        def native(command,workspace,expected=None):
            result=subprocess.run([binary,*command,str(workspace),"--json"],cwd=root,
                env={**os.environ,"COVEN_MULTIPLEXER":""},capture_output=True,timeout=8)
            if expected is not None: assert result.returncode==expected,(command,result.returncode,result.stdout,result.stderr)
            return result
        def snapshot(path):
            result={}
            for item in path.rglob("*"):
                relative=str(item.relative_to(path))
                mode=item.lstat().st_mode
                if item.is_symlink(): result[relative]=("link",os.readlink(item),mode)
                elif item.is_dir(): result[relative]=("dir",mode)
                elif item.is_file(): result[relative]=("file",item.read_bytes(),mode,item.stat().st_mtime_ns,item.stat().st_ino)
                else: result[relative]=("special",mode)
            return result
        def normalized(report):
            # Error text is presentation, not a versioned cross-language value.
            report=deepcopy(report)
            if "projection" in report and "error" in report["projection"]: report["projection"]["error"]="diagnostic"
            if "source_error" in report: report["source_error"]="diagnostic"
            return report
        def compare(workspace,command=("task","list"),accepted=None):
            nonlocal cases
            before=snapshot(workspace)
            status,out,error=python(*command,workspace,"--json")
            result=native(command,workspace)
            assert (result.returncode==0)==(status==0),(cases,command,status,error,result.stdout,result.stderr)
            if accepted is not None: assert (status==0)==accepted,(cases,out,error)
            if status==0:
                expected=normalized(json.loads(out))
                actual=normalized(json.loads(result.stdout))
                assert ledger._canonical(expected)==ledger._canonical(actual),(cases,expected,actual)
            assert snapshot(workspace)==before,"read changed authority, namespace, mode, mtime or inode"
            cases+=1
        def fixture(name,missing=False):
            workspace=root/name
            invoke("init",workspace,"--blank","--agents","operator,other")
            source=workspace/"state/tasks.json"
            if missing: source.unlink()
            else:
                source.write_bytes((json.dumps([{"id":"seed","title":"Original λ","status":"legacy custom",
                    "extra":{"z":1e-07,"a":-0.0,"integer":2**64+1,"nested":["é","é","🦊",None]}}],
                    indent=4,ensure_ascii=False)+"\r\n").encode())
            revision=coven.read_task_snapshot(workspace)[0]
            invoke("state","import-tasks",workspace,"--expect-revision",revision,"--json")
            return workspace
        def base(workspace):
            marker=json.loads((workspace/ledger.MARKER).read_text())
            return workspace/ledger.LEDGER/marker["epoch_id"]
        def clone(source,name):
            target=root/name
            shutil.copytree(source,target,symlinks=True)
            return target
        def store_frame(path,payload): path.write_bytes(ledger._envelope(payload))
        def resign_baseline(workspace, change_source=None, change_payload=None):
            folder=base(workspace)
            marker_path=workspace/ledger.MARKER
            marker=json.loads(marker_path.read_text())
            manifest_path=folder/"manifest.json"
            manifest=json.loads(manifest_path.read_text())
            frame=folder/"frames/00000000000000000000.frame"
            payload=ledger._payload(frame.read_bytes())
            if change_source is not None:
                backup=folder/"baseline.tasks.bytes"
                backup.write_bytes(change_source(backup.read_bytes()))
                raw=backup.read_bytes()
                manifest.update(source_revision=storage.snapshot_revision(raw),source_length=len(raw),source_digest=ledger._digest(raw))
                payload["source_revision"]=manifest["source_revision"]
            if change_payload is not None: change_payload(payload)
            store_frame(frame,payload)
            marker["baseline_digest"]=manifest["baseline_digest"]=ledger._digest(frame.read_bytes())
            manifest_path.write_text(json.dumps(manifest))
            marker["manifest_digest"]=ledger._digest(manifest_path.read_bytes())
            marker_path.write_text(json.dumps(marker))

        baseline=fixture("baseline")
        compare(baseline)
        compare(baseline,("state","inspect"))
        absent=fixture("absent baseline",True)
        compare(absent)
        compare(absent,("state","inspect"))
        # Baseline source formatting, not reconstructed JSON, determines health.
        source=baseline/"state/tasks.json"
        original=source.read_bytes()
        for projection in (None,b"broken\xff",json.dumps(json.loads(original)).encode()):
            if projection is None: source.unlink(missing_ok=True)
            else: source.write_bytes(projection)
            compare(baseline)
            compare(baseline,("state","inspect"))
        source.write_bytes(original)
        if os.name=="posix":
            source.unlink()
            source.symlink_to(root/"missing")
            compare(baseline)
            compare(baseline,("state","inspect"))
            source.unlink()
            os.mkfifo(source)
            compare(baseline)
            compare(baseline,("state","inspect"))
            source.unlink()
            source.write_bytes(original)

        history=clone(baseline,"history")
        for index,(title,owner) in enumerate((("Duplicate title",None),("Duplicate title","OPERATOR"),("İ + K extra",None))):
            args=["task","add",history,title,"--notes","Opaque λ details","--request-id",f"add-{index}",
                "--expect-revision",coven.read_task_snapshot(history)[0],"--json"]
            if owner: args.extend(["--owner",owner])
            invoke(*args)
            compare(history)
        invoke("task","assign",history,"seed","OTHER","--request-id","assign-1",
            "--expect-revision",coven.read_task_snapshot(history)[0],"--json")
        compare(history)
        compare(history,("state","inspect"))
        # History must not re-resolve old owners against the current roster.
        config=history/"coven.json"
        team=json.loads(config.read_text());team["agents"]=["other"];config.write_text(json.dumps(team))
        compare(history)
        # State inspect has narrower record-content preflight than task listing.
        agents=history/"state/agents.json";saved=agents.read_bytes();agents.write_text("bad current agent records")
        compare(history,("state","inspect"),True)
        compare(history,accepted=False)
        agents.write_bytes(saved)

        # Corruption must fail even when the projection still contains valid tasks.
        for index,mutation in enumerate((
            lambda value:value.update(version=True),lambda value:value.update(extra=True),
            lambda value:value.update(workspace_id="0"*32),lambda value:value.update(manifest_digest="0"*64),
            lambda value:value.update(baseline_digest="0"*64),lambda value:value.update(epoch_id="../escape"))):
            target=clone(history,f"marker-{index}")
            marker=target/ledger.MARKER;value=json.loads(marker.read_text());mutation(value);marker.write_text(json.dumps(value))
            compare(target,accepted=False)
        for index,mutation in enumerate((
            lambda value:value.update(version=1.0),lambda value:value.update(source_present=1),
            lambda value:value.update(source_length=True),lambda value:value.update(source_length=0),
            lambda value:value.update(source_mode=True),lambda value:value.update(source_mode=4096),
            lambda value:value.update(source_digest="0"*64),lambda value:value.update(source_revision="tasks-missing:v1"))):
            target=clone(history,f"manifest-{index}")
            manifest=base(target)/"manifest.json";value=json.loads(manifest.read_text());mutation(value)
            manifest.write_text(json.dumps(value))
            marker=target/ledger.MARKER;m=json.loads(marker.read_text());m["manifest_digest"]=ledger._digest(manifest.read_bytes());marker.write_text(json.dumps(m))
            compare(target,accepted=False)
        for index,change in enumerate(("gap","empty","unexpected","symlink-temp","directory-temp","bad-name","missing-baseline")):
            target=clone(history,f"files-{index}");frames=base(target)/"frames"
            if change=="gap": (frames/"00000000000000000001.frame").unlink()
            elif change=="empty":
                for item in frames.iterdir(): item.unlink()
            elif change=="unexpected": (base(target)/"unexpected").write_text("not allowed")
            elif change=="symlink-temp": (frames/".manifest.json.X.tmp").symlink_to(root/"missing")
            elif change=="directory-temp": (frames/".manifest.json.X.tmp").mkdir()
            elif change=="bad-name": (frames/"00000000000000000001.frame").rename(frames/"1.frame")
            else: (base(target)/"baseline.tasks.bytes").unlink()
            compare(target,accepted=False)
        inert=clone(history,"inert temps")
        (base(inert)/"frames/.manifest.json.X.tmp").write_text("inert")
        (base(inert)/".00000000000000000001.frame.X.tmp").write_text("inert")
        compare(inert)

        # Single-add/assign fixtures let semantic defects carry VALID hashes and
        # recomputed final revisions, distinguishing replay checks from hashing.
        add=clone(baseline,"one add")
        invoke("task","add",add,"A task","--owner","operator","--notes","note","--request-id","add-one","--json")
        assign=clone(baseline,"one assign")
        invoke("task","assign",assign,"seed","operator","--request-id","assign-one","--json")
        def corrupt_effect(source,label,operation,mutate):
            target=clone(source,label);path=base(target)/"frames/00000000000000000001.frame"
            value=ledger._payload(path.read_bytes());mutate(value)
            first=ledger._payload((base(target)/"frames/00000000000000000000.frame").read_bytes())
            tasks=deepcopy(first["tasks"])
            if operation=="add": tasks.append(value["result"]["task"])
            else: tasks[0]=value["result"]["task"]
            value["result"]["after_revision"]=ledger._revision(value["workspace_id"],value["epoch_id"],1,tasks)
            store_frame(path,value)
            compare(target,accepted=False)
        for index,mutate in enumerate((
            lambda value:value["result"]["task"].update(title="Wrong title"),
            lambda value:value["result"]["task"].update(id="wrong-id"),
            lambda value:value["result"]["task"].update(owner="other"),
            lambda value:value["result"]["task"].update(status="pending"),
            lambda value:value["result"]["task"].pop("created_at"),
            lambda value:value["binding"].update(expected_revision="tasks-sha256:"+"0"*64),
            lambda value:value.update(sequence=True),lambda value:value["result"].update(sequence=True),
            lambda value:value["result"].update(outcome="failed"),lambda value:value["binding"]["args"].update(extra=True))):
            corrupt_effect(add,f"bad-add-{index}","add",mutate)
        for index,mutate in enumerate((
            lambda value:value["result"]["task"].update(extra={"changed":True}),
            lambda value:value["result"]["task"].update(owner="other"),
            lambda value:value["result"]["task"].pop("updated_at"),
            lambda value:value["result"]["task"].update(title="changed"))):
            corrupt_effect(assign,f"bad-assign-{index}","assign",mutate)
        for index,mutate in enumerate((lambda value:value.update(previous_digest="0"*64),
            lambda value:value.update(request_id="add-0"),lambda value:value["result"].update(mutation_id=True))):
            target=clone(history,f"last-frame-{index}");path=sorted((base(target)/"frames").glob("*.frame"))[-1]
            value=ledger._payload(path.read_bytes());mutate(value);store_frame(path,value)
            compare(target,accepted=False)
        # No extra mutation-ID uniqueness restriction: valid repeated UUID allowed.
        repeated=clone(history,"repeated mutation ID");frames=sorted((base(repeated)/"frames").glob("*.frame"))
        value=ledger._payload(frames[-1].read_bytes());value["result"]["mutation_id"]=ledger._payload(frames[-2].read_bytes())["result"]["mutation_id"]
        store_frame(frames[-1],value);compare(repeated)
        for index,raw_change in enumerate((lambda raw:raw[:-1],lambda raw:raw+b" ",lambda raw:raw.replace(b'"version":1',b'"version":1.0',1))):
            target=clone(history,f"noncanonical-{index}");path=sorted((base(target)/"frames").glob("*.frame"))[-1]
            path.write_bytes(raw_change(path.read_bytes()));compare(target,accepted=False)
        for index,mutation in enumerate((lambda value:value.update(sequence=True),lambda value:value.update(previous_digest="0"*64),
                lambda value:value["tasks"][0]["extra"].update(a=0),lambda value:value.update(revision="tasks-missing:v1"))):
            target=clone(baseline,f"baseline-payload-{index}")
            resign_baseline(target,change_payload=mutation)
            compare(target,accepted=False)
        for index,mutation in enumerate((lambda value:value.update(source_mode=0),lambda value:value.update(source_length=1),
                lambda value:value.update(source_digest="0"*64),lambda value:value.update(source_revision="tasks-sha256:"+"0"*64))):
            target=clone(absent,f"bad-absent-{index}")
            manifest=base(target)/"manifest.json";value=json.loads(manifest.read_text());mutation(value);manifest.write_text(json.dumps(value))
            marker=target/ledger.MARKER;value=json.loads(marker.read_text());value["manifest_digest"]=ledger._digest(manifest.read_bytes());marker.write_text(json.dumps(value))
            compare(target,accepted=False)
        extra_backup=clone(absent,"absent with backup");(base(extra_backup)/"baseline.tasks.bytes").write_bytes(b"[]")
        compare(extra_backup,accepted=False)
        def inconsistent(value):
            value["tasks"][0]["extra"]["a"]=0
            value["revision"]=ledger._revision(value["workspace_id"],value["epoch_id"],0,value["tasks"])
        target=clone(baseline,"resigned source mismatch")
        resign_baseline(target,change_payload=inconsistent)
        compare(target,accepted=False)
        duplicate=clone(baseline,"strict duplicate backup")
        resign_baseline(duplicate,change_source=lambda raw:raw.replace(b'"id": "seed"',b'"id": "seed", "id": "seed"',1))
        compare(duplicate,accepted=False)
        # Some Python-supported byte encodings are explicitly outside the native
        # UTF-8 profile, not a reason to parse the projection as authority.
        encoded=clone(baseline,"unsupported source encoding")
        resign_baseline(encoded,change_source=lambda raw:raw.decode("utf-8").encode("utf-16"))
        before=snapshot(encoded)
        assert python("task","list",encoded,"--json")[0]==0
        native(("task","list"),encoded,1)
        assert snapshot(encoded)==before
        cases+=1
        # Extra fields on add, empty timestamps and duplicate mutation UUIDs are
        # permitted by Python; replay must not invent stricter lifecycle rules.
        allowed=clone(add,"allowed extra add fields")
        path=base(allowed)/"frames/00000000000000000001.frame"
        value=ledger._payload(path.read_bytes());value["result"]["task"].update(extension={"float":2.9802322387695312e-08},created_at="",updated_at="")
        first=ledger._payload((base(allowed)/"frames/00000000000000000000.frame").read_bytes())
        value["result"]["after_revision"]=ledger._revision(value["workspace_id"],value["epoch_id"],1,first["tasks"]+[value["result"]["task"]])
        store_frame(path,value);compare(allowed)
        # Unsupported mutations remain parser refusals with no recovery/write.
        before=snapshot(history)
        for command in (("state","recover-tasks"),("state","import-tasks"),("task","add"),("task","assign")):
            native(command,history,2)
            assert snapshot(history)==before
            cases+=1
        oversized=clone(history,"oversized final frame")
        path=sorted((base(oversized)/"frames").glob("*.frame"))[-1]
        with path.open("wb") as stream: stream.truncate(ledger.MAX_FRAME+1)
        compare(oversized,accepted=False)
        projection=clone(baseline,"large readable projection")
        with (projection/"state/tasks.json").open("wb") as stream: stream.truncate(9*1024*1024)
        compare(projection)
        for index,mutation in enumerate((lambda value:value.update(length=True),lambda value:value.update(length=1.0),
                lambda value:value.update(length=0),lambda value:value.update(sha256="0"*64),lambda value:value.update(extra=True))):
            target=clone(history,f"bad-envelope-{index}");path=sorted((base(target)/"frames").glob("*.frame"))[-1]
            envelope=json.loads(path.read_text());mutation(envelope);path.write_bytes(ledger._canonical(envelope)+b"\n")
            compare(target,accepted=False)
        for field in (ledger.MARKER,):
            target=clone(history,"duplicate marker key");path=target/field;raw=path.read_bytes()
            path.write_bytes(raw.rstrip()[:-1]+b',"version":1}')
            compare(target,accepted=False)
        # Snapshot state diagnostics, including unresolved import without promotion.
        pending=root/"pending";invoke("init",pending,"--blank","--agents","operator")
        compare(pending,("state","inspect"))
        (pending/ledger.LEDGER).mkdir()
        compare(pending,("state","inspect"))
        (pending/"state/tasks.json").write_text("bad pending source")
        compare(pending,("state","inspect"))
        assert native(("task","list"),pending,1).stdout==b""
        cases+=1
    print(json.dumps({"ledger_authority_and_corruption_cases":cases,"native_writes_enabled":False}))


if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("--binary",required=True)
    main(parser.parse_args().binary)
