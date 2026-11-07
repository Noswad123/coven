"""Snapshot task CLI/preflight oracle and explicit unsupported-authority tests."""
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest import mock

sys.set_int_max_str_digits(4300)
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from runner import coven, task_attempts


def main(binary):
    cases=0
    with tempfile.TemporaryDirectory(prefix="native-tasks-") as temporary:
        root=Path(temporary)
        def invoke(*args):
            out,err=io.StringIO(),io.StringIO()
            with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
                try: status=coven.main(list(map(str,args)))
                except SystemExit as failure: status=failure.code
            return status,out.getvalue(),err.getvalue()
        def init(name):
            workspace=root/name
            assert invoke("init",workspace,"--blank","--agents","operator")[0]==0
            return workspace
        def snapshot(path):
            return {str(item.relative_to(path)):("symlink",os.readlink(item)) if item.is_symlink() else ("dir",)
                if item.is_dir() else ("file",item.read_bytes(),item.stat().st_mtime_ns) for item in path.rglob("*")}
        def native(workspace, expected=None, clean_path=False, cwd=None):
            env={**os.environ,"COVEN_MULTIPLEXER":""}
            if clean_path: env["PATH"]=str(root/"no-tools")
            result=subprocess.run([binary,"task","list",str(workspace),"--json"],cwd=cwd or root,env=env,capture_output=True,timeout=5)
            if expected is not None: assert result.returncode==expected,(result.returncode,result.stdout,result.stderr)
            return result
        workspace=init("control with spaces λ")
        source=workspace/"state/tasks.json"
        initial=source.read_bytes()
        def compare(raw=None, native_refusal=False):
            nonlocal cases
            if raw is None: source.unlink(missing_ok=True)
            else: source.write_bytes(raw)
            before=snapshot(workspace)
            status,out,error=invoke("task","list",workspace,"--json")
            result=native(workspace)
            if native_refusal:
                assert result.returncode!=0,(result.stdout,result.stderr)
            else:
                assert (result.returncode==0)==(status==0),(cases,status,error,result.stdout,result.stderr)
                if status==0:
                    # Canonical bytes preserve integer/float/signed-zero distinctions.
                    python=task_attempts.canonical(json.loads(out))
                    rust=task_attempts.canonical(json.loads(result.stdout))
                    assert rust==python,(out,result.stdout)
            assert snapshot(workspace)==before
            cases+=1
        compare(None)
        compare(b'[]')
        compare(b'[ ]\r\n')
        task={"id":"Exact_ID","title":"","status":"custom legacy status","extra":{
            "b":1.0,"a":-0.0,"big":10**100,"nested":[None,True,"λ 🦊\x00\u0085"],"key":"é"}}
        for variant in ({}, {"owner":None}, {"owner":"not-in-roster"}, {"notes":"","created_at":"not a timestamp"}):
            compare(json.dumps([{**task,**variant}],ensure_ascii=False).encode())
        compare(b'[{"id":"first","id":"second","title":"old","title":"new","status":""}]')
        compare(json.dumps([{**task,"id":"Task"},{**task,"id":"task"}]).encode())
        for data in ([task,task],{},None,[{"id":"x","title":True,"status":"pending"}],
                     [{**task,"owner":False}],[{**task,"notes":None}],[{**task,"id":"space id"}]):
            compare(json.dumps(data).encode())
        for raw in (b'',b'{',b'\xff',b'\xef\xbb\xbf[]'):
            compare(raw)
        compare(b'[{"id":"t","title":"x","status":"x","extension":NaN}]',True)
        compare(b'[{"id":"t","title":"x","status":"x","extension":"\\ud800"}]',True)
        source.write_bytes(initial)
        # The default command validates its surrounding workspace, not just tasks.
        files={name:(workspace/name).read_bytes() if (workspace/name).exists() else None for name in (
            "coven.json","coven.toml","state/agents.json","state/lead.json","checkpoints/plan-approval.md")}
        changes=[("coven.json",b'{}'),("coven.json",b'{"name":"ok","agents":["A","a"]}'),
            ("coven.json",b'{"name":"ok","agents":["a"],"version":true}'),
            ("coven.toml",b'[runtime]\nmultiplexer="bad"\n'),("coven.toml",b'[runtime]\nopencode_command=1\n'),
            ("coven.toml",b'broken TOML !'),("state/agents.json",b'null'),("state/lead.json",b'{}'),
            ("checkpoints/plan-approval.md",b'\xff')]
        for name,raw in changes:
            selected=workspace/name
            selected.write_bytes(raw)
            compare(initial)
            if files[name] is None: selected.unlink()
            else: selected.write_bytes(files[name])
        bad=workspace/"checkpoints/not.valid.MD"
        bad.write_text("checkpoint text")
        compare(initial)
        bad.unlink()
        # Scalar IDs are exact, not routed through roster aliases/current owners.
        compare(json.dumps([{**task,"id":"_","owner":"-"}]).encode())
        before=snapshot(workspace)
        native(workspace,0,True)
        assert snapshot(workspace)==before
        cases+=1
        home=root/"chosen-home"
        real=init("chosen-home/real")
        fake=init("~/real")
        (fake/"state/tasks.json").write_text('[{"id":"wrong","title":"wrong workspace","status":"pending"}]')
        with mock.patch.dict(os.environ,{"HOME":str(home)}):
            expected=invoke("task","list","~/real","--json")
            assert expected[0]==0
            assert json.loads(native("~/real",0).stdout)==json.loads(expected[1])
            with contextlib.chdir(real):
                expected=invoke("task","list","","--json")
            assert json.loads(native("",0,cwd=real).stdout)==json.loads(expected[1])
        assert b'unsupported_named_user_expansion' in native("~unsupported-user/real",1).stderr
        cases+=3
        # Any unported authority evidence refuses BEFORE even snapshot type checks.
        source.unlink()
        os.mkfifo(source)
        for relative, directory in (("state/task-authority.json",False),("state/task-ledger",True),
                                    ("state/runtime-launch-control.json",False),("state/actions",True)):
            selected=workspace/relative
            if directory: selected.mkdir()
            else: selected.write_text("unparsed unsupported authority")
            result=native(workspace,1)
            code=b'invalid_compat_json' if relative=="state/task-authority.json" else b'unsupported_native_authority'
            assert code in result.stderr,result.stderr
            if directory: selected.rmdir()
            else: selected.unlink()
            cases+=1
        source.unlink()
        source.write_bytes(initial)
        marker=workspace/"state/task-authority.json"
        marker.symlink_to(root/"missing")
        assert b'unsafe_archive_path' in native(workspace,1).stderr
        marker.unlink()
        cases+=1
        # The new authority reader validates the ledger, not its stale projection.
        revision=coven.read_task_snapshot(workspace)[0]
        assert invoke("state","import-tasks",workspace,"--expect-revision",revision,"--json")[0]==0
        source.write_text("broken projection")
        before=snapshot(workspace)
        result=native(workspace,0)
        assert json.loads(result.stdout)==json.loads(invoke("task","list",workspace,"--json")[1])
        assert snapshot(workspace)==before
        cases+=1
    print(json.dumps({"snapshot_cli_cases":cases,"native_writes_enabled":False}))


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--binary",required=True)
    main(parser.parse_args().binary)
