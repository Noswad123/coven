"""No real agents or servers: real Git fixtures and mocked backend effects only."""
import contextlib
import difflib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from runner import coven


def main(binary):
    os.environ["COVEN_COMMAND"] = "coven"
    def cli(*args, ok=True, env=None):
        result = subprocess.run([binary, *map(str, args)], capture_output=True, text=True, env=env)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return result

    def git(path, *args):
        env = {"PATH": os.environ["PATH"], "HOME": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null"}
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "-C", str(path), *args], env=env,
                       check=True, capture_output=True)

    with tempfile.TemporaryDirectory(prefix="isolation-") as temporary:
        base = Path(temporary)
        control = base / "control 'quoted $path"
        cli("init", control, "--blank", "--agents", "implementer,reviewer")
        git(control, "init")
        control_alias = base / "control-index-alias"
        control_alias.mkdir()
        (control_alias / ".git").symlink_to(control / ".git", target_is_directory=True)
        repo = base / "source one"
        repo.mkdir()
        git(repo, "init")
        git(repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "fixture")
        linked = base / "linked 'two"
        git(repo, "worktree", "add", "-b", "isolation-fixture", str(linked))
        independent = base / "independent"
        independent.mkdir()
        git(independent, "init")
        bare = base / "bare"
        bare.mkdir()
        git(bare, "init", "--bare")
        shared_index = base / "shared-index-alias"
        shared_index.mkdir()
        (shared_index / ".git").symlink_to(repo / ".git", target_is_directory=True)
        subdir = repo / "subdir"
        subdir.mkdir()
        alias = base / "alias"
        alias.symlink_to(repo, target_is_directory=True)
        config = json.loads((control / "coven.json").read_text())
        original_toml = (control / "coven.toml").read_text()
        role = control / "prompts/implementer.md"
        role.write_text("Custom role prompt. Do not overwrite.\n")

        def configure(mapping, toml=False):
            config.pop("runtime", None)
            (control / "coven.toml").write_text(original_toml)
            if toml:
                # Deliberately invalid JSON mapping demonstrates whole-table override.
                config["runtime"] = {"agent_working_directories": {"wrong": "/missing"}}
                text = "\n[runtime.agent_working_directories]\n" + "".join(f"{k} = {json.dumps(str(v))}\n" for k, v in mapping.items())
                (control / "coven.toml").write_text(original_toml + text)
            else:
                config["runtime"] = {"agent_working_directories": mapping}
            (control / "coven.json").write_text(json.dumps(config))

        for mapping in ({"implementer": str(repo), "reviewer": str(linked)},
                        {"implementer": str(repo), "reviewer": str(independent)}):
            for toml in (False, True):
                configure(mapping, toml)
                team = coven.read_team_config(control)
                directories = coven.resolve_agent_directories(control, team)
                cli("refresh", control)
                expected = coven.prepare_refresh(control)
                actual = (control / "tmux/coven.toml").read_text()
                assert actual == expected[0], "".join(difflib.unified_diff(expected[0].splitlines(True), actual.splitlines(True)))
                manifest = tomllib.loads(expected[0])
                windows = manifest["sessions"][0]["windows"]
                assert all(p["dir"] == "${COVEN_DIR}" for p in windows[0]["panes"])
                assert [w["panes"][0]["dir"] for w in windows[1:]] == list(map(str, directories.values()))
                assert role.read_text() == "Custom role prompt. Do not overwrite.\n"
                for agent in mapping:
                    assert (control / f"prompts/{agent}.start.txt").read_text() == expected[2][f"prompts/{agent}.start.txt"]
                    assert "Execution repository:" in expected[2][f"prompts/{agent}.start.txt"]
                for backend in ("tmux", "herdr"):
                    team["multiplexer"] = backend
                    staged = coven.stage_agent_launches(control, team)
                    assert str(control / "prompts/implementer.start.txt").replace("'", "'\"'\"'") in staged["implementer"][0]
                    assert staged["implementer"][1] == repo
                before = sorted(p.relative_to(control).as_posix() for p in control.rglob("*"))
                # Capture itself refuses before constructing/publishing a v1 plan.
                with contextlib.redirect_stderr(io.StringIO()):
                    try:
                        coven.runtime_launch_inputs(control)
                    except coven.actions.ActionError as exc:
                        assert str(exc) == "runtime_launch_v1_agent_directories_unsupported"
                    else:
                        raise AssertionError("Python v1 accepted isolated cwds")
                assert before == sorted(p.relative_to(control).as_posix() for p in control.rglob("*"))

        invalid = [None, {}, {"implementer": str(repo)},
                   {"implementer": str(repo), "reviewer": str(linked), "extra": str(independent)},
                   {"implementer": str(repo), "reviewer": str(alias)},
                   {"implementer": str(repo), "reviewer": str(shared_index)},
                   {"implementer": str(bare), "reviewer": str(linked)},
                   {"implementer": str(base), "reviewer": str(linked)},
                   {"implementer": str(subdir), "reviewer": str(linked)},
                   {"implementer": str(control), "reviewer": str(linked)},
                   {"implementer": str(control_alias), "reviewer": str(linked)},
                   {"implementer": "relative", "reviewer": str(linked)},
                   {"implementer": str(base / "missing"), "reviewer": str(linked)},
                   {"implementer": 42, "reviewer": str(linked)}]
        for mapping in invalid:
            configure(mapping)
            before = {p.relative_to(control): p.read_bytes() for p in control.rglob("*") if p.is_file()}
            cli("refresh", control, ok=False)
            cli("restart", control, ok=False)
            cli("agent", "restart", control, "implementer", ok=False)
            with contextlib.redirect_stderr(io.StringIO()):
                try:
                    coven.resolve_agent_directories(control, coven.read_team_config(control))
                except SystemExit:
                    pass
                else:
                    raise AssertionError(f"Python accepted {mapping}")
            assert before == {p.relative_to(control): p.read_bytes() for p in control.rglob("*") if p.is_file()}

        configure({"implementer": str(repo), "reviewer": str(linked)})
        result = cli("restart", control, ok=False)
        assert "agent_working_directories_refresh_required" in result.stderr
        result = cli("bootstrap", control, ok=False)
        assert "agent_working_directories_refresh_required" in result.stderr
        cli("refresh", control)
        # A late roster member's missing/stale starter must fail at the Python
        # command entrypoint, before requested history, either lock, or transport.
        starter = control / "prompts/reviewer.start.txt"
        original_starter = starter.read_bytes()
        try:
            for contents in (None, b"stale relative-path instructions\n"):
                if contents is None:
                    starter.unlink()
                else:
                    starter.write_bytes(contents)
                for lock in ("state/.workspace.lock", "state/.runtime-transport.lock"):
                    (control / lock).unlink(missing_ok=True)
                before = {p.relative_to(control): p.read_bytes() if p.is_file() else None for p in control.rglob("*")}
                for backend in ("tmux", "herdr"):
                    for command in (["up", str(control)], ["restart", str(control)], ["up", "--replace", str(control)]):
                        with patch.dict(os.environ, {"COVEN_MULTIPLEXER": backend}), \
                                patch.object(coven, "run_tmux") as tmux, patch.object(coven, "run_herdr") as herdr, \
                                patch.object(coven.subprocess, "run") as subprocess_run, \
                                contextlib.redirect_stderr(io.StringIO()):
                            try:
                                coven.main(command)
                            except SystemExit as exc:
                                assert exc.code == 1
                            else:
                                raise AssertionError("Python launch accepted a missing/stale isolated starter")
                            tmux.assert_not_called()
                            herdr.assert_not_called()
                            subprocess_run.assert_not_called()
                        assert before == {p.relative_to(control): p.read_bytes() if p.is_file() else None for p in control.rglob("*")}
        finally:
            starter.write_bytes(original_starter)
        @contextlib.contextmanager
        def stale_after_admission(workspace):
            starter.write_bytes(b"changed after preflight\n")
            yield
        for backend in ("tmux", "herdr"):
            try:
                with patch.dict(os.environ, {"COVEN_MULTIPLEXER": backend}), \
                        patch.object(coven, "legacy_runtime_transport", stale_after_admission), \
                        patch.object(coven, "run_tmux") as tmux, patch.object(coven, "run_herdr") as herdr, \
                        contextlib.redirect_stderr(io.StringIO()):
                    try:
                        coven.main(["restart", str(control)])
                    except SystemExit as exc:
                        assert exc.code == 1
                    else:
                        raise AssertionError("Python launch did not revalidate starters after admission")
                    tmux.assert_not_called()
                    herdr.assert_not_called()
            finally:
                starter.write_bytes(original_starter)
        # These inert binaries record backend arguments; they never execute pane commands.
        backend_bin = base / "backend-bin"
        backend_bin.mkdir()
        transcript = base / "backend.jsonl"
        backend_source = '''#!/usr/bin/python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ["ISOLATION_TRANSCRIPT"], "a") as stream:
    stream.write(json.dumps({"argv": args, "cwd": os.getcwd(), "path": os.environ["PATH"]}) + "\\n")
if args[0] == "has-session":
    sys.exit(1 if os.environ["ISOLATION_MODE"] == "team" else 0)
if args[0] == "list-panes":
    if os.environ["ISOLATION_MODE"] == "stop":
        print("%1")
        sys.exit(0)
    sys.exit(1)
if args[0] == "split-window":
    print("%1")
elif args[:2] == ["workspace", "list"]:
    print(json.dumps({"result": {"workspaces": [] if os.environ["ISOLATION_MODE"] == "team" else [{"workspace_id":"w1", "label":os.environ["ISOLATION_LABEL"]}]}}))
elif args[:2] == ["workspace", "create"]:
    print(json.dumps({"result":{"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"p0"},"tab":{"tab_id":"t0"}}}))
elif args[:2] == ["tab", "create"]:
    print(json.dumps({"result":{"root_pane":{"pane_id":"p1"}}}))
elif args[:2] == ["tab", "list"]:
    print(json.dumps({"result":{"tabs":[{"workspace_id":"w1", "tab_id":"t1", "label":"agent-implementer"}] if os.environ["ISOLATION_MODE"] == "stop" else []}}))
elif args[:2] == ["pane", "list"]:
    print(json.dumps({"result":{"panes":[{"tab_id":"t1", "pane_id":"p1"}]}}))
else:
    print("{}")
'''
        for backend in ("tmux", "herdr"):
            executable = backend_bin / backend
            executable.write_text(backend_source)
            executable.chmod(0o755)
        for backend in ("tmux", "herdr"):
            for mode in ("team", "individual"):
                transcript.write_text("")
                env = {**os.environ, "PATH": f"{backend_bin}:{os.environ['PATH']}",
                       "COVEN_MULTIPLEXER": backend, "ISOLATION_MODE": mode,
                       "ISOLATION_TRANSCRIPT": str(transcript),
                       "ISOLATION_LABEL": coven.team_session_name(coven.read_team_config(control))}
                if mode == "team":
                    cli("restart", control, env=env)
                else:
                    cli("agent", "restart", control, "implementer", env=env)
                calls = [json.loads(line) for line in transcript.read_text().splitlines()]
                assert calls and all(c["cwd"] == os.getcwd() and c["path"] == env["PATH"] for c in calls)
                agent_calls = [c["argv"] for c in calls if c["argv"][0] == "new-window" or c["argv"][:2] == ["tab", "create"]]
                assert len(agent_calls) == (2 if mode == "team" else 1)
                for call in agent_calls:
                    agent = "implementer" if "agent-implementer" in call else "reviewer"
                    assert call[call.index("-c" if backend == "tmux" else "--cwd") + 1] == str(repo if agent == "implementer" else linked)
                scripts = [c["argv"][-1] for c in calls if c["argv"][0] == "new-window" or c["argv"][:2] == ["pane", "run"]]
                assert any("/prompts/implementer.start.txt" in script for script in scripts)
        # Losing the reviewer's mount must not disable the remaining control plane.
        configure({"implementer": str(repo), "reviewer": str(base / "unmounted-reviewer")})
        for backend in ("tmux", "herdr"):
            env = {**os.environ, "PATH": f"{backend_bin}:{os.environ['PATH']}",
                   "COVEN_MULTIPLEXER": backend, "ISOLATION_MODE": "stop",
                   "ISOLATION_TRANSCRIPT": str(transcript),
                   "ISOLATION_LABEL": coven.team_session_name(coven.read_team_config(control))}
            for python in (False, True):
                transcript.write_text("")
                commands = (["stop", str(control)], ["agent", "stop", str(control), "implementer"],
                            ["message", str(control), "implementer", "coven", "Reviewer mount unavailable"],
                            ["standup", str(control)])
                for command in commands:
                    if python:
                        with patch.dict(os.environ, env), contextlib.redirect_stdout(io.StringIO()):
                            assert coven.main(command) == 0
                    else:
                        cli(*command, env=env)
                calls = [json.loads(line)["argv"] for line in transcript.read_text().splitlines()]
                assert any(call[0] == "kill-session" if backend == "tmux" else call[:2] == ["workspace", "close"] for call in calls)
                assert any(call[0] == "kill-window" if backend == "tmux" else call[:2] == ["tab", "close"] for call in calls)
                before = {p.relative_to(control): p.read_bytes() if p.is_file() else None for p in control.rglob("*")}
                for command in (["up", str(control)], ["restart", str(control)],
                                ["agent", "start", str(control), "implementer"],
                                ["agent", "restart", str(control), "implementer"],
                                ["refresh", str(control)], ["bootstrap", str(control)]):
                    if python:
                        with patch.dict(os.environ, env), contextlib.redirect_stderr(io.StringIO()):
                            try:
                                coven.main(command)
                            except SystemExit as exc:
                                assert exc.code == 1
                            else:
                                raise AssertionError("Python launch accepted a missing execution root")
                    else:
                        cli(*command, env=env, ok=False)
                    assert before == {p.relative_to(control): p.read_bytes() if p.is_file() else None for p in control.rglob("*")}
                assert calls == [json.loads(line)["argv"] for line in transcript.read_text().splitlines()]
        cli("action", "enable-runtime-launch", control)
        before = {p.relative_to(control): p.read_bytes() for p in control.rglob("*") if p.is_file()}
        result = cli("action", "prepare-runtime-launch", control, ok=False)
        assert "runtime_launch_v1_agent_directories_unsupported" in result.stderr, result.stderr
        assert before == {p.relative_to(control): p.read_bytes() for p in control.rglob("*") if p.is_file()}
    print("isolation: JSON/TOML, linked/independent roots, asset parity, custom prompts, invalid preflight, reviewed refusal passed")


if __name__ == "__main__":
    main(sys.argv[1])
