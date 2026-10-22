"""Offline CLI regressions; no agent runtimes or model calls are required."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest import mock

from runner import coven


ROOT = Path(__file__).resolve().parents[1]


def snapshot(root):
    """Capture entries without following symlinks, including dangling links."""
    result = {}
    for path in root.rglob("*"):
        key = str(path.relative_to(root))
        if path.is_symlink():
            result[key] = ("symlink", os.readlink(path))
        elif path.is_dir():
            result[key] = ("dir",)
        else:
            result[key] = ("file", path.read_bytes(), path.stat().st_mtime_ns)
    return result


class CovenTestCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coven-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.workspace = self.root / "workspace with spaces"
        self.env = mock.patch.dict(os.environ, {"COVEN_MULTIPLEXER": ""})
        self.env.start()
        self.addCleanup(self.env.stop)

    def invoke(self, *args, error=None):
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            if error:
                with self.assertRaises(SystemExit) as raised:
                    coven.main(list(map(str, args)))
                self.assertNotEqual(raised.exception.code, 0)
            else:
                self.assertEqual(coven.main(list(map(str, args))), 0)
        if error:
            self.assertIn(error, errors.getvalue())
        return output.getvalue()

    def read_json(self, relative):
        return json.loads((self.workspace / relative).read_text())


class CovenTests(CovenTestCase):
    def test_default_seed_has_correct_roles_and_canonical_owners(self):
        with mock.patch.object(coven.subprocess, "run", side_effect=AssertionError("No runtime calls allowed")):
            self.invoke("init", self.workspace)
        agents = self.read_json("state/agents.json")
        self.assertEqual([a["id"] for a in agents], ["clippy", "srikanth", "jamal", "kris", "eric"])
        for agent, role in zip(agents, ("Planner.", "Tech lead.", "Senior developer.", "Frontend/UI", "QA.")):
            self.assertTrue(agent["role"].startswith(role))
            self.assertIn(agent["role"], (self.workspace / f"prompts/{agent['id']}.md").read_text())
        tasks = self.read_json("state/tasks.json")
        self.assertEqual([t["owner"] for t in tasks], ["clippy", "srikanth", "jamal", "kris", "eric", "srikanth"])
        self.assertTrue(all(t["status"] == "assigned" for t in tasks))
        for relative in ("coven.toml", "tmux/coven.toml"):
            tomllib.loads((self.workspace / relative).read_text())
        self.assertIn("| `core-chatbot` | assigned | jamal |", self.invoke("status", self.workspace))

    def test_blank_init_preserves_unrelated_project_files(self):
        self.workspace.mkdir()
        sentinel = self.workspace / "existing-project.txt"
        sentinel.write_bytes(b"user project\n")
        self.invoke("init", self.workspace, "--blank", "--agents", "Architect,Reviewer", "--force")
        self.assertEqual(sentinel.read_bytes(), b"user project\n")
        self.assertEqual(self.read_json("coven.json")["agents"], ["architect", "reviewer"])
        self.assertEqual(self.read_json("state/tasks.json"), [])
        self.assertFalse((self.workspace / "spec.md").exists())

    def test_reinit_never_changes_an_existing_workspace(self):
        self.invoke("init", self.workspace)
        for relative in ("goal.md", "dashboard.md", "agents/jamal.md", "prompts/jamal.md",
                         "checkpoints/plan-approval.md", "logs/messages.jsonl"):
            with (self.workspace / relative).open("a") as handle:
                handle.write("\nUser-authored content; preserve me.\n")
        (self.workspace / "work/dirty.py").write_text("# Uncommitted work\n")
        (self.workspace / "state/lead.json").write_text('{"agent": "srikanth"}\n')
        before = snapshot(self.workspace)
        for flags in ([], ["--force"], ["--blank"], ["--blank", "--force"]):
            with self.subTest(flags=flags):
                self.invoke("init", self.workspace, *flags, error="Refusing to initialize")
                self.assertEqual(snapshot(self.workspace), before)

    def test_partial_workspace_collisions_are_refused(self):
        for index, name in enumerate(coven.INIT_RESERVED_NAMES):
            for kind in ("file", "directory", "symlink", "dangling-symlink"):
                with self.subTest(name=name, kind=kind):
                    workspace = self.root / f"partial-{index}-{kind}"
                    workspace.mkdir()
                    path = workspace / name
                    outside = self.root / f"outside-{index}-{kind}"
                    if kind == "file":
                        path.write_text("Existing user data")
                    elif kind == "directory":
                        path.mkdir()
                    elif kind == "symlink":
                        outside.mkdir()
                        (outside / "sentinel").write_text("Keep")
                        path.symlink_to(outside, target_is_directory=True)
                    else:
                        path.symlink_to(outside)
                    before = snapshot(self.root)
                    self.invoke("init", workspace, "--blank", "--force", error="Refusing to initialize")
                    self.assertEqual(snapshot(self.root), before)

    def test_workspace_path_must_be_a_directory(self):
        self.workspace.write_text("Not a directory")
        self.invoke("init", self.workspace, error="not a directory")
        self.assertEqual(self.workspace.read_text(), "Not a directory")

    def test_invalid_inputs_do_not_create_workspace(self):
        cases = [
            (["--agents", "Jamal,jamal"], "Duplicate agent ID"),
            (["--agents", "Dr Oom,dr-oom"], "Duplicate agent ID"),
            (["--agents", "_"], "letter or number"),
            (["--agents", ""], "letter or number"),
            (["--name", "!!!"], "letter or number"),
            (["--opencode-command", "   "], "must not be empty"),
            (["--example", "not-an-example"], "Unknown coven example"),
        ]
        for flags, error in cases:
            with self.subTest(flags=flags):
                self.invoke("init", self.workspace, *flags, error=error)
                self.assertFalse(self.workspace.exists())

    def test_invalid_runtime_override_does_not_create_workspace(self):
        with mock.patch.dict(os.environ, {"COVEN_MULTIPLEXER": "unknown"}):
            self.invoke("init", self.workspace, error="Unsupported multiplexer")
        self.assertFalse(self.workspace.exists())

    def test_invalid_example_records_fail_before_any_writes(self):
        cases = [
            {}, ["not-an-object"], [{"id": "../escape"}], [{"id": "/absolute"}],
            [{"id": "same"}, {"id": "same"}], [{"owner": 1}], [{"owner": "!!!"}],
            [{"title": ["invalid"]}], [{"notes": {}}],
        ]
        for tasks in cases:
            with self.subTest(tasks=tasks), mock.patch.object(coven, "read_example_json", return_value=tasks):
                self.invoke("init", self.workspace, error="Invalid initialization inputs")
                self.assertFalse(self.workspace.exists())

    def test_invalid_seed_json_fails_before_any_writes(self):
        example = self.root / "example"
        example.mkdir()
        (example / "example.json").write_text("not JSON")
        with mock.patch.object(coven, "EXAMPLES_DIR", self.root):
            self.invoke("init", self.workspace, "--example", "example", error="Invalid example")
            self.assertFalse(self.workspace.exists())
            (example / "example.json").write_text('{"id": "example"}')
            (example / "tasks.json").write_text("truncated JSON[")
            self.invoke("init", self.workspace, "--example", "example", error="Invalid initialization inputs")
            self.assertFalse(self.workspace.exists())

    def test_custom_team_leaves_omitted_example_owners_unassigned(self):
        self.invoke("init", self.workspace, "--agents", "Jamal,Reviewer")
        tasks = self.read_json("state/tasks.json")
        self.assertEqual([t["id"] for t in tasks if t["owner"]], ["core-chatbot"])
        self.assertTrue(all(t["status"] == "pending" for t in tasks if not t["owner"]))

    def test_late_seed_read_failure_does_not_create_workspace(self):
        example = self.root / "example"
        example.mkdir()
        (example / "example.json").write_text('{"id": "example"}')
        (example / "prompt-context.md").write_bytes(b"\xff\xfe")
        with mock.patch.object(coven, "EXAMPLES_DIR", self.root):
            self.invoke("init", self.workspace, "--example", "example", error="Invalid initialization inputs")
        self.assertFalse(self.workspace.exists())

    def test_exclusive_create_failure_preserves_colliding_and_partial_files(self):
        original_open = Path.open
        target = self.workspace.resolve() / "state/tasks.json"

        def inject_collision(path, mode="r", *args, **kwargs):
            if path == target and mode == "x":
                with original_open(path, "w") as handle:
                    handle.write("Newly appeared user data")
            return original_open(path, mode, *args, **kwargs)

        with mock.patch.object(Path, "open", new=inject_collision):
            self.invoke("init", self.workspace, error="partial files have been preserved")
        self.assertEqual(target.read_text(), "Newly appeared user data")
        self.assertTrue((self.workspace / "coven.json").is_file())
        self.assertFalse((self.workspace / "logs/events.jsonl").exists())
        before = snapshot(self.workspace)
        self.invoke("init", self.workspace, "--force", error="Refusing to initialize")
        self.assertEqual(snapshot(self.workspace), before)

    def test_task_owner_inputs_resolve_to_existing_ids(self):
        self.invoke("init", self.workspace, "--blank")
        self.invoke("task", "add", self.workspace, "Implement feature", "--owner", "JAMAL")
        task = self.read_json("state/tasks.json")[0]
        self.assertEqual((task["owner"], task["status"]), ("jamal", "assigned"))
        self.invoke("task", "assign", self.workspace, task["id"], "Srikanth")
        self.assertEqual(self.read_json("state/tasks.json")[0]["owner"], "srikanth")
        before = snapshot(self.workspace)
        self.invoke("task", "add", self.workspace, "Bad owner", "--owner", "ghost", error="Unknown or ambiguous agent")
        self.invoke("task", "assign", self.workspace, task["id"], "ghost", error="Unknown or ambiguous agent")
        self.assertEqual(snapshot(self.workspace), before)

    def test_owner_resolution_preserves_legacy_identity_and_rejects_ambiguity(self):
        self.assertEqual(coven.resolve_agent({"agents": ["Jamal"]}, "jamal"), "Jamal")
        self.assertEqual(coven.resolve_agent({"agents": ["jamal", "_"]}, "JAMAL"), "jamal")
        self.assertEqual(coven.resolve_agent({"agents": ["jamal", "_"]}, "_"), "_")
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.resolve_agent({"agents": ["Jamal", "jamal"]}, "Jamal")

    def test_legacy_roster_does_not_poison_task_owner_mutations(self):
        self.invoke("init", self.workspace, "--blank", "--agents", "Jamal")
        team = self.read_json("coven.json")
        team["agents"].append("_")
        (self.workspace / "coven.json").write_text(json.dumps(team))
        self.invoke("task", "add", self.workspace, "Legacy task", "--owner", "JAMAL")
        task_id = self.read_json("state/tasks.json")[0]["id"]
        self.invoke("task", "assign", self.workspace, task_id, "jamal")
        self.assertEqual(self.read_json("state/tasks.json")[0]["owner"], "jamal")
        self.invoke("task", "assign", self.workspace, task_id, "_")
        self.assertEqual(self.read_json("state/tasks.json")[0]["owner"], "_")

    def test_wrapper_creates_and_refuses_reinit_without_runtime(self):
        env = {key: value for key, value in os.environ.items()
               if key not in {"COVEN_HOME", "COVEN_COMMAND", "COVEN_MULTIPLEXER"}}
        command = [str(ROOT / "bin/coven"), "init", str(self.workspace)]
        created = subprocess.run(command, env=env, text=True, capture_output=True, timeout=20)
        self.assertEqual(created.returncode, 0, created.stderr)
        before = snapshot(self.workspace)
        refused = subprocess.run(command + ["--force"], env=env, text=True, capture_output=True, timeout=20)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("--force does not authorize a reset", refused.stderr)
        self.assertNotIn("Traceback", refused.stderr)
        self.assertEqual(snapshot(self.workspace), before)


if __name__ == "__main__":
    unittest.main()
