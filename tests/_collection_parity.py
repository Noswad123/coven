"""Native/Python evidence writers share one protocol and kernel lock, no jobs."""
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.set_int_max_str_digits(4300)
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from runner import coven, handoffs, task_attempts


def main(binary):
    cases=0
    with tempfile.TemporaryDirectory(prefix="mixed-collection-") as temporary:
        root=Path(temporary)
        env={**os.environ,"PYTHONDONTWRITEBYTECODE":"1","COVEN_MULTIPLEXER":"","PYTHONINTMAXSTRDIGITS":"4300"}
        def invoke(*args,expected=0):
            out,error=io.StringIO(),io.StringIO()
            with contextlib.redirect_stdout(out),contextlib.redirect_stderr(error):
                try: status=coven.main(list(map(str,args)))
                except SystemExit as failure: status=failure.code
            assert status==expected,(args,status,out.getvalue(),error.getvalue())
            return json.loads(out.getvalue()) if out.getvalue().lstrip().startswith("{") else out.getvalue()
        def snapshot(path):
            result={}
            for entry in path.rglob("*"):
                key=str(entry.relative_to(path));metadata=entry.lstat()
                if entry.is_symlink(): result[key]=("link",os.readlink(entry),metadata.st_mode)
                elif entry.is_dir(): result[key]=("dir",metadata.st_mode)
                elif entry.is_file(): result[key]=("file",entry.read_bytes(),metadata.st_mode,metadata.st_mtime_ns,metadata.st_ino)
                else: result[key]=("special",metadata.st_mode,metadata.st_ino)
            return result
        def fixture(name):
            workspace=root/name
            invoke("init",workspace,"--blank","--agents","operator,other")
            (workspace/"state/tasks.json").write_bytes((' [ {"id":"task","title":"Assigned λ","status":"assigned","owner":"operator",'
                '"extra":{"n":18446744073709551617,"float":2.9802322387695312e-08,"zero":-0.0,"nested":{"b":2,"a":1}}} ]\r\n').encode())
            return workspace
        def document(workspace,name="worker.json",summary="Literal λ 🦊 é é\nclaim only"):
            revision=coven.read_task_snapshot(workspace)[0]
            value={"schema_version":1,"contract":handoffs.HANDOFF,"task":{"id":"task","owner":"operator","revision":revision},
                "claimed_status":"completed","summary":summary,"confidence":"medium","findings":[],"files_inspected":["src/never-opened"],
                "files_changed":["src/uncommitted"],"checks":[{"name":"reported","claimed_outcome":"passed","details":"No execution here","evidence":[]}],
                "blockers":[],"risks":[],"follow_ups":[],"references":[{"kind":"artifact","uri":"file:///missing/not-read"}],
                "source_claim":{"state":"uncommitted","repository_id":None,"base_sha":None,"source_sha":None,"tree_sha":None}}
            source=root/name;source.write_text(json.dumps(value,ensure_ascii=False,indent=2)+"\n")
            digest=handoffs.validate_file(source)["handoff_sha256"]
            return source,digest,revision
        def args(workspace,doc):
            source,digest,revision=doc
            return ["handoff","collect",str(workspace),"--file",str(source),"--expect-sha256",digest,"--expect-tasks-revision",revision,"--json"]
        def native(workspace,doc,expected=0):
            result=subprocess.run([binary,*args(workspace,doc)],cwd=root,env=env,capture_output=True,timeout=12)
            assert result.returncode==expected,(result.returncode,result.stdout,result.stderr)
            return json.loads(result.stdout) if expected==0 else result
        def python(workspace,doc):
            source,digest,revision=doc
            return handoffs.collect(workspace,source,expected_sha256=digest,expected_tasks_revision=revision)
        def golden(workspace,doc):
            source,digest,revision=doc;raw,value=handoffs.read_handoff(source)
            binding=handoffs._context(workspace.resolve(),value,revision)
            return handoffs._envelope(raw,binding,workspace.resolve())
        # Native -> Python and Python -> native converge on exact bytes and IDs.
        for native_first in (True,False):
            workspace=fixture("native-first" if native_first else "python-first")
            doc=document(workspace,f"worker-{native_first}.json")
            identity,encoded=golden(workspace,doc)
            before=snapshot(workspace)
            first=native(workspace,doc) if native_first else python(workspace,doc)
            artifact=Path(first["artifact_path"]);inode=artifact.stat().st_ino;mtime=artifact.stat().st_mtime_ns
            assert first["publication"]=="created" and first["collection_id"]==identity
            assert artifact.read_bytes()==encoded and artifact.stat().st_mode&0o777==0o600
            second=python(workspace,doc) if native_first else native(workspace,doc)
            assert second=={**first,"publication":"existing"}
            assert artifact.stat().st_ino==inode and artifact.stat().st_mtime_ns==mtime
            after=snapshot(workspace)
            assert all(after[key]==value for key,value in before.items())
            assert all(key=="state/.workspace.lock" or key.startswith("output/handoffs") for key in set(after)-set(before))
            assert handoffs.inspect(workspace,identity)["handoff"]==handoffs.validate_file(doc[0])["handoff"]
            cases+=1
        # Simultaneous cross-language writers serialize; no duplicate artifact.
        workspace=fixture("competing")
        doc=document(workspace,"competing.json")
        code="from runner.handoffs import collect;import sys,json;print(json.dumps(collect(sys.argv[1],sys.argv[2],expected_sha256=sys.argv[3],expected_tasks_revision=sys.argv[4])))"
        commands=[[binary,*args(workspace,doc)],[sys.executable,"-c",code,str(workspace),str(doc[0]),doc[1],doc[2]]]
        processes=[subprocess.Popen(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE) for command in commands]
        try:
            results=[]
            for process in processes:
                output,error=process.communicate(timeout=15);assert process.returncode==0,error
                results.append(json.loads(output))
            assert results[0]["collection_id"]==results[1]["collection_id"]
            assert {item["publication"] for item in results}=={"created","existing"}
            assert len(handoffs.list_collections(workspace)["collections"])==1
        finally:
            for process in processes:
                if process.poll() is None:process.kill()
                process.communicate(timeout=5)
        cases+=1
        # Active authority wins over bad projection, and unrelated controls are inert.
        workspace=fixture("active-ledger")
        revision=coven.read_task_snapshot(workspace)[0]
        invoke("state","import-tasks",workspace,"--expect-revision",revision,"--json")
        doc=document(workspace,"active.json")
        (workspace/"state/tasks.json").write_text("bad projection must remain")
        (workspace/"state/execution").mkdir();(workspace/"state/execution/control.json").write_text("bad unrelated execution")
        (workspace/"state/actions").mkdir();(workspace/"state/actions/control.json").write_text("bad unrelated action")
        before=snapshot(workspace);result=native(workspace,doc);after=snapshot(workspace)
        assert result["task_binding"]["revision"]==doc[2] and all(after[key]==value for key,value in before.items())
        cases+=1
        if os.name=="posix":
            # Opening any of these as data would reject/block; successful capture
            # demonstrates the narrow context avoids projection and other controls.
            for name in ("state/tasks.json","state/execution/control.json","state/actions/control.json"):
                path=workspace/name;path.unlink();os.mkfifo(path)
            before=snapshot(workspace);native(workspace,doc);after=snapshot(workspace)
            assert after==before
            cases+=1
        # Actual prepared attempt remains reserved after a collected-completed claim.
        workspace=fixture("prepared")
        doc=document(workspace,"prepared.json")
        invoke("execution","enable",workspace,"--expect-execution-revision",task_attempts.DISABLED,"--request-id","enable","--json")
        invoke("execution","enroll",workspace,"task","--expect-execution-revision",task_attempts.Store(workspace).read().revision,
            "--expect-tasks-revision",doc[2],"--request-id","enroll","--json")
        intent=root/"intent.json";intent.write_text(json.dumps({"schema_version":1,"kind":"task-intent","task_id":"task","owner":"operator",
            "acceptance_criteria":[{"id":"review","text":"Not fulfilled by collection"}]}))
        invoke("task","attempt","prepare",workspace,"task","--intent",intent,"--expect-execution-revision",task_attempts.Store(workspace).read().revision,
            "--expect-tasks-revision",doc[2],"--request-id","prepare","--json")
        before=snapshot(workspace);native(workspace,doc);after=snapshot(workspace)
        assert all(after[key]==value for key,value in before.items())
        invoke("task","assign",workspace,"task","operator",expected=1)
        cases+=1
        # Deterministic input/context failures do not create lock/output archives.
        for mode in ("hash","stale","owner","roster","duplicate-source","pending","corrupt-ledger","symlink-output","capacity"):
            workspace=fixture("reject-"+mode);doc=document(workspace,mode+".json")
            if mode=="hash":doc=(doc[0],"0"*64,doc[2])
            elif mode=="stale":(workspace/"state/tasks.json").write_text("[]")
            elif mode=="owner":
                value=json.loads(doc[0].read_text());value["task"]["owner"]="other";doc[0].write_text(json.dumps(value));doc=(doc[0],handoffs.validate_file(doc[0])["handoff_sha256"],doc[2])
            elif mode=="roster":
                team=json.loads((workspace/"coven.json").read_text());team["agents"]=["other"];(workspace/"coven.json").write_text(json.dumps(team))
            elif mode=="duplicate-source":
                raw=(workspace/"state/tasks.json").read_bytes().replace(b'"id":"task"',b'"id":"task","id":"task"')
                (workspace/"state/tasks.json").write_bytes(raw)
                doc=document(workspace,mode+".json")
            elif mode=="pending":(workspace/"state/task-ledger").mkdir()
            elif mode=="corrupt-ledger":(workspace/"state/task-authority.json").write_text("null")
            elif mode=="symlink-output":(workspace/"output/handoffs").symlink_to(root/"missing")
            else:
                folder=workspace/"output/handoffs";folder.mkdir()
                for index in range(255):(folder/f".handoff-{index}.tmp").write_text("")
            before=snapshot(workspace);native(workspace,doc,1);after=snapshot(workspace)
            # Capacity is deliberately decided under the lock; other deterministic
            # failures should not even create coordination state.
            allowed={"state/.workspace.lock"} if mode=="capacity" else set()
            assert all(after[key]==value for key,value in before.items())
            assert set(after)-set(before)<=allowed,(mode,set(after)-set(before))
            cases+=1
        workspace=fixture("unsupported-native-context-profile")
        source=workspace/"state/tasks.json"
        value=json.loads(source.read_text());value[0]["extra"]["array"]=[0]*65537;source.write_text(json.dumps(value))
        doc=document(workspace,"limited-context.json")
        # Python accepts this finite depth-16/byte-bounded extension; the native
        # value-profile item bound is an explicit migration limitation.
        python(workspace,doc)
        before=snapshot(workspace);failure=native(workspace,doc,1)
        assert b'unsupported_json_array_profile' in failure.stderr
        assert snapshot(workspace)==before
        cases+=1
    print(json.dumps({"native_python_collection_cases":cases,"native_task_or_runtime_mutations":False}))


if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("--binary",required=True);main(parser.parse_args().binary)
