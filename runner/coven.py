#!/usr/bin/env python3

from __future__ import annotations

import argparse
import base64
import hashlib
import io
from functools import wraps
import json
import os
import re
import shlex
import subprocess
import sys
import threading
import tomllib
import uuid
from contextlib import contextmanager, redirect_stderr
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

if __package__:
    from . import records, storage, task_ledger, checkpoints, actions, launch, workers, task_attempts, task_graphs, execution_v2, repositories, kitsune_observation, groups, group_publisher
    from . import kitsune_process_observation
    from . import djinn_observation
    from . import djinn_live_observation
    from . import group_live
    from . import interactive_sessions
    from . import session_inspection
else:
    import records
    import storage
    import task_ledger
    import checkpoints
    import actions
    import launch
    import workers
    import task_attempts
    import task_graphs
    import execution_v2
    import repositories
    import kitsune_observation
    import kitsune_process_observation
    import djinn_observation
    import djinn_live_observation
    import group_live
    import groups
    import group_publisher
    import interactive_sessions
    import session_inspection

DEFAULT_AGENTS = "Clippy,Srikanth,Jamal,Kris,Eric"
DEFAULT_OPENCODE_COMMAND = "oc"
DEFAULT_EXAMPLE_ID = "interactive-demo"
DEFAULT_MULTIPLEXER = "herdr"
SUPPORTED_MULTIPLEXERS = {"herdr", "tmux"}
# Init owns these names. Even partial workspaces must not be silently adopted.
INIT_RESERVED_NAMES = (
    "coven.json", "coven.toml", "goal.md", "spec.md", "dashboard.md",
    "agents", "prompts", "state", "logs", "checkpoints", "work", "output", "tmux",
)
MANAGED_DIRECTORIES = ("agents", "prompts", "state", "logs", "checkpoints", "work", "output", "tmux",
                       "state/task-ledger", "state/task-recovery", "state/actions")
MANAGED_FILES = (
    "coven.json", "coven.toml", "goal.md", "spec.md", "dashboard.md",
    "state/agents.json", "state/tasks.json", "state/lead.json", "state/.workspace.lock", "state/task-authority.json",
    "logs/events.jsonl", "logs/messages.jsonl", "tmux/coven.toml",
    "work/README.md", "output/README.md", "state/runtime-launch-control.json", "state/.runtime-transport.lock",
)
COVEN_DIR = Path(__file__).resolve().parents[1]
PROJECT_DIR = Path(__file__).resolve().parents[3]
EXAMPLES_DIR = COVEN_DIR / "examples"


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def info(message: str) -> None:
    print(f"[INFO] {message}")


def fail(message: str) -> None:
    print(f"[ERROR] {message}", file=sys.stderr)
    raise SystemExit(1)


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9_-]+", "-", value.strip().lower()).strip("-")
    if not re.search(r"[a-z0-9]", slug):
        raise ValueError("value must contain at least one letter or number")
    return slug


def parse_agents(raw: str) -> list[str]:
    agents: list[str] = []
    seen: set[str] = set()
    for item in raw.split(","):
        agent = slugify(item)
        if agent in seen:
            raise ValueError(f"Duplicate agent ID after normalization: {agent}")
        agents.append(agent)
        seen.add(agent)
    if not agents:
        raise ValueError("at least one agent is required")
    return agents


def workspace_path(path: str | None) -> Path:
    return Path(path or ".").expanduser().resolve()


def record_id(value: Any, kind: str) -> str:
    """Validate persisted identities without renaming them or excluding legacy `_`."""
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        fail(f"Invalid {kind} ID {value!r}; use only ASCII letters, digits, underscores, and hyphens")
    return value


def managed_path(workspace: Path, relative: str, *, directory: bool = False) -> Path:
    """Reject pre-existing managed symlinks/types; not a race-proof filesystem sandbox."""
    root = workspace.resolve()
    suffix = Path(relative)
    if suffix.is_absolute() or not suffix.parts or ".." in suffix.parts:
        fail(f"Unsafe workspace-relative path: {relative!r}")
    path = root
    for index, part in enumerate(suffix.parts):
        path = path / part
        if path.is_symlink():
            fail(f"Unsafe managed path (symlinks are not supported): {path}")
        if path.exists():
            want_directory = directory or index < len(suffix.parts) - 1
            if want_directory and not path.is_dir():
                fail(f"Managed path must be a directory: {path}")
            if not want_directory and not path.is_file():
                fail(f"Managed path must be a regular file: {path}")
    return path


def checkpoint_path(workspace: Path, checkpoint: Any) -> Path:
    return managed_path(workspace, f"checkpoints/{record_id(checkpoint, 'checkpoint')}.md")


def managed_text(workspace: Path, relative: str) -> str | None:
    path = managed_path(workspace, relative)
    try:
        return path.read_text(encoding="utf-8") if path.exists() else None
    except (ValueError, OSError) as exc:
        fail(f"Cannot read managed file {path}: {exc}")


def checkpoint_text(workspace: Path, checkpoint: Any) -> str | None:
    path = checkpoint_path(workspace, checkpoint)
    return managed_text(workspace, f"checkpoints/{path.name}")


def check_workspace_paths(workspace: Path, team: dict[str, Any] | None = None) -> None:
    """Validate the known control plane, not arbitrary user artifacts recursively."""
    for relative in MANAGED_DIRECTORIES:
        managed_path(workspace, relative, directory=True)
    managed_path(workspace, "state/task-authority.json")
    imported = task_ledger.LedgerStore(workspace).read() is not None
    for relative in MANAGED_FILES:
        if relative == "state/tasks.json" and imported:
            # Authority does not depend on projection readability/type. The
            # ledger reports unsafe/unreadable projections without following
            # them, and blocks new changes until explicit safe repair.
            continue
        managed_path(workspace, relative)
    for checkpoint in ("plan-approval", "pre-delivery-review"):
        checkpoint_text(workspace, checkpoint)
    checkpoint_dir = managed_path(workspace, "checkpoints", directory=True)
    for path in sorted(checkpoint_dir.iterdir()) if checkpoint_dir.exists() else []:
        # glob("*.md") misses .MD aliases even on case-insensitive filesystems.
        # Inspect actual names so both symlink/type and decoding checks apply.
        if path.suffix.lower() == ".md":
            record_id(path.stem, "checkpoint")
            managed_text(workspace, f"checkpoints/{path.name}")
    if team is not None:
        for agent in team_agents(team):
            managed_path(workspace, f"agents/{agent}.md")
            managed_path(workspace, f"prompts/{agent}.md")
            managed_path(workspace, f"prompts/{agent}.start.txt")


@dataclass(frozen=True)
class WorkspaceState:
    team: dict[str, Any]
    tasks: list[dict[str, Any]]
    agents: list[dict[str, Any]]
    lead: dict[str, Any] | None


def read_state_records(workspace: Path, relative: str, default: Any, validator: Any) -> Any:
    try:
        return validator(read_json(managed_path(workspace, relative), default))
    except (ValueError, OSError) as exc:
        fail(f"Invalid workspace record ({relative}): {exc}")


def read_tasks(workspace: Path) -> list[dict[str, Any]]:
    view = task_ledger.LedgerStore(workspace).read()
    if view is not None:
        return view.tasks
    return read_state_records(workspace, "state/tasks.json", [], records.tasks)


def read_agent_states(workspace: Path) -> list[dict[str, Any]]:
    return read_state_records(workspace, "state/agents.json", [], records.agents)


def load_workspace_state(workspace: Path, team: dict[str, Any] | None = None) -> WorkspaceState:
    """Read-only validation boundary, not an atomic/concurrent state snapshot."""
    check_workspace_paths(workspace)
    persisted = read_team_config(workspace)
    check_workspace_paths(workspace, persisted)
    if team is not None:
        try:
            records.team(team)
        except ValueError as exc:
            fail(f"Invalid supplied workspace configuration: {exc}")
        runtime_multiplexer(team)
        check_workspace_paths(workspace, team)
    return WorkspaceState(persisted, read_tasks(workspace), read_agent_states(workspace), read_lead(workspace))


def preflight_workspace_paths(workspace: Path, team: dict[str, Any] | None = None) -> None:
    # Shared mutations and runtime entrypoints must not trust a cached team alone.
    load_workspace_state(workspace, team)


@contextmanager
def workspace_mutation(workspace: Path, lock: storage.WorkspaceLock | None = None):
    # Deterministic invalid requests fail before creating coordination files.
    preflight_workspace_paths(workspace)
    path = managed_path(workspace, "state/.workspace.lock")
    with storage.workspace_lock(workspace, path, held=lock) as held:
        preflight_workspace_paths(workspace)
        yield held


def is_team_workspace(path: str | Path) -> bool:
    marker = Path(path).expanduser() / "coven.json"
    return marker.exists() or marker.is_symlink()


def split_optional_workspace(items: list[str]) -> tuple[Path, list[str]]:
    if items and is_team_workspace(items[0]):
        return workspace_path(items[0]), items[1:]
    return workspace_path("."), items


def q(value: str) -> str:
    return json.dumps(value)


def toml_string(value: str) -> str:
    # JSON surrogate-pair escapes are not legal TOML Unicode scalars. Keep valid
    # Unicode literal, escape ASCII DEL, and reject unpaired surrogates up front.
    value.encode("utf-8")
    return json.dumps(value, ensure_ascii=False).replace("\x7f", r"\u007f")


def cli_command() -> str:
    explicit = os.environ.get("COVEN_COMMAND")
    if explicit:
        return shlex.quote(explicit)
    wrapper = PROJECT_DIR / "bin" / "coven"
    if wrapper.exists():
        return shlex.quote(str(wrapper))
    return shlex.quote(sys.argv[0] or "coven")


def normalize_opencode_command(command: str) -> str:
    command = command.strip()
    if command.startswith("oc ") or command == "oc":
        args = command[2:].strip()
        suffix = f" {args}" if args else ""
        fallback = f"if command -v oc >/dev/null 2>&1; then exec oc{suffix}; else exec opencode{suffix}; fi"
        return f"zsh -ic {q(fallback)}"
    return command


def read_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    return json.loads(path.read_text(encoding="utf-8"))


def read_toml(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    with path.open("rb") as handle:
        return tomllib.load(handle)


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def append_event(workspace: Path, event_type: str, *, lock: storage.WorkspaceLock | None = None, **payload: Any) -> None:
    preflight_workspace_paths(workspace)
    event = {"event_id": str(uuid.uuid4()), "ts": now(), "type": event_type, **payload}
    path = managed_path(workspace, "logs/events.jsonl")
    storage.read_jsonl(path)
    storage.encode_json(event)
    with workspace_mutation(workspace, lock):
        path.parent.mkdir(parents=True, exist_ok=True)
        storage.append_jsonl(path, event)


def append_message(workspace: Path, sender: str, recipient: str, body: str, *, lock: storage.WorkspaceLock | None = None) -> None:
    preflight_workspace_paths(workspace)
    event_path = managed_path(workspace, "logs/events.jsonl")
    message = {"ts": now(), "from": sender, "to": recipient, "body": body}
    path = managed_path(workspace, "logs/messages.jsonl")
    storage.encode_json(message)
    storage.read_jsonl(event_path)
    storage.read_jsonl(path)
    with workspace_mutation(workspace, lock) as held:
        storage.read_jsonl(event_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        storage.append_jsonl(path, message)
        append_event(workspace, "message.sent", sender=sender, recipient=recipient, lock=held)


def load_team(workspace: Path) -> dict[str, Any]:
    return load_workspace_state(workspace).team


def read_team_config(workspace: Path) -> dict[str, Any]:
    path = managed_path(workspace, "coven.json")
    if not path.exists():
        fail(f"Not a coven workspace: {workspace} (missing coven.json)")
    try:
        team = records.team(read_json(path, {}))
        config = read_toml(managed_path(workspace, "coven.toml"))
    except (ValueError, OSError) as exc:
        fail(f"Invalid workspace configuration: {exc}")
    runtime = config.get("runtime") if isinstance(config, dict) else None
    if "runtime" in config and not isinstance(runtime, dict):
        fail("coven.toml [runtime] must be a table")
    if runtime is not None:
        for key in ("multiplexer", "opencode_command"):
            if key in runtime:
                value = runtime[key]
                if not isinstance(value, str) or not value.strip():
                    fail(f"coven.toml runtime.{key} must be a nonblank string")
                team[key] = value.strip()
    try:
        records.team(team)
    except ValueError as exc:
        fail(f"Invalid effective workspace configuration: {exc}")
    runtime_multiplexer(team)
    # Derived flag, never an authorization input from user metadata. Workspace-
    # aware launch entrypoints also check the actual control directly.
    team["_runtime_launch_mode"] = actions.ActionStore(workspace).read_control() is not None
    team["_workspace_root"] = str(workspace.resolve())
    return team


def runtime_multiplexer(team: dict[str, Any]) -> str:
    value = os.environ.get("COVEN_MULTIPLEXER") or team.get("multiplexer") or DEFAULT_MULTIPLEXER
    multiplexer = str(value).strip().lower()
    if multiplexer not in SUPPORTED_MULTIPLEXERS:
        fail(f"Unsupported multiplexer `{multiplexer}`. Choose one of: {', '.join(sorted(SUPPORTED_MULTIPLEXERS))}")
    return multiplexer


def visible_text(value: str) -> str:
    """Labels are presentation, never terminal controls or shell fragments."""
    return "".join(char if char.isprintable() else f"\\u{ord(char):04x}" for char in value)


def markdown_text(value: str) -> str:
    value = visible_text(value).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    return re.sub(r"([\\`*_{}\[\]()#+.!|~-])", r"\\\1", value)


def agent_display_name(team: dict[str, Any], agent: str) -> str:
    return team.get("display_names", {}).get(agent, agent)


def agent_label(team: dict[str, Any], agent: str) -> str:
    label = visible_text(agent_display_name(team, agent))
    return f"{label} ({agent})" if agent_display_name(team, agent) != agent else agent


def write_if_missing(path: Path, content: str, *, force: bool) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and not force:
        return False
    path.write_text(content, encoding="utf-8")
    return True


def load_example(example_id: str | None) -> dict[str, Any] | None:
    if not example_id:
        return None
    example_path = EXAMPLES_DIR / slugify(example_id)
    manifest_path = example_path / "example.json"
    if not manifest_path.exists():
        available = ", ".join(path.parent.name for path in sorted(EXAMPLES_DIR.glob("*/example.json"))) or "none"
        fail(f"Unknown coven example `{example_id}`. Available examples: {available}")
    manifest = read_json(manifest_path, {})
    if not isinstance(manifest, dict):
        fail(f"Invalid example manifest: {manifest_path}")
    manifest["id"] = str(manifest.get("id") or example_path.name)
    manifest["path"] = str(example_path)
    return manifest


def example_path(example: dict[str, Any] | None, relative: str) -> Path | None:
    if not example:
        return None
    return Path(str(example["path"])) / relative


def read_example_text(example: dict[str, Any] | None, relative: str, default: str = "") -> str:
    path = example_path(example, relative)
    if not path or not path.exists():
        return default
    return path.read_text(encoding="utf-8")


def read_example_json(example: dict[str, Any] | None, relative: str, default: Any) -> Any:
    path = example_path(example, relative)
    if not path or not path.exists():
        return default
    return read_json(path, default)


def example_goal_text(example: dict[str, Any] | None) -> str:
    goal = read_example_text(example, "goal.md", "# Goal\n\nDescribe the unified coven goal here.\n")
    return goal if goal.endswith("\n") else goal + "\n"


def unique_slug(base: str, existing: set[str]) -> str:
    slug = slugify(base)
    candidate = slug
    index = 2
    while candidate in existing:
        candidate = f"{slug}-{index}"
        index += 1
    existing.add(candidate)
    return candidate


def agent_role(agent: str) -> str:
    defaults = {
        "orchestrator": "Coordinate tasks, checkpoints, messages, and dashboard state.",
        "architect": "Break goals into plans, identify tradeoffs, and define checkpoints.",
        "implementer": "Make targeted changes and report validation results.",
        "reviewer": "Review diffs, test outcomes, risks, and readiness for user approval.",
        "clippy": "Planner. Turn the user goal into checkpoints, milestones, and clear task slices.",
        "srikanth": "Tech lead. Own architecture, integration decisions, and final technical direction.",
        "jamal": "Senior developer. Implement the core terminal application and keep changes shippable.",
        "kris": "Frontend/UI specialist, aka Dr Oom. Shape the terminal UI, flow, copy, and presentation polish.",
        "eric": "QA. Verify behavior, run the app, test edge cases, and confirm acceptance criteria.",
    }
    return defaults.get(agent.lower(), f"Contribute to the coven as `{agent}`.")


def prompt_text(agent: str, workspace: Path, example_context: str = "") -> str:
    context_block = f"\n{example_context.strip()}\n" if example_context.strip() else ""
    return f"""You are `{agent}` in a coven workspace.

Workspace: `{workspace}`
Role: {agent_role(agent)}

Read first:
1. `goal.md`
2. `spec.md` if present
3. `dashboard.md`
4. `coven.toml`
5. `agents/{agent}.md`
6. Output of `coven task list --json` (after import, `state/tasks.json` is only a projection)
7. `logs/messages.jsonl`
8. `checkpoints/*.md`

Rules:
- Treat task state as authoritative and the logs as history, not a replay ledger.
- Do not edit shared task JSON, logs, or dashboard files directly. Use `coven task list --json`, `coven task add`, `coven task assign`, `coven message`, and `coven refresh`.
- Use `--expect-revision` from `task list --json` when a task change depends on the state you reviewed. A stale revision requires reinspection, not an automatic retry.
- If task authority is an imported ledger, add/assign require `--request-id`. On a lost/uncertain response, retry the same key with the original exact request and original expected revision, not a new key or edited arguments.
- Completion/blocker lifecycle updates are not yet a task API: report them with `coven message {agent} coven "<summary>"` rather than changing task JSON manually.
- Consult the user at configured checkpoints before proceeding past them.
- Do not overwrite another agent's notes; append corrections or send messages.
- Keep work scoped to your current assignment.
- Put in-progress implementation files under `work/`.
- Put final runnable/demo artifacts and handoff output under `output/`.
{context_block}
"""


def starter_prompt_text(agent: str, workspace: Path) -> str:
    return f"""Read `{workspace}/prompts/{agent}.md` and begin as `{agent}`.

Your first actions:
1. Read `goal.md`, `spec.md` if present, `dashboard.md`, `coven.toml`, `agents/{agent}.md`, and `coven task list --json` output.
2. Find your assigned task(s).
3. Record your start through `coven message {agent} coven "Starting assigned work"`; do not append logs directly.
4. Inspect `checkpoints/*.md`, `work/`, and `output/`.
5. Work only within your role and current assignment.
6. If you reach a checkpoint requiring user approval, stop and send the request through `coven message {agent} user "Checkpoint <id> needs approval: <summary>"`.

Shared task JSON, logs, and dashboard files are not editing surfaces. Use the CLI
for task changes and messages. Existing role prompts may be customized; these
coordination rules still apply even if an older prompt asks for direct edits.
Inspect task authority through `coven state inspect --json`. In imported ledger
mode, add/assign require `--request-id`; retry a lost response with the same key
and original arguments/revision. Task JSON may be stale and is not authority.

Do not wait for another prompt unless blocked. Start now.
"""


def agent_state_text(agent: str) -> str:
    return f"""# Agent: {agent}

## Role

{agent_role(agent)}

## Current assignment

- None.

## Notes

- Append agent-local notes here.
"""


def work_readme() -> str:
    return """# Work Directory

Agents should put in-progress implementation files here.

For the starter interactive demo example, this is where draft source and design
handoffs should be created before the final runnable artifact lands in
`../output/demo.py`.
"""


def output_readme() -> str:
    return """# Output Directory

Agents should put final runnable artifacts, demos, summaries, and delivery notes
here.

For the starter example, this should include `demo.py`, an interactive
workspace-aware terminal demo agent runnable with:

```bash
python3 output/demo.py
```

The demo should answer questions about what was built, who did what, why
decisions were made, which checkpoints were approved, and what QA verified.
"""


def checkpoint_file(checkpoint_id: str, title: str, description: str) -> str:
    return f"""# Checkpoint: {title}

ID: `{checkpoint_id}`

Status: pending

## Purpose

{description}

## User decision

- [ ] Approved
- [ ] Rejected / needs changes

## Notes

- Agents should append requests, rationale, and decision notes here.
"""


def starter_tasks(agents: list[str], example: dict[str, Any] | None = None) -> list[dict[str, Any]]:
    owners = set(agents)

    def owner(agent: str) -> str | None:
        canonical = slugify(agent)
        return canonical if canonical in owners else None

    raw_tasks = read_example_json(example, "tasks.json", [])
    tasks: list[dict[str, Any]] = []
    task_ids: set[str] = set()
    if not isinstance(raw_tasks, list):
        raise ValueError("Example tasks.json must contain a list of task records")
    for raw_task in raw_tasks:
        if not isinstance(raw_task, dict):
            raise ValueError("Example tasks must be objects")
        for field in ("id", "title", "owner", "notes"):
            if raw_task.get(field) is not None and not isinstance(raw_task[field], str):
                raise ValueError(f"Example task {field} must be a string")
        task_id = raw_task.get("id") or slugify(raw_task.get("title") or "task")
        if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", task_id):
            raise ValueError(f"Invalid example task ID: {task_id!r}")
        if task_id in task_ids:
            raise ValueError(f"Duplicate example task ID: {task_id}")
        task_ids.add(task_id)
        title = raw_task.get("title") or task_id
        task_owner = owner(raw_task["owner"]) if raw_task.get("owner") else None
        tasks.append(
            {
                "id": task_id,
                "title": title,
                "status": "assigned" if task_owner else "pending",
                "owner": task_owner,
                "notes": str(raw_task.get("notes") or "Starter example task."),
                "created_at": now(),
                "updated_at": now(),
            }
        )
    return tasks


def team_toml(name: str, agents: list[str], opencode_command: str, multiplexer: str, display_names: dict[str, str] | None = None) -> str:
    agent_blocks = "\n".join(
        f"""[[agents]]
id = {toml_string(agent)}
display_name = {toml_string((display_names or {}).get(agent, agent))}
role = {toml_string(agent_role(agent))}
prompt = {toml_string(f"prompts/{agent}.md")}
"""
        for agent in agents
    )
    return f"""version = 1
name = {toml_string(name)}

[runtime]
multiplexer = {toml_string(multiplexer)}
opencode_command = {toml_string(opencode_command)}
source_of_truth = "workspace-files"
dashboard = "dashboard.md"

{agent_blocks}
[[checkpoints]]
id = "plan-approval"
description = "Consult the user after the coven proposes a plan and before implementation."
file = "checkpoints/plan-approval.md"
consult_user = true

[[checkpoints]]
id = "pre-delivery-review"
description = "Consult the user after review/validation and before declaring the task complete."
file = "checkpoints/pre-delivery-review.md"
consult_user = true
"""


def tmux_manifest(name: str, workspace: Path, agents: list[str], opencode_command: str) -> str:
    session = f"coven-{slugify(name)}"
    opencode_command = normalize_opencode_command(opencode_command)
    cli = cli_command()
    lines = [
        "version = 1",
        f"name = {toml_string(f'Coven: {name}')}",
        f"default_session = {toml_string(session)}",
        "",
        "[vars]",
        f"COVEN_DIR = {toml_string(str(workspace))}",
        f"OPENCODE_CMD = {toml_string(opencode_command)}",
        "",
        "[[sessions]]",
        f"name = {toml_string(session)}",
        'if_exists = "attach"',
        "",
        "[[sessions.windows]]",
        'name = "orchestrator"',
        "",
        "[sessions.windows.layout]",
        'strategy = "services-left-playground-right"',
        'playground_pane = "shell"',
        "playground_percent = 50",
        "",
    ]

    monitor_panes = [
        ("dashboard", "while true; do clear; cat dashboard.md; sleep 2; done", "monitor"),
        ("events", f"while true; do clear; {cli} events; sleep 2; done", "monitor"),
        ("messages", f"while true; do clear; {cli} messages; sleep 2; done", "monitor"),
        ("shell", "printf 'coven workspace: '; pwd; exec /bin/zsh", "playground"),
    ]
    for title, command, role in monitor_panes:
        lines.extend(
            [
                "[[sessions.windows.panes]]",
                f"title = {toml_string(title)}",
                'dir = "${COVEN_DIR}"',
                f"command = {toml_string(command)}",
                f"role = {toml_string(role)}",
                "",
            ]
        )

    for agent in agents:
        command = agent_launch_command(name, workspace, agent, opencode_command)
        lines.extend(
            [
                "[[sessions.windows]]",
                f"name = {toml_string(f'agent-{agent}')}",
                "",
                "[[sessions.windows.panes]]",
                f"title = {toml_string(agent)}",
                'dir = "${COVEN_DIR}"',
                f"command = {toml_string(command)}",
                "",
            ]
        )

    return "\n".join(lines)


def agent_launch_command(name: str, workspace: Path, agent: str, opencode_command: str, multiplexer: str = "tmux") -> str:
    record_id(agent, "agent")
    managed_path(workspace, f"prompts/{agent}.md")
    managed_path(workspace, f"prompts/{agent}.start.txt")
    opencode_command = normalize_opencode_command(opencode_command)
    prompt_path = f"prompts/{agent}.start.txt"
    if multiplexer == "herdr":
        return (
            f"printf 'Agent: {agent}\\nPrompt: {workspace}/prompts/{agent}.md\\n'"
            f"; printf 'Auto-bootstrapping OpenCode with {prompt_path}...\\n\\n'"
            "; pane=\"${HERDR_PANE_ID:-}\""
            "; herdr_bin=\"${HERDR_BIN_PATH:-herdr}\""
            f"; (sleep 5; if [ -n \"$pane\" ]; then prompt=\"$(cat {shlex.quote(prompt_path)})\"; "
            '"$herdr_bin" pane send-text "$pane" "$prompt"; '
            '"$herdr_bin" pane send-keys "$pane" enter; fi) & '
            f"{opencode_command}"
        )

    buffer_name = f"coven-{slugify(name)}-{agent}-starter"
    return (
        f"printf 'Agent: {agent}\\nPrompt: {workspace}/prompts/{agent}.md\\n'"
        f"; printf 'Auto-bootstrapping OpenCode with {prompt_path}...\\n\\n'"
        f"; pane=\"$(tmux display-message -p '#{{pane_id}}')\""
        f"; (sleep 5; tmux load-buffer -b {q(buffer_name)} {q(prompt_path)}; "
        f"tmux paste-buffer -b {q(buffer_name)} -t \"$pane\"; "
        f"tmux send-keys -t \"$pane\" Enter) & "
        f"{opencode_command}"
    )


def render_dashboard(workspace: Path) -> str:
    snapshot = load_workspace_state(workspace)
    team, tasks, agents = snapshot.team, snapshot.tasks, snapshot.agents
    goal = (workspace / "goal.md").read_text(encoding="utf-8").strip() if (workspace / "goal.md").exists() else ""
    if goal.startswith("# Goal"):
        goal = goal.removeprefix("# Goal").strip()
    multiplexer = runtime_multiplexer(team)
    lead = snapshot.lead
    lead_text = "None set."
    if lead:
        lead_text = (
            f"`{lead.get('agent')}` — set {lead.get('set_at', 'unknown time')}. "
            f"Stop condition: {lead.get('until', 'goal achieved or critical blocker')}."
        )

    task_rows = "\n".join(
        f"| `{task['id']}` | {task['status']} | {task.get('owner') or 'unassigned'} | {task['title']} |"
        for task in tasks
    ) or "| - | - | - | No tasks yet |"
    agent_rows = "\n".join(
        f"| {agent['id']} | {markdown_text(agent_display_name(team, agent['id']))} | {agent['status']} | {agent['role']} |" for agent in agents
    )

    checkpoint_rows = []
    checkpoint_specs = [
        ("plan-approval", "consult user before implementation"),
        ("pre-delivery-review", "consult user before declaring completion"),
    ]
    for checkpoint_id, purpose in checkpoint_specs:
        status = checkpoint_status(workspace, checkpoint_id)
        checkpoint_rows.append(f"| `{checkpoint_id}` | {status} | `checkpoints/{checkpoint_id}.md` | {purpose} |")

    return f"""# Coven Dashboard: {team['name']}

## Goal

{goal or '_No goal set yet._'}

## Coven Lead

{lead_text}

## Agents

| Agent ID | Display name | Status | Role |
| --- | --- | --- | --- |
{agent_rows}

## Tasks

| ID | Status | Owner | Title |
| --- | --- | --- | --- |
{task_rows}

## Checkpoints

| ID | Status | File | Purpose |
| --- | --- | --- | --- |
{chr(10).join(checkpoint_rows)}

## Work output

- In-progress build files: `work/`
- Final/demo artifacts: `output/`

## Monitoring

- Events: `logs/events.jsonl`
- Messages: `logs/messages.jsonl`
- Runtime multiplexer: `{multiplexer}`
- Tmux manifest (for optional tmux runtime): `tmux/coven.toml`
"""


def refresh_dashboard(workspace: Path, *, lock: storage.WorkspaceLock | None = None) -> None:
    # The render, not just publication, is serialized with task mutations.
    preflight_workspace_paths(workspace)
    managed_text(workspace, "goal.md")
    with workspace_mutation(workspace, lock):
        content = render_dashboard(workspace).encode("utf-8")
        storage.atomic_replace_bytes(managed_path(workspace, "dashboard.md"), content)


def read_lead(workspace: Path) -> dict[str, Any] | None:
    return read_state_records(workspace, "state/lead.json", None, records.lead)


def write_lead(workspace: Path, state: dict[str, Any] | None, *, lock: storage.WorkspaceLock | None = None) -> None:
    preflight_workspace_paths(workspace)
    try:
        records.lead(state)
    except ValueError as exc:
        fail(f"Invalid proposed lead record: {exc}")
    with workspace_mutation(workspace, lock):
        path = managed_path(workspace, "state/lead.json")
        if state is None:
            if path.exists():
                path.unlink()
                try:
                    storage.sync_directory(path.parent)
                except OSError as exc:
                    raise storage.CommitUncertain(path) from exc
            return
        storage.atomic_replace_bytes(path, storage.encode_json(state))


def checkpoint_status(workspace: Path, checkpoint: str) -> str:
    return checkpoint_view(workspace, checkpoint).effective_status


def checkpoint_bytes(workspace: Path, checkpoint: str) -> bytes | None:
    path = checkpoint_path(workspace, checkpoint)
    try:
        return path.read_bytes() if path.exists() else None
    except OSError as exc:
        raise storage.StorageError(f"Cannot read managed checkpoint {path}: {exc}") from exc


def checkpoint_view(workspace: Path, checkpoint: str) -> checkpoints.View:
    return checkpoints.inspect(checkpoint, checkpoint_bytes(workspace, checkpoint))


def checkpoint_log_records(workspace: Path, filename: str) -> list[dict[str, Any]]:
    records = iter_jsonl(managed_path(workspace, f"logs/{filename}.jsonl"))
    # Validate the entire batch before discovery can consume any entry.
    for record in records:
        if "checkpoint" in record:
            checkpoint_text(workspace, record["checkpoint"])
    return records


def workspace_structure_files(example: dict[str, Any] | None = None) -> dict[str, str]:
    return {
        "work/README.md": read_example_text(example, "work/README.md", work_readme()),
        "output/README.md": read_example_text(example, "output/README.md", output_readme()),
        "checkpoints/plan-approval.md": checkpoint_file(
            "plan-approval",
            "Plan Approval",
            "Approve the coven plan, acceptance criteria, and proposed implementation sequence before implementation begins.",
        ),
        "checkpoints/pre-delivery-review.md": checkpoint_file(
            "pre-delivery-review",
            "Pre-delivery Review",
            "Approve final validation and review notes before the coven declares the work complete.",
        ),
    }


def preflight_init(workspace: Path) -> None:
    if workspace.exists() and not workspace.is_dir():
        fail(f"Workspace path is not a directory: {workspace}")
    conflicts = [name for name in INIT_RESERVED_NAMES
                 if (workspace / name).exists() or (workspace / name).is_symlink()]
    if conflicts:
        fail(
            f"Refusing to initialize over existing workspace files in {workspace}: "
            f"{', '.join(conflicts)}. Use `coven refresh` for an existing workspace "
            "or choose a new directory. --force does not authorize a reset."
        )


def command_init(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    preflight_init(workspace)
    try:
        agents = parse_agents(args.agents)
    except ValueError as exc:
        fail(str(exc))
    multiplexer = str(args.multiplexer).strip().lower()
    if multiplexer not in SUPPORTED_MULTIPLEXERS:
        fail(f"Unsupported multiplexer `{multiplexer}`. Choose one of: {', '.join(sorted(SUPPORTED_MULTIPLEXERS))}")

    name = args.name or workspace.name or "coven"
    try:
        example = None if args.blank else load_example(args.example)
    except (ValueError, OSError) as exc:
        fail(f"Invalid example: {exc}")

    team = {
        "version": 1,
        "name": name,
        "created_at": now(),
        "source_of_truth": "workspace-files",
        "opencode_command": args.opencode_command,
        "multiplexer": multiplexer,
        "agents": agents,
        "display_names": dict(zip(agents, (label.strip() for label in args.agents.split(",")))),
        "example": None if args.blank else example["id"],
    }
    runtime_multiplexer(team)  # Validate environment overrides before writing too.
    # Read and validate all seed inputs and render templates before creating files.
    try:
        if not args.opencode_command.strip():
            raise ValueError("opencode command must not be empty")
        records.team(team)
        tasks = [] if args.blank else starter_tasks(agents, example)
        records.tasks(tasks)
        files = {
            "coven.json": json.dumps(team, indent=2) + "\n",
            "state/agents.json": json.dumps(
                [{"id": agent, "role": agent_role(agent), "status": "idle"} for agent in agents], indent=2
            ) + "\n",
            "state/tasks.json": json.dumps(tasks, indent=2) + "\n",
            "goal.md": example_goal_text(example),
            "coven.toml": team_toml(name, agents, args.opencode_command, multiplexer, team["display_names"]),
            "logs/events.jsonl": "",
            "logs/messages.jsonl": "",
            "tmux/coven.toml": tmux_manifest(name, workspace, agents, args.opencode_command),
            **workspace_structure_files(example),
        }
        if not args.blank:
            files["spec.md"] = read_example_text(example, "spec.md", "# Spec\n\nDescribe the desired behavior here.\n")
        example_context = read_example_text(example, "prompt-context.md")
        for agent in agents:
            files[f"agents/{agent}.md"] = agent_state_text(agent)
            files[f"prompts/{agent}.md"] = prompt_text(agent, workspace, example_context)
            files[f"prompts/{agent}.start.txt"] = starter_prompt_text(agent, workspace)
        for relative in ("coven.toml", "tmux/coven.toml"):
            tomllib.loads(files[relative])
    except (ValueError, OSError) as exc:
        fail(f"Invalid initialization inputs: {exc}")

    try:
        workspace.mkdir(parents=True, exist_ok=True)
        for relative, content in files.items():
            path = workspace / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("x", encoding="utf-8") as handle:
                handle.write(content)
    except OSError as exc:
        fail(f"Initialization stopped; partial files have been preserved for inspection: {exc}")

    append_event(workspace, "workspace.created", name=name, agents=agents, example="blank" if args.blank else example["id"])
    if not args.blank:
        append_event(workspace, "goal.seeded", example=example["id"])
        for task in tasks:
            append_event(workspace, "task.seeded", task=task)
    refresh_dashboard(workspace)
    info(f"Coven workspace: {workspace}")
    if args.blank:
        info(f"Next: coven goal {workspace} \"<your goal>\"")
    else:
        info(str(example.get("seed_message") or f"Seeded example: {example['id']}"))
    info(f"Launch ({multiplexer}): coven up {workspace}")
    return 0


def command_goal(args: argparse.Namespace) -> int:
    workspace, items = split_optional_workspace(list(args.items))
    load_team(workspace)
    text = " ".join(items).strip()
    if not text:
        print((workspace / "goal.md").read_text(encoding="utf-8"))
        return 0
    with workspace_mutation(workspace) as held:
        storage.atomic_replace_bytes(managed_path(workspace, "goal.md"), f"# Goal\n\n{text}\n".encode("utf-8"))
        for callback in (lambda: append_event(workspace, "goal.updated", goal=text, lock=held),
                         lambda: refresh_dashboard(workspace, lock=held)):
            try:
                callback()
            except (Exception, SystemExit) as exc:
                print(f"[WARN] Goal committed; secondary update failed: {exc}", file=sys.stderr)
    info("Goal updated")
    return 0


def next_task_id(tasks: list[dict[str, Any]]) -> str:
    existing = {str(task.get("id")) for task in tasks if isinstance(task, dict) and task.get("id")}
    return unique_slug("task", existing)


def read_task_snapshot(workspace: Path) -> tuple[str, list[dict[str, Any]]]:
    view = task_ledger.LedgerStore(workspace).read()
    if view is not None:
        return view.revision, view.tasks
    return read_legacy_task_snapshot(workspace)


def read_legacy_task_snapshot(workspace: Path) -> tuple[str, list[dict[str, Any]]]:
    path = managed_path(workspace, "state/tasks.json")
    try:
        data = path.read_bytes() if path.exists() else None
        tasks = records.tasks(json.loads(data.decode("utf-8")) if data is not None else [])
    except (ValueError, OSError) as exc:
        raise storage.StorageError(f"Invalid task snapshot {path}: {exc}") from exc
    return storage.snapshot_revision(data), tasks


def check_task_revision(expected: str | None, current: str) -> None:
    if expected is not None and expected != current:
        raise storage.RevisionConflict(f"Stale tasks revision: expected {expected}; current {current}; no task change applied")


TaskChange = Callable[[dict[str, Any], list[dict[str, Any]]], tuple[dict[str, Any], str, dict[str, Any]]]


def mutate_tasks(workspace: Path, change: TaskChange, *, expected_revision: str | None = None,
                 request_id: str | None = None, binding: dict[str, Any] | None = None) -> dict[str, Any]:
    """One authoritative snapshot commit; history/projection are secondary effects."""
    store = task_ledger.LedgerStore(workspace)
    if store.read() is not None:
        return mutate_ledger_tasks(workspace, change, request_id, binding)
    if request_id is not None:
        raise task_ledger.LedgerError("--request-id requires explicit state import-tasks; snapshot-v1 does not provide durable deduplication")
    if store.has_pending_import():
        raise task_ledger.LedgerError("Task import is pending; inspect and explicitly resume or abort it before task mutations")
    storage.validate_revision(expected_revision)
    # Validate deterministic failures without creating a lock, then discard this
    # draft. Only the freshly reloaded state inside the lock may be committed.
    team = load_team(workspace)
    before, draft = read_task_snapshot(workspace)
    check_task_revision(expected_revision, before)
    change(team, draft)
    records.tasks(draft)
    storage.encode_json(draft)
    for filename in ("events", "messages"):
        iter_jsonl(managed_path(workspace, f"logs/{filename}.jsonl"))

    with workspace_mutation(workspace) as held:
        # Import and task writes use one lock. Never commit to a backend chosen
        # only during the unlocked preflight.
        if store.read() is not None:
            return mutate_ledger_tasks(workspace, change, request_id, binding, lock=held)
        if store.has_pending_import():
            raise task_ledger.LedgerError("Task import is pending; inspect and explicitly resume or abort it")
        team = load_team(workspace)
        before, tasks = read_task_snapshot(workspace)
        check_task_revision(expected_revision, before)
        for filename in ("events", "messages"):
            iter_jsonl(managed_path(workspace, f"logs/{filename}.jsonl"))
        task, event_type, payload = change(team, tasks)
        records.tasks(tasks)
        task_attempts.guard_task_mutation(workspace, task["id"], held)
        encoded = storage.encode_json(tasks)
        after = storage.snapshot_revision(encoded)
        mutation_id = str(uuid.uuid4())
        event_payload = {**payload, "mutation_id": mutation_id, "before_revision": before, "after_revision": after}
        storage.encode_json(event_payload)
        result = {"outcome": "committed", "mutation_id": mutation_id, "task": task,
                  "before_revision": before, "after_revision": after, "warnings": []}
        try:
            storage.atomic_replace_bytes(managed_path(workspace, "state/tasks.json"), encoded)
        except storage.CommitUncertain as exc:
            result["outcome"] = "durability_uncertain"
            result["warnings"].append(str(exc))
            return result
        # Never roll back or report an ordinary failed mutation after the durable
        # authoritative replace. This is NOT a snapshot+event+dashboard transaction.
        for label, action in (
            ("history incomplete", lambda: append_event(workspace, event_type, lock=held, **event_payload)),
            ("dashboard stale", lambda: refresh_dashboard(workspace, lock=held)),
        ):
            try:
                action()
            except (Exception, SystemExit) as exc:
                result["warnings"].append(f"Task committed; {label}: {exc}. Inspect before retrying this request.")
        return result


def ledger_delivery(store: task_ledger.LedgerStore, view: task_ledger.LedgerView, result: dict[str, Any], *,
                    replayed: bool, warnings: list[str] | None = None, outcome: str = "committed") -> dict[str, Any]:
    health = store.projection_health(view)
    messages = list(warnings or [])
    if health["status"] != "current":
        messages.append("Task projection is not current; new mutations require state recover-tasks --repair-projection")
    return {"result": result, "delivery": {"outcome": outcome, "replayed": replayed,
            "warnings": messages, "current_revision": view.revision, "projection": health}}


def mutate_ledger_tasks(workspace: Path, change: TaskChange, request_id: str | None,
                        binding: dict[str, Any] | None, *, lock: storage.WorkspaceLock | None = None) -> dict[str, Any]:
    if request_id is None or binding is None:
        raise task_ledger.LedgerError("Imported task authority requires --request-id for every add/assign")
    task_ledger.validate_key(request_id)
    with workspace_mutation(workspace, lock) as held:
        team = load_team(workspace)
        store = task_ledger.LedgerStore(workspace)
        view = store.read()
        if view is None:
            raise task_ledger.LedgerError("Task authority changed; inspect before retrying")
        # Receipt lookup precedes expected-revision/current roster interpretation.
        receipt = store.lookup(view, request_id, binding)
        try:
            store.confirm_durability(view, held)
        except task_ledger.PublicationUncertain as exc:
            if receipt is None:
                raise
            return ledger_delivery(store, view, receipt, replayed=True, warnings=[str(exc)], outcome="durability_uncertain")
        if receipt is not None:
            return ledger_delivery(store, view, receipt, replayed=True)
        if store.projection_health(view)["status"] != "current":
            raise task_ledger.LedgerError("Task projection diverged/missing; explicitly state recover-tasks --repair-projection before new mutations")
        check_task_revision(binding["expected_revision"], view.revision)
        # Replay is a fresh object; modifying this draft does not change history.
        task, event_type, payload = change(team, json.loads(json.dumps(view.tasks)))
        try:
            committed = store.commit(view, request_id, binding, task, held)
        except task_ledger.PublicationUncertain as exc:
            observed = store.read()
            receipt = store.lookup(observed, request_id, binding) if observed else None
            if receipt is None:
                raise
            return ledger_delivery(store, observed, receipt, replayed=False, warnings=[str(exc)], outcome="durability_uncertain")
        result = store.lookup(committed, request_id, binding)
        warnings = []
        for label, action in (
            ("task projection stale", lambda: store.publish_projection(committed, held)),
            ("history incomplete", lambda: append_event(workspace, event_type, lock=held, **payload,
                mutation_id=result["mutation_id"], epoch_id=committed.epoch_id, sequence=committed.sequence,
                frame_digest=committed.head_digest)),
            ("dashboard stale", lambda: refresh_dashboard(workspace, lock=held)),
        ):
            try:
                action()
            except (Exception, SystemExit) as exc:
                warnings.append(f"Ledger task committed; {label}: {exc}")
        return ledger_delivery(store, committed, result, replayed=False, warnings=warnings)


def add_task(workspace: Path, title: str, owner: str | None = None, notes: str = "", *, expected_revision: str | None = None,
             request_id: str | None = None) -> dict[str, Any]:
    def change(team: dict[str, Any], tasks: list[dict[str, Any]]):
        task_owner = resolve_agent(team, owner) if owner else None
        try:
            records.text(title, "task title")
            task_id = unique_slug(title, {task["id"] for task in tasks})
            records.text(notes, "task notes")
        except ValueError as exc:
            fail(f"Invalid task title/notes: {exc}")
        task = {"id": task_id, "title": title, "status": "assigned" if task_owner else "pending", "owner": task_owner,
                "notes": notes, "created_at": now(), "updated_at": now()}
        tasks.append(task)
        return task, "task.created", {"task": task}
    binding = task_ledger.request_binding("add", {"title": title, "owner": owner or None, "notes": notes}, expected_revision) if request_id is not None else None
    return mutate_tasks(workspace, change, expected_revision=expected_revision, request_id=request_id, binding=binding)


def assign_task(workspace: Path, task_id: str, agent: str, *, expected_revision: str | None = None,
                request_id: str | None = None) -> dict[str, Any]:
    def change(team: dict[str, Any], tasks: list[dict[str, Any]]):
        owner = resolve_agent(team, agent)
        for task in tasks:
            if task["id"] == task_id:
                task.update(owner=owner, status="assigned", updated_at=now())
                return task, "task.assigned", {"task_id": task_id, "agent": owner}
        fail(f"Task not found: {task_id}")
    binding = task_ledger.request_binding("assign", {"task_id": task_id, "agent": agent}, expected_revision) if request_id is not None else None
    return mutate_tasks(workspace, change, expected_revision=expected_revision, request_id=request_id, binding=binding)


def print_task_result(result: dict[str, Any], *, json_output: bool, action: str) -> int:
    if "delivery" in result:
        if json_output:
            print(json.dumps(result, ensure_ascii=False))
        else:
            frozen, delivery = result["result"], result["delivery"]
            verb = ("Observed; durability uncertain for" if delivery["outcome"] == "durability_uncertain"
                    else "Replayed committed" if delivery["replayed"] else "Committed")
            print(f"{verb} ledger task {frozen['task']['id']} (sequence {frozen['sequence']})")
            for warning in delivery["warnings"]:
                print(f"[WARN] {warning}", file=sys.stderr)
        return 0 if result["delivery"]["outcome"] == "committed" else 2
    if json_output:
        print(json.dumps(result, ensure_ascii=False))
    else:
        task = result["task"]
        if result["outcome"] == "committed":
            print(f"Created {task['id']}: {task['title']}" if action == "add" else f"Assigned {task['id']} to {task['owner']}")
        else:
            print(f"Task snapshot replaced for {task['id']}; durability uncertain. Inspect before retrying.")
        for warning in result["warnings"]:
            print(f"[WARN] {warning}", file=sys.stderr)
    return 0 if result["outcome"] == "committed" else 2


def command_task_add(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if not rest:
        fail("task add requires a title")
    result = add_task(workspace, " ".join(rest), args.owner, args.notes or "", expected_revision=getattr(args, "expect_revision", None), request_id=getattr(args, "request_id", None))
    return print_task_result(result, json_output=getattr(args, "json", False), action="add")


def command_task_assign(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 2:
        fail("task assign requires task_id and agent")
    result = assign_task(workspace, rest[0], rest[1], expected_revision=getattr(args, "expect_revision", None), request_id=getattr(args, "request_id", None))
    return print_task_result(result, json_output=getattr(args, "json", False), action="assign")


def command_task_list(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    revision, tasks = read_task_snapshot(workspace)
    if getattr(args, "json", False):
        view = task_ledger.LedgerStore(workspace).read()
        if view is not None:
            print(json.dumps(task_inspection(workspace, view), ensure_ascii=False))
        else:
            print(json.dumps({"revision": revision, "tasks": tasks}, ensure_ascii=False))
        return 0
    if not tasks:
        print("No tasks yet")
        return 0
    for task in tasks:
        print(f"{task['id']} [{task['status']}] {task.get('owner') or 'unassigned'} - {task['title']}")
    return 0


def task_inspection(workspace: Path, view: task_ledger.LedgerView | None = None) -> dict[str, Any]:
    store = task_ledger.LedgerStore(workspace)
    view = view or store.read()
    if view is None:
        pending = store.has_pending_import()
        try:
            revision, tasks = read_legacy_task_snapshot(workspace)
        except storage.StorageError as exc:
            if not pending:
                raise
            return {"authority": "snapshot-v1", "import_pending": True, "source_error": str(exc)}
        return {"authority": "snapshot-v1", "revision": revision, "tasks": tasks,
                "import_pending": pending}
    return {"authority": "task-ledger-v1", "revision": view.revision, "tasks": view.tasks,
            "workspace_id": view.workspace_id, "epoch_id": view.epoch_id,
            "sequence": view.sequence, "frame_digest": view.head_digest,
            "projection": store.projection_health(view)}


def print_state_outcome(value: dict[str, Any], json_output: bool) -> int:
    print(json.dumps(value, ensure_ascii=False) if json_output else json.dumps(value, indent=2, ensure_ascii=False))
    return 2 if value.get("outcome") == "durability_uncertain" else 0


def import_secondary_outputs(workspace: Path, store: task_ledger.LedgerStore, view: task_ledger.LedgerView,
                             lock: storage.WorkspaceLock) -> list[str]:
    warnings = []
    for label, action in (
        ("task projection stale", lambda: store.publish_projection(view, lock) if store.projection_health(view)["status"] != "current" else None),
        ("dashboard stale", lambda: refresh_dashboard(workspace, lock=lock)),
    ):
        try:
            action()
        except (Exception, SystemExit) as exc:
            warnings.append(f"Ledger authority active; {label}: {exc}")
    return warnings


def command_state_inspect(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    # Diagnostic reads must not depend on a valid non-authoritative projection
    # or a valid source snapshot while an interrupted import awaits inspection.
    check_workspace_paths(workspace)
    read_team_config(workspace)
    return print_state_outcome(task_inspection(workspace), args.json)


@contextmanager
def pending_import_recovery(workspace: Path):
    # Archival changes no task contents. Do not require repairing a corrupt v1
    # source just to preserve/rename an unactivated import's artifacts.
    check_workspace_paths(workspace)
    read_team_config(workspace)
    with storage.workspace_lock(workspace, managed_path(workspace, "state/.workspace.lock")) as held:
        check_workspace_paths(workspace)
        read_team_config(workspace)
        yield held


def command_state_import_tasks(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    storage.validate_revision(args.expect_revision)
    store = task_ledger.LedgerStore(workspace)
    if store.read() is not None or store.has_pending_import():
        raise task_ledger.LedgerError("Task authority is already active or import pending; inspect/recover explicitly")
    revision, _ = read_legacy_task_snapshot(workspace)
    check_task_revision(args.expect_revision, revision)
    with workspace_mutation(workspace) as held:
        try:
            view = store.import_tasks(args.expect_revision, held)
        except task_ledger.PublicationUncertain as exc:
            view = store.read()
            if view is None:
                raise
            return print_state_outcome({**task_inspection(workspace, view), "outcome": "durability_uncertain",
                                        "warnings": [str(exc)]}, args.json)
        warnings = import_secondary_outputs(workspace, store, view, held)
        return print_state_outcome({**task_inspection(workspace, view), "outcome": "committed", "warnings": warnings}, args.json)


def command_state_recover_tasks(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    if args.abort_import:
        if args.expect_revision is not None:
            raise task_ledger.LedgerError("--expect-revision is used only with --resume-import")
        with pending_import_recovery(workspace) as held:
            archived = task_ledger.LedgerStore(workspace).abort_import(held)
            return print_state_outcome({"outcome": "aborted", "authority": "snapshot-v1", "preserved_artifacts": archived}, args.json)
    load_team(workspace)
    store = task_ledger.LedgerStore(workspace)
    if args.resume_import:
        if args.expect_revision is None:
            raise task_ledger.LedgerError("--resume-import requires --expect-revision of the original snapshot")
        storage.validate_revision(args.expect_revision)
    elif args.expect_revision is not None:
        raise task_ledger.LedgerError("--expect-revision is used only with --resume-import")
    with workspace_mutation(workspace) as held:
        try:
            if args.resume_import:
                view = store.resume_import(args.expect_revision, held)
                detail = {"import_resumed": True}
            else:
                view = store.read()
                if view is None:
                    raise task_ledger.LedgerError("No active ledger; inspect a pending import and use --resume-import or --abort-import")
                store.confirm_durability(view, held)
                detail = store.repair_projection(held)
        except (task_ledger.PublicationUncertain, storage.CommitUncertain) as exc:
            view = store.read()
            if view is None:
                raise
            return print_state_outcome({**task_inspection(workspace, view), "outcome": "durability_uncertain", "warnings": [str(exc)]}, args.json)
        warnings = import_secondary_outputs(workspace, store, view, held)
        return print_state_outcome({**task_inspection(workspace, view), "outcome": "recovered", "recovery": detail, "warnings": warnings}, args.json)


def command_message(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 3:
        fail("message requires sender, recipient, and body")
    sender, recipient = rest[0], rest[1]
    body = " ".join(rest[2:])
    load_team(workspace)
    append_message(workspace, sender, recipient, body)
    refresh_dashboard(workspace)
    info("Message appended")
    return 0


def reject_legacy_runtime() -> int:
    raise actions.ActionError("Approved runtime-launch mode is enabled; use action prepare-runtime-launch/inspect/submit. Legacy launch, restart, pane delivery, and agent stop are disabled; use action stop for scoped operator control")


def require_legacy_runtime(workspace: Path) -> None:
    if actions.ActionStore(workspace).read_control() is not None:
        reject_legacy_runtime()


_transport_gates = threading.local()


@contextmanager
def legacy_runtime_transport(workspace: Path):
    preflight_workspace_paths(workspace)
    root = workspace.resolve()
    entries = getattr(_transport_gates, "entries", {})
    old = entries.get(str(root))
    path = managed_path(root, "state/.runtime-transport.lock")
    with storage.transport_gate(root, path, held=old) as gate:
        require_legacy_runtime(root)  # Recheck only AFTER admission is fenced.
        _transport_gates.entries = {**entries, str(root): gate}
        try:
            yield
        finally:
            _transport_gates.entries = entries


def legacy_transport_effect(function):
    @wraps(function)
    def guarded(workspace, *args, **kwargs):
        if function.__name__ in {"start_agent_window", "stop_agent_window"} and len(args) >= 2:
            require_agent(args[0], args[1])
        with legacy_runtime_transport(workspace):
            return function(workspace, *args, **kwargs)
    return guarded


@contextmanager
def action_control_mutation(workspace: Path):
    # Consumed receipts/outcome recording must survive changes to non-authority
    # input files. Fresh claims validate those inputs separately inside this lock.
    store = actions.ActionStore(workspace)
    if store.read_control() is None:
        raise actions.ActionError("Enable runtime-launch approval mode explicitly first")
    with storage.workspace_lock(workspace, store.path("state/.workspace.lock")) as held:
        store.read_control()
        yield store, held


def runtime_launch_inputs(workspace: Path) -> dict[str, Any]:
    team = load_team(workspace)
    tasks = task_inspection(workspace)
    if tasks.get("import_pending") or tasks.get("projection", {"status": "current"})["status"] != "current":
        raise actions.ActionError("Resolve pending task import/projection recovery before reviewing runtime startup")
    authority = {key: tasks[key] for key in ("authority", "revision", "tasks", "workspace_id", "epoch_id", "sequence") if key in tasks}
    roster = team_agents(team)
    paths = ["coven.json", "coven.toml", "goal.md", "spec.md", "state/tasks.json", "state/agents.json", "state/lead.json"]
    paths += [relative for agent in roster for relative in (f"agents/{agent}.md", f"prompts/{agent}.md", f"prompts/{agent}.start.txt")]
    files = []
    for relative in paths:
        path = managed_path(workspace, relative)
        try:
            data = path.read_bytes() if path.exists() else None
        except OSError as exc:
            raise actions.ActionError(f"Cannot capture runtime input {relative}: {exc}") from exc
        files.append({"path": relative, "bytes": None if data is None else base64.b64encode(data).decode("ascii"),
                      "sha256": None if data is None else hashlib.sha256(data).hexdigest()})
    return {"version": 1, "location": actions.ActionStore(workspace).location(), "files": files,
            "task_authority": authority, "roster": roster,
            "effective_runtime": {"multiplexer": runtime_multiplexer(team),
                "command": normalize_opencode_command(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)},
            "environment": {"selection": {"COVEN_MULTIPLEXER": os.environ.get("COVEN_MULTIPLEXER")},
                "transport": {key: os.environ[key] for key in sorted(launch.ENV_KEYS) if key in os.environ}}}


def frozen_launch_plan(control: dict[str, Any], action_id: str, inputs: dict[str, Any]) -> dict[str, Any]:
    files = {item["path"]: item["bytes"] for item in inputs["files"]}
    agents = []
    try:
        for agent in inputs["roster"]:
            for relative in (f"prompts/{agent}.md", f"prompts/{agent}.start.txt"):
                value = files[relative]
                if value is None or not base64.b64decode(value).decode("utf-8").strip():
                    raise ValueError(f"Missing/empty prompt {relative}; refresh before preparing")
            agents.append({"id": agent, "command": inputs["effective_runtime"]["command"],
                           "starter": base64.b64decode(files[f"prompts/{agent}.start.txt"]).decode("utf-8")})
        return launch.build_plan(inputs["location"]["root"], f"coven-{control['workspace_id']}-{action_id}",
                                 inputs["effective_runtime"]["multiplexer"], agents, inputs["environment"]["transport"])
    except (ValueError, KeyError, TypeError) as exc:
        raise actions.ActionError(f"Cannot prepare frozen runtime launch: {exc}") from exc


def prepare_runtime_launch(workspace: Path) -> dict[str, Any]:
    store = actions.ActionStore(workspace)
    control = store.read_control()
    if control is None:
        raise actions.ActionError("Enable runtime-launch approval mode explicitly first")
    action_id = uuid.uuid4().hex
    frozen_launch_plan(control, action_id, runtime_launch_inputs(workspace))  # discarded preflight
    with workspace_mutation(workspace) as held:
        control = store.read_control()
        inputs = runtime_launch_inputs(workspace)
        plan = frozen_launch_plan(control, action_id, inputs)
        bundle = store.make_bundle(inputs, plan, action_id, held)
        manifest = store.publish_bundle(bundle, held)
        cp_id = actions.checkpoint_id(bundle)
        path = checkpoint_path(workspace, cp_id)
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            # A crash may leave an unapproved preparation; never overwrite an
            # existing checkpoint or treat bundle publication as permission.
            with path.open("xb") as stream:
                stream.write(actions.checkpoint_bytes(bundle))
                stream.flush()
                os.fsync(stream.fileno())
            storage.sync_directory(path.parent)
            storage.sync_directory(path.parent.parent)
        except OSError as exc:
            raise actions.ActionError(f"Action bundle preserved but checkpoint preparation incomplete; inspect {action_id}: {exc}") from exc
        return {"outcome": "prepared", "action_id": action_id, "manifest_digest": manifest,
                "checkpoint": cp_id, "checkpoint_revision": checkpoint_view(workspace, cp_id).revision,
                "warning": "Unrestricted cold runtime startup; not read-only work, dispatch, or worker acceptance"}


def submit_runtime_launch(workspace: Path, action_id: str, expected_manifest: str,
                          expected_checkpoint: str, expected_decision: str) -> dict[str, Any]:
    actions.validate_action_id(action_id)
    with action_control_mutation(workspace) as (store, held):
        try:
            if store.get_claim(action_id) is not None:
                claim, replayed = store.claim(action_id, expected_manifest, expected_checkpoint, expected_decision, {}, held)
            else:
                inputs = runtime_launch_inputs(workspace)
                bundle = store.read_bundle(action_id)
                plan = frozen_launch_plan(store.read_control(), action_id, inputs)
                if actions.canonical(plan) != actions.canonical(bundle["plan"]):
                    raise actions.ActionError("Frozen plan differs from current captured runtime inputs")
                claim, replayed = store.claim(action_id, expected_manifest, expected_checkpoint, expected_decision, inputs, held)
        except storage.CommitUncertain as exc:
            return {"outcome": "durability_uncertain", "replayed": True, "claim": store.get_claim(action_id),
                    "warnings": [str(exc), "No transport attempted; consumed action must never be automatically relaunched"]}
    if replayed:
        return {"outcome": claim["status"], "replayed": True, "claim": claim, "warnings": []}
    # This is the only executor boundary. It receives the durable claim's plan,
    # not live team/config/prompt paths. No workspace lock spans runtime I/O.
    try:
        report = launch.execute(claim["plan"])
    except Exception as exc:
        report = {"operation": "runtime_launch", "status": "transport_failed_or_partial", "completed_steps": 0,
                  "handles": {}, "diagnostics": [{"step": None, "error": type(exc).__name__, "message": str(exc)[:2048]}],
                  "outcome_unknown": True}
    try:
        with action_control_mutation(workspace) as (store, held):
            claim = store.record_outcome(action_id, claim["claim_id"], report, held)
    except (Exception, SystemExit) as exc:
        return {"outcome": "outcome_unknown", "replayed": False, "claim": claim, "transport_report": report,
                "warnings": [f"Transport may have run; durable outcome recording failed: {exc}. Inspect, never relaunch this action."]}
    warnings = []
    try:
        append_event(workspace, "runtime_launch.transport.recorded", action_id=action_id, claim_id=claim["claim_id"], status=claim["status"])
    except (Exception, SystemExit) as exc:
        warnings.append(f"Outcome committed; history incomplete: {exc}")
    return {"outcome": claim["status"], "replayed": False, "claim": claim, "warnings": warnings}


def stop_runtime_action(workspace: Path, action_id: str) -> dict[str, Any]:
    store = actions.ActionStore(workspace)
    claim = store.get_claim(action_id)
    if claim is None:
        raise actions.ActionError("No consumed launch claim for this action")
    report = launch.stop(claim["plan"])
    return {"outcome": report["status"], "action_id": action_id, "transport_report": report,
            "warning": "Operator stop is not evidence of native worker termination and does not reset launch consumption"}


def action_exit_code(value: dict[str, Any]) -> int:
    if value["outcome"] in {"claimed", "outcome_unknown", "durability_uncertain"}:
        return 2
    return 1 if value["outcome"] == "transport_failed_or_partial" else 0


def print_action_result(value: dict[str, Any], json_output: bool = True) -> int:
    print(json.dumps(value, ensure_ascii=False, indent=None if json_output else 2))
    return action_exit_code(value)


def stop_current_action(workspace: Path) -> int:
    control = actions.ActionStore(workspace).read_control()
    if control is None or control["head"] is None:
        return print_action_result({"outcome": "transport_completed", "note": "No scoped launch attempt recorded; no transport sent"})
    return print_action_result(stop_runtime_action(workspace, control["head"]))


def command_action_enable(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    # Never wait for the transport gate while holding the workspace lock: an
    # admitted legacy operation may need that lock to finish its own history.
    with storage.transport_gate(workspace, managed_path(workspace, "state/.runtime-transport.lock"), exclusive=True) as gate:
        with workspace_mutation(workspace) as held:
            store = actions.ActionStore(workspace)
            try:
                control = store.enable(held, gate=gate)
            except storage.CommitUncertain as exc:
                return print_action_result({"outcome": "durability_uncertain", "control": store.read_control(), "warnings": [str(exc)]}, args.json)
    return print_action_result({"outcome": "enabled", "control": control,
        "warning": "Legacy launches and pane delivery are disabled; this mode does not sandbox runtime tools"}, args.json)


def command_action_prepare(args: argparse.Namespace) -> int:
    return print_action_result(prepare_runtime_launch(workspace_path(args.workspace)), args.json)


def command_action_inspect(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    value = actions.ActionStore(workspace).inspect(args.action_id)
    try:
        value["checkpoint"] = checkpoint_view(workspace, value["checkpoint_id"]).to_dict()
    except (Exception, SystemExit) as exc:
        value["checkpoint_error"] = str(exc)
    value["outcome"] = "inspected"
    return print_action_result(value, args.json)


def command_action_submit(args: argparse.Namespace) -> int:
    value = submit_runtime_launch(workspace_path(args.workspace), args.action_id, args.expect_manifest,
                                  args.expect_checkpoint, args.expect_decision)
    return print_action_result(value, args.json)


def command_action_acknowledge(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    with action_control_mutation(workspace) as (store, held):
        claim = store.acknowledge_unknown(args.action_id, args.expect_claim, held)
    return print_action_result({"outcome": "acknowledged", "claim": claim,
        "warning": "No process was proven stopped; only a separately prepared and approved new action may launch again"}, args.json)


def command_action_stop(args: argparse.Namespace) -> int:
    return print_action_result(stop_runtime_action(workspace_path(args.workspace), args.action_id), args.json)


def command_worker_readiness(args: argparse.Namespace) -> int:
    # This is independent adapter observation, not workspace mutation or native
    # task execution. Do not load native config, inspect sessions, or acquire
    # workspace/transport authority merely to report missing compatibility.
    try:
        report = workers.inspect_readiness(workspace_path(args.workspace), adapter=args.adapter,
            profile=args.profile, executable=args.executable, timeout=args.timeout,
            max_output_bytes=args.max_output_bytes)
    except ValueError as exc:
        raise storage.StorageError(f"Invalid worker readiness request: {exc}") from exc
    if args.json:
        print(json.dumps(report, ensure_ascii=False))
    else:
        print(f"{report['adapter']} adapter compatibility: {report['assessment'].upper()}")
        print(f"Profile: {report['profile']}; adapter contract: {report['adapter_contract']}")
        executable = report["executable"]["resolved_path"] or report["executable"]["selected_path"] or "not found"
        print(f"Executable: {visible_text(executable)}")
        if report["executable"]["identity"] is not None:
            print(f"Executable SHA256: {report['executable']['identity']['sha256']}")
        print(f"Native version: {report['native_version'] or 'not observed'}")
        print("Authorization: none. No worker created; help surfaces do not attest effective policy or execution readiness.")
        for surface, value in report["cli_surfaces"].items():
            print(f"- CLI {surface}: {value['state']}")
        print("Blockers:")
        for blocker in report["blockers"]:
            print(f"- {blocker['code']}: {blocker['next_step']}")
    return workers.report_exit_code(report)


def print_repository_result(value: dict[str, Any], json_output: bool) -> int:
    # Escape arbitrary filesystem labels even in terminal output. Registration
    # is a source fact, never a mutation/delivery or native execution grant.
    print(json.dumps(value, ensure_ascii=True, indent=None if json_output else 2))
    if value.get("delivery", {}).get("outcome") == "durability_uncertain":
        return 2
    return 1 if value["status"] == "unavailable" else 0


def command_repository_inspect(args: argparse.Namespace) -> int:
    return print_repository_result(repositories.inspect_repository(
        workspace_path(args.workspace), verify=args.verify), args.json)


def command_repository_register(args: argparse.Namespace) -> int:
    return print_repository_result(repositories.register_repository(
        workspace_path(args.workspace), path=args.path, base=args.base,
        delivery_target=args.delivery_target, expected_revision=args.expect_repository_revision), args.json)


def command_observe_kitsune(args: argparse.Namespace) -> int:
    # No workspace authority, executable, or implicit endpoint selection. This
    # one-shot observation has no annotation/UI return path or native actions.
    try:
        if args.process_pane is not None:
            value = kitsune_process_observation.inspect(
                args.socket, args.process_pane, timeout=args.timeout,
                max_response_bytes=args.max_response_bytes)
        else:
            value = kitsune_observation.inspect(args.socket, timeout=args.timeout,
                                                max_response_bytes=args.max_response_bytes)
    except ValueError as exc:
        raise storage.StorageError(f"Invalid Kitsune observation request: {exc}") from exc
    print(json.dumps(value, ensure_ascii=True, indent=None if args.json else 2))
    return {"observed": 0, "unavailable": 1, "incompatible": 2}[value["assessment"]]


def command_observe_djinn(args: argparse.Namespace) -> int:
    try:
        value = djinn_observation.inspect(args.executable, args.folder, timeout=args.timeout,
                                          max_output_bytes=args.max_output_bytes)
    except ValueError as exc:
        raise storage.StorageError(f"Invalid Djinn observation request: {exc}") from exc
    print(json.dumps(value, ensure_ascii=True, indent=None if args.json else 2))
    return {"observed": 0, "unavailable": 1, "incompatible": 2}[value["assessment"]]


def command_observe_djinn_live(args: argparse.Namespace) -> int:
    try:
        value = djinn_live_observation.inspect(args.file, args.runtime, args.instance,
                                               args.session, timeout=args.timeout)
    except ValueError as exc:
        raise storage.StorageError(f"Invalid live Djinn observation request: {exc}") from exc
    print(json.dumps(value, ensure_ascii=True, indent=None if args.json else 2))
    return {"observed": 0, "unavailable": 1, "incompatible": 2}[value["assessment"]]


def print_group_result(value: dict[str, Any], json_output: bool) -> int:
    print(json.dumps(value, ensure_ascii=True, indent=None if json_output else 2))
    return 2 if value.get("delivery", {}).get("outcome") == "durability_uncertain" else 0


def command_group_create(args: argparse.Namespace) -> int:
    return print_group_result(groups.create_group(workspace_path(args.workspace), args.group,
        args.group if args.name is None else args.name, expected_revision=args.expect_revision), args.json)


def command_group_list(args: argparse.Namespace) -> int:
    view = groups.read_groups(workspace_path(args.workspace))
    # Publication watermarks are implementation state, not participant identities
    # or execution facts. Keep the user-facing list focused on group membership.
    value = {"initialized": view["initialized"], "revision": view["revision"],
             "groups": [] if view["state"] is None else view["state"]["groups"],
             "authorization": "none", "dispatchable": False,
             "native_lifecycle": "unestablished", "job_completion": "unestablished"}
    return print_group_result(value, args.json)


def command_group_add(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    # Deterministic invalid input and absent groups must not contact a server or
    # create a lock. Fresh evidence is read outside the mutation lock.
    groups._selector(args.group)
    groups._selector(args.member)
    groups._display(args.role)
    groups._string(args.pane)
    if not args.pane.isprintable() or any(char.isspace() for char in args.pane):
        raise groups.GroupError("An exact current pane ID is required")
    view = groups.read_groups(workspace)
    groups._cas(view["state"], args.expect_revision)
    groups._group(view["state"], args.group)
    try:
        import time
        if args.process_evidence:
            binding = group_publisher.binding_from_process(
                args.socket, args.pane, deadline=time.monotonic() + 3.0)
        else:
            fresh = group_publisher.read_publication_snapshot(args.socket, deadline=time.monotonic() + 3.0)
            binding = group_publisher.binding_from_snapshot(fresh, args.pane)
    except (ValueError, group_publisher.PublisherError) as exc:
        code = exc.code if isinstance(exc, group_publisher.PublisherError) else str(exc)
        raise groups.GroupError(f"Cannot observe selected group participant: {code}") from exc
    return print_group_result(groups.add_member(workspace, args.group, args.member, args.role, binding,
        rebind=args.rebind, expected_revision=args.expect_revision), args.json)


def command_group_remove(args: argparse.Namespace) -> int:
    return print_group_result(groups.remove_member(workspace_path(args.workspace), args.group,
        args.member, expected_revision=args.expect_revision), args.json)


def command_group_attach_live(args: argparse.Namespace) -> int:
    try:
        result = group_live.attach(workspace_path(args.workspace), args.group, args.member,
            args.file, args.runtime, args.instance, args.session, reattach=args.reattach,
            expected_revision=args.expect_revision, timeout=args.timeout)
    except ValueError as exc:
        raise groups.GroupError(f"Invalid group live attachment request: {exc}") from exc
    return print_group_result(result, args.json)


def command_group_detach_live(args: argparse.Namespace) -> int:
    return print_group_result(group_live.detach(workspace_path(args.workspace), args.group,
        args.member, expected_revision=args.expect_revision), args.json)


def command_group_status(args: argparse.Namespace) -> int:
    try:
        result = group_live.status(workspace_path(args.workspace), args.group,
                                   member_key=args.member, timeout=args.timeout)
    except ValueError as exc:
        raise groups.GroupError(f"Invalid group status request: {exc}") from exc
    print(json.dumps(result, ensure_ascii=True, indent=None if args.json else 2))
    return 0 if result["assessment"] == "observed" else 1


def command_group_publish(args: argparse.Namespace) -> int:
    try:
        value = group_publisher.publish_group(workspace_path(args.workspace), args.group,
            args.socket, args.source, duration=args.duration, **({"live": True} if args.live else {}))
    except ValueError as exc:
        raise groups.GroupError(f"Invalid group publication request: {exc}") from exc
    print(json.dumps(value, ensure_ascii=True, indent=None if args.json else 2))
    return {"completed": 0, "failed": 1, "uncertain": 2, "interrupted": 130}[value["outcome"]]


def execution_source(workspace: Path, snapshot_source_id: str) -> dict[str, Any]:
    """One authoritative task view/revision, read by a caller holding its lock."""
    team = load_team(workspace)
    ledger = task_ledger.LedgerStore(workspace)
    view = ledger.read()
    if view is None:
        if ledger.has_pending_import():
            raise task_attempts.AttemptError("Resolve the pending task import before execution enrollment/preparation")
        revision, tasks = read_legacy_task_snapshot(workspace)
        identity = {"kind": "snapshot-v1", "source_id": snapshot_source_id, "path": "state/tasks.json"}
    else:
        revision, tasks = view.revision, view.tasks
        identity = {"kind": "task-ledger-v1", "workspace_id": view.workspace_id,
                    "epoch_id": view.epoch_id, "path": "state/task-ledger"}
    return {"identity": identity, "revision": revision, "tasks": tasks, "roster": team_agents(team)}


@contextmanager
def execution_control_mutation(workspace: Path):
    # Cancellation and receipts must not require valid mutable tasks, roster,
    # checkpoint, runtime, or provider state. This is not generic preflight.
    store = task_attempts.Store(workspace)
    with storage.workspace_lock(workspace, managed_path(workspace, "state/.workspace.lock")) as held:
        yield store, held


def mutate_execution(workspace: Path, operation: str, parameters: dict[str, Any], *,
                     expected_execution_revision: str, expected_tasks_revision: str | None,
                     request_id: str) -> dict[str, Any]:
    view = task_attempts.Store(workspace).read()
    if operation in {"graph_propose", "graph_decide", "prepare_graph"} and getattr(view, "schema_version", 1) != 2:
        raise task_attempts.AttemptError("Task graphs require explicit execution upgrade; existing v1 authority is unchanged")
    binder = (execution_v2.request_binding if operation in {"graph_propose", "graph_decide", "prepare_graph"}
              or (isinstance(expected_execution_revision, str) and expected_execution_revision.startswith("execution-control-v2:"))
              else task_attempts.request_binding)
    binder(operation, parameters, expected_execution_revision, expected_tasks_revision)
    if not isinstance(request_id, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}", request_id):
        raise task_attempts.AttemptError("Invalid execution request key")
    if view is None:
        if operation != "enable":
            raise task_attempts.AttemptError("Execution control is disabled; enable explicitly first")
        if expected_execution_revision != task_attempts.DISABLED:
            raise task_attempts.AttemptError("execution revision conflict")
        # Deterministic invalid workspace/source must not create a legacy lock.
        task_attempts._source(execution_source(workspace, "0" * 32), "0" * 32)
    with execution_control_mutation(workspace) as (store, held):
        return store.mutate(operation, parameters, expected_execution_revision=expected_execution_revision,
            expected_tasks_revision=expected_tasks_revision, request_id=request_id,
            source_loader=lambda source_id: execution_source(workspace, source_id), lock=held)


def print_execution_result(value: dict[str, Any], json_output: bool) -> int:
    print(json.dumps(value, ensure_ascii=True, indent=None if json_output else 2))
    return 2 if value.get("delivery", {}).get("outcome") == "durability_uncertain" else 0


def execution_binding_status(record: dict[str, Any], source: dict[str, Any] | None, error: str | None) -> dict[str, Any]:
    if source is None:
        return {"status": "source_unavailable", "reason": error or "Source not assessed"}
    if record["task_authority_identity"] != source["identity"]:
        return {"status": "source_authority_changed", "reason": "Explicit source rebind is required before new preparation"}
    task = next((item for item in source["tasks"] if item["id"] == record["task_id"]), None)
    if task is None:
        return {"status": "task_missing"}
    if task_attempts.task_snapshot_digest(task) != record["task_snapshot_digest"]:
        return {"status": "task_changed"}
    owner = record["complete_task_snapshot"].get("owner")
    if owner is not None and owner not in source["roster"]:
        return {"status": "owner_missing"}
    if record["task_source_revision"] != source["revision"]:
        return {"status": "source_revision_changed", "reason": "Whole-task authority revision differs, even if selected task content is unchanged"}
    return {"status": "current"}


def graph_inspection(view: execution_v2.View, revision: str, source: dict[str, Any] | None,
                      error: str | None) -> dict[str, Any]:
    graph = view.graphs[revision]
    try:
        match = (task_graphs.match_source(graph["manifest"], source, view.enrollments) if source is not None else
                 {"status": "source_unavailable", "reason": error})
    except storage.StorageError as exc:
        match = {"status": "source_unavailable", "reason": str(exc)}
    current = revision == view.current_graph_revision
    eligible = current and graph["current_review"] == "approved_for_preparation" and match["status"] == "current"
    prepared = {item["prepared"]["task_id"] for item in view.attempts.values() if item["state"] == "prepared"}
    return {"graph_revision": revision, "graph_digest": graph["digest"], "current": current,
            "manifest": graph["manifest"], "planning_review": graph["current_review"],
            "decisions": graph["decisions"], "source_binding": match,
            "dependency_order": task_graphs.topology(graph["manifest"]),
            "preparation_eligible": eligible,
            "nodes": [{"task_id": node["task_id"], "depends_on": node["depends_on"],
                       "preparation_eligible": eligible and node["task_id"] not in prepared,
                       "dependency_acceptance": "unestablished" if node["depends_on"] else "not_required",
                       "authorization": "none", "dispatchable": False} for node in graph["manifest"]["nodes"]],
            "affected_preparations": [item["prepared"]["attempt_id"] for item in view.attempts.values()
                if (item["prepared"].get("graph_binding") or {}).get("graph_revision") == revision],
            "authorization": "none", "dispatchable": False}


def execution_inspection(workspace: Path) -> dict[str, Any]:
    store = task_attempts.Store(workspace)
    result = {"execution": store.inspect_control(), "authorization": "none", "dispatchable": False,
              "blockers": list(task_attempts.BLOCKERS)}
    if result["execution"]["status"] != "active":
        return result
    view = store.read()
    source, error = None, "Coherent source assessment requires the existing workspace lock; none was created"
    diagnostics = io.StringIO()
    try:
        with redirect_stderr(diagnostics):
            lock_path = managed_path(workspace, "state/.workspace.lock")
            if lock_path.exists():
                with storage.workspace_lock(workspace, lock_path):
                    view = store.read()
                    if view is None:
                        result["execution"] = {"status": "disabled", "revision": task_attempts.DISABLED}
                        return result
                    try:
                        source = task_attempts._source(execution_source(workspace, view.snapshot_source_id), view.snapshot_source_id)
                        error = None
                    except (Exception, SystemExit) as exc:
                        error = f"Current source could not be assessed: {diagnostics.getvalue().strip() or str(exc)}"
    except (Exception, SystemExit) as exc:
        state = store.inspect_control()
        if state["status"] != "active":
            result["execution"] = state
            return result
        error = f"Coordinated source assessment unavailable: {diagnostics.getvalue().strip() or str(exc)}"
    result["execution"] = {"status": "active", "revision": view.revision,
                           "execution_authority_id": view.authority_id, "generation": view.generation,
                           "schema_version": getattr(view, "schema_version", 1)}
    result["source_assessment"] = {"coordinated": source is not None, "error": error,
                                   "revision": None if source is None else source["revision"]}
    result["enrollments"] = [{**record, "source_binding": execution_binding_status(record["bindings"][-1], source, error)}
                             for record in view.enrollments.values()]
    result["attempts"] = [{**record, "source_binding": execution_binding_status(record["prepared"], source, error)}
                         for record in view.attempts.values()]
    if getattr(view, "schema_version", 1) == 2:
        result["graphs"] = [graph_inspection(view, revision, source, error) for revision in view.graphs]
        result["current_graph_revision"] = view.current_graph_revision
        result["graph_managed_tasks"] = sorted(view.managed_tasks)
        for attempt in result["attempts"]:
            binding = attempt["prepared"].get("graph_binding")
            if binding is None:
                attempt["graph_binding_status"] = "not_graph_bound"
            elif binding["graph_revision"] != view.current_graph_revision:
                attempt["graph_binding_status"] = "superseded_revision"
            else:
                graph = view.graphs[binding["graph_revision"]]
                report = next(item for item in result["graphs"] if item["graph_revision"] == view.current_graph_revision)
                attempt["graph_binding_status"] = ("planning_review_changed" if graph["current_review"] != "approved_for_preparation"
                    or graph["decisions"][-1]["decision_id"] != binding["decision_id"] else
                    "current" if report["source_binding"]["status"] == "current"
                    else "source_unavailable_or_stale")
    return result


def command_execution_inspect(args: argparse.Namespace) -> int:
    return print_execution_result(execution_inspection(workspace_path(args.workspace)), args.json)


def command_execution_enable(args: argparse.Namespace) -> int:
    return print_execution_result(mutate_execution(workspace_path(args.workspace), "enable", {},
        expected_execution_revision=args.expect_execution_revision, expected_tasks_revision=None, request_id=args.request_id), args.json)


def command_execution_enroll(args: argparse.Namespace) -> int:
    return print_execution_result(mutate_execution(workspace_path(args.workspace), "enroll",
        {"task_id": args.task_id, "rebind_source": args.rebind_source}, expected_execution_revision=args.expect_execution_revision,
        expected_tasks_revision=args.expect_tasks_revision, request_id=args.request_id), args.json)


def command_execution_upgrade(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    execution_v2.request_binding("upgrade", {}, args.expect_execution_revision, None)
    with execution_control_mutation(workspace) as (store, held):
        value = execution_v2.upgrade(store, args.expect_execution_revision, args.request_id, held)
    return print_execution_result(value, args.json)


def read_graph_document(path: str) -> dict[str, Any]:
    try:
        selected = Path(path).expanduser().resolve()
        with os.fdopen(storage._open_regular(selected, os.O_RDONLY), "rb") as stream:
            data = stream.read(task_graphs.MAX_BYTES + 1)
    except (OSError, ValueError, RuntimeError) as exc:
        raise task_graphs.GraphError(f"Cannot read operator-selected graph document: {exc}") from exc
    if len(data) > task_graphs.MAX_BYTES:
        raise task_graphs.GraphError("graph document exceeds 1 MiB")
    return task_graphs.validate_document(task_attempts._json(data))


def command_graph_propose(args: argparse.Namespace) -> int:
    document = read_graph_document(args.document)
    return print_execution_result(mutate_execution(workspace_path(args.workspace), "graph_propose", {"document": document},
        expected_execution_revision=args.expect_execution_revision, expected_tasks_revision=args.expect_tasks_revision,
        request_id=args.request_id), args.json)


def command_graph_decide(args: argparse.Namespace) -> int:
    parameters = {"graph_revision": args.graph_revision, "decision": args.decision, "reason": args.reason,
                  "expected_graph_digest": args.expect_graph_digest}
    return print_execution_result(mutate_execution(workspace_path(args.workspace), "graph_decide", parameters,
        expected_execution_revision=args.expect_execution_revision, expected_tasks_revision=args.expect_tasks_revision,
        request_id=args.request_id), args.json)


def command_graph_inspect(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) > 1:
        raise task_graphs.GraphError("graph inspect accepts one optional graph revision")
    value = execution_inspection(workspace)
    if value["execution"]["status"] != "active":
        return print_execution_result(value, args.json)
    if value["execution"]["schema_version"] != 2:
        raise task_attempts.AttemptError("Task graphs require explicit execution upgrade; v1 remains unchanged")
    revision = (rest[0] if rest else None) or value["current_graph_revision"]
    if revision is None:
        return print_execution_result({"execution": value["execution"], "graph": None,
            "authorization": "none", "dispatchable": False}, args.json)
    if not re.fullmatch(task_attempts.UUID, revision):
        raise task_graphs.GraphError("Invalid graph revision")
    graph = next((item for item in value["graphs"] if item["graph_revision"] == revision), None)
    if graph is None:
        raise task_graphs.GraphError("Unknown graph revision")
    return print_execution_result({"execution": value["execution"], "graph": graph,
                                   "authorization": "none", "dispatchable": False}, args.json)


def command_graph_export(args: argparse.Namespace) -> int:
    if not re.fullmatch(task_attempts.UUID, args.graph_revision):
        raise task_graphs.GraphError("Invalid graph revision")
    view = task_attempts.Store(workspace_path(args.workspace)).read()
    if view is None or getattr(view, "schema_version", 1) != 2:
        raise task_attempts.AttemptError("Task graphs require explicit execution upgrade")
    graph = view.graphs.get(args.graph_revision)
    if graph is None:
        raise task_graphs.GraphError("Unknown graph revision")
    return print_execution_result({"manifest": graph["manifest"], "graph_digest": graph["digest"],
        "execution_revision": view.revision, "authorization": "none", "dispatchable": False}, args.json)


def read_attempt_intent(path: str) -> dict[str, Any]:
    try:
        selected = Path(path).expanduser().resolve()
        with os.fdopen(storage._open_regular(selected, os.O_RDONLY), "rb") as stream:
            data = stream.read(1024 * 1024 + 1)
    except (OSError, ValueError, RuntimeError) as exc:
        raise task_attempts.AttemptError(f"Cannot read operator-selected intent document: {exc}") from exc
    if len(data) > 1024 * 1024:
        raise task_attempts.AttemptError("intent document exceeds 1 MiB")
    return task_attempts.validate_intent(task_attempts._json(data))


def command_attempt_prepare(args: argparse.Namespace) -> int:
    graph_revision = getattr(args, "graph_revision", None)
    operation = "prepare_graph" if graph_revision is not None else "prepare"
    parameters = ({"task_id": args.task_id, "graph_revision": graph_revision} if graph_revision is not None else
                  {"task_id": args.task_id, "intent": read_attempt_intent(args.intent)})
    return print_execution_result(mutate_execution(workspace_path(args.workspace), operation, parameters,
        expected_execution_revision=args.expect_execution_revision, expected_tasks_revision=args.expect_tasks_revision,
        request_id=args.request_id), args.json)


def command_attempt_cancel(args: argparse.Namespace) -> int:
    return print_execution_result(mutate_execution(workspace_path(args.workspace), "cancel",
        {"attempt_id": args.attempt_id, "reason": args.reason}, expected_execution_revision=args.expect_execution_revision,
        expected_tasks_revision=None, request_id=args.request_id), args.json)


def command_attempt_list(args: argparse.Namespace) -> int:
    value = execution_inspection(workspace_path(args.workspace))
    if args.task is not None:
        record_id(args.task, "task")
        if "attempts" in value:
            value["attempts"] = [item for item in value["attempts"] if item["prepared"]["task_id"] == args.task]
    value.pop("enrollments", None)
    return print_execution_result(value, args.json)


def command_attempt_inspect(args: argparse.Namespace) -> int:
    if not re.fullmatch(r"[0-9a-f]{32}", args.attempt_id):
        raise task_attempts.AttemptError("Invalid attempt ID")
    value = execution_inspection(workspace_path(args.workspace))
    if "attempts" in value:
        selected = next((item for item in value["attempts"] if item["prepared"]["attempt_id"] == args.attempt_id), None)
        if selected is None:
            raise task_attempts.AttemptError("Unknown attempt ID")
        value["attempt"] = selected
        value.pop("attempts")
    value.pop("enrollments", None)
    return print_execution_result(value, args.json)


def command_execution_recover(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    if not args.archive_pending and (not isinstance(args.expect_execution_revision, str)
            or not re.fullmatch(task_attempts.ALL_EXEC_REV, args.expect_execution_revision)):
        raise task_attempts.AttemptError("--confirm-authority requires an inspected active --expect-execution-revision")
    with execution_control_mutation(workspace) as (store, held):
        if args.archive_pending:
            if args.expect_execution_revision is not None:
                raise task_attempts.AttemptError("Archive does not accept an execution revision; active control can never be archived")
            value = {"outcome": "archived_pending", "archive_path": store.archive_pending(held), "revision": task_attempts.DISABLED}
        else:
            view = store.read()
            if view is None or args.expect_execution_revision != view.revision:
                raise task_attempts.AttemptError("Execution revision conflict during confirmation")
            store.confirm_durability(view, held)
            value = {"outcome": "confirmed", "revision": view.revision}
    return print_execution_result({**value, "authorization": "none", "dispatchable": False}, args.json)


def print_file(path: Path, default: str) -> int:
    if not path.exists():
        print(default)
        return 0
    print(path.read_text(encoding="utf-8"), end="")
    return 0


def iter_jsonl(path: Path) -> list[dict[str, Any]]:
    return storage.read_jsonl(path)


def print_pretty_jsonl(path: Path, kind: str) -> int:
    items = iter_jsonl(path)
    if not items:
        print(f"No {kind} yet")
        return 0
    for item in items:
        ts = item.get("ts", "unknown-time")
        if kind == "messages":
            sender = item.get("from", "?")
            recipient = item.get("to", "?")
            msg_type = item.get("type", "message")
            body = item.get("message") or item.get("request") or item.get("body") or ""
            checkpoint = f" checkpoint={item['checkpoint']}" if item.get("checkpoint") else ""
            task = f" task={item['task_id']}" if item.get("task_id") else ""
            print(f"- {ts} [{msg_type}{checkpoint}{task}] {sender} -> {recipient}: {body}")
        else:
            event_type = item.get("type", "event")
            agent = f" agent={item['agent']}" if item.get("agent") else ""
            checkpoint = f" checkpoint={item['checkpoint']}" if item.get("checkpoint") else ""
            task_obj = item.get("task")
            task_id = item.get("task_id") or (task_obj.get("id") if isinstance(task_obj, dict) else None)
            task = f" task={task_id}" if task_id else ""
            print(f"- {ts} [{event_type}{agent}{checkpoint}{task}]")
    return 0


def herdr_workspace_label(team: dict[str, Any]) -> str:
    return f"coven-{slugify(str(team['name']))}"


def run_herdr(args: list[str], *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["herdr", *args], check=False, capture_output=capture, text=True)


def herdr_result(result: subprocess.CompletedProcess[str], message: str) -> dict[str, Any]:
    if result.returncode != 0:
        fail(result.stderr.strip() or message)
    try:
        payload = json.loads(result.stdout or "{}")
    except json.JSONDecodeError:
        fail(f"{message}: invalid Herdr JSON response")
    response = payload.get("result")
    return response if isinstance(response, dict) else {}


def find_herdr_workspace(team: dict[str, Any]) -> dict[str, Any] | None:
    result = run_herdr(["workspace", "list"], capture=True)
    if result.returncode != 0:
        return None
    try:
        workspaces = json.loads(result.stdout).get("result", {}).get("workspaces", [])
    except json.JSONDecodeError:
        return None
    label = herdr_workspace_label(team)
    for workspace in workspaces if isinstance(workspaces, list) else []:
        if isinstance(workspace, dict) and workspace.get("label") == label:
            return workspace
    return None


def find_herdr_tab(team: dict[str, Any], agent: str) -> dict[str, Any] | None:
    workspace_info = find_herdr_workspace(team)
    if not workspace_info:
        return None
    workspace_id = str(workspace_info.get("workspace_id") or "")
    if not workspace_id:
        return None
    result = run_herdr(["tab", "list", "--workspace", workspace_id], capture=True)
    if result.returncode != 0:
        return None
    try:
        tabs = json.loads(result.stdout).get("result", {}).get("tabs", [])
    except json.JSONDecodeError:
        return None
    label = f"agent-{agent}"
    for tab in tabs if isinstance(tabs, list) else []:
        if isinstance(tab, dict) and tab.get("label") == label:
            return tab
    return None


def running_herdr_agent_target(team: dict[str, Any], agent: str) -> tuple[str, str] | None:
    tab = find_herdr_tab(team, agent)
    if not tab:
        return None
    workspace_id = str(tab.get("workspace_id") or "")
    tab_id = str(tab.get("tab_id") or "")
    if not workspace_id or not tab_id:
        return None
    result = run_herdr(["pane", "list", "--workspace", workspace_id], capture=True)
    if result.returncode != 0:
        return None
    try:
        panes = json.loads(result.stdout).get("result", {}).get("panes", [])
    except json.JSONDecodeError:
        return None
    for pane in panes if isinstance(panes, list) else []:
        if isinstance(pane, dict) and pane.get("tab_id") == tab_id:
            pane_id = str(pane.get("pane_id") or "")
            if pane_id:
                return pane_id, tab_id
    return None


def running_tmux_agent_pane(team: dict[str, Any], agent: str) -> str | None:
    session = team_session_name(team)
    window_target = f"{session}:agent-{agent}"
    pane_lookup = subprocess.run(
        ["tmux", "list-panes", "-t", window_target, "-F", "#{pane_id}"],
        check=False,
        capture_output=True,
        text=True,
    )
    if pane_lookup.returncode != 0 or not pane_lookup.stdout.strip():
        return None
    return pane_lookup.stdout.strip().splitlines()[0]


def running_agent_pane(team: dict[str, Any], agent: str) -> str | None:
    if runtime_multiplexer(team) == "herdr":
        target = running_herdr_agent_target(team, agent)
        return target[0] if target else None
    return running_tmux_agent_pane(team, agent)


def send_to_agent_pane(team: dict[str, Any], agent: str, text: str) -> bool:
    """Low-level transport; workspace-aware callers perform state preflight."""
    root = team.get("_workspace_root")
    require_agent(team, agent)
    if not isinstance(root, str):
        raise actions.ActionError("Legacy transport requires a loaded workspace identity")
    with legacy_runtime_transport(Path(root)):
        return _send_to_agent_pane_legacy(team, agent, text)


def _send_to_agent_pane_legacy(team: dict[str, Any], agent: str, text: str) -> bool:
    require_agent(team, agent)
    if team.get("_runtime_launch_mode"):
        raise actions.ActionError("Legacy pane delivery is disabled in approved-launch mode; launch approval is not task-dispatch permission")
    target = running_agent_pane(team, agent)
    if not target:
        return False
    if runtime_multiplexer(team) == "herdr":
        commands = [["herdr", "pane", "send-text", target, text], ["herdr", "pane", "send-keys", target, "enter"]]
    else:
        commands = [["tmux", "send-keys", "-t", target, text, "Enter"]]
    for command in commands:
        result = subprocess.run(command, check=False, capture_output=True, text=True)
        if result.returncode != 0:
            raise storage.StorageError(f"Agent pane delivery failed for {agent}: {result.stderr.strip() or 'transport rejected the command'}")
    return True


def team_agents(team: dict[str, Any]) -> list[str]:
    try:
        return records.agent_ids(team.get("agents"))
    except ValueError as exc:
        fail(str(exc))


def require_agent(team: dict[str, Any], agent: str) -> None:
    record_id(agent, "agent")
    agents = team_agents(team)
    if agent not in agents:
        fail(f"Unknown agent `{agent}`. Known agents: {', '.join(agents)}")


def resolve_agent(team: dict[str, Any], value: str) -> str:
    """Resolve a human-facing owner name without rewriting persisted identities."""
    agents = team_agents(team)
    try:
        canonical = slugify(value)
    except ValueError as exc:
        # Older versions accepted underscore-only IDs. Keep exact legacy lookup
        # possible, without allowing those names in newly initialized rosters.
        if value in agents:
            return value
        fail(str(exc))
    matches = []
    for agent in agents:
        try:
            if slugify(agent) == canonical:
                matches.append(agent)
        except ValueError:
            continue
    if len(matches) != 1:
        fail(f"Unknown or ambiguous agent `{value}`. Known agents: {', '.join(agents)}")
    return matches[0]


def agent_window_target(team: dict[str, Any], agent: str) -> str:
    return f"{team_session_name(team)}:agent-{agent}"


def agent_runtime_status(team: dict[str, Any], agent: str) -> tuple[str, str | None]:
    record_id(agent, "agent")
    if team.get("_runtime_launch_mode"):
        return "unverified (inspect action transport records)", None
    pane = running_agent_pane(team, agent)
    return ("running", pane) if pane else ("stopped", None)


@legacy_transport_effect
def start_agent_window(workspace: Path, team: dict[str, Any], agent: str) -> bool:
    preflight_workspace_paths(workspace, team)
    require_legacy_runtime(workspace)
    require_agent(team, agent)
    status, pane = agent_runtime_status(team, agent)
    if status == "running":
        info(f"{agent} already running at {pane}")
        return False

    if runtime_multiplexer(team) == "herdr":
        workspace_info = find_herdr_workspace(team)
        if not workspace_info:
            fail(f"Coven Herdr workspace is not running: {herdr_workspace_label(team)}. Run `coven up` first.")
        workspace_id = str(workspace_info.get("workspace_id") or "")
        if not workspace_id:
            fail("Could not resolve Coven Herdr workspace id")
        opencode_command = str(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)
        command = agent_launch_command(str(team["name"]), workspace, agent, opencode_command, "herdr")
        tab_result = run_herdr(
            ["tab", "create", "--workspace", workspace_id, "--cwd", str(workspace), "--label", f"agent-{agent}", "--focus"],
            capture=True,
        )
        tab_payload = herdr_result(tab_result, f"Failed to create Herdr tab for agent `{agent}`")
        pane_id = str(tab_payload.get("root_pane", {}).get("pane_id") or "")
        if not pane_id:
            fail(f"Could not resolve Herdr pane for agent `{agent}`")
        run_herdr(["pane", "rename", pane_id, agent], capture=True)
        run_herdr(["pane", "run", pane_id, command], capture=True)
        append_event(workspace, "agent.started.tab", agent=agent, multiplexer="herdr")
        return True

    session = team_session_name(team)
    session_check = subprocess.run(["tmux", "has-session", "-t", session], check=False, capture_output=True, text=True)
    if session_check.returncode != 0:
        fail(f"Coven tmux session is not running: {session}. Run `coven start` first.")

    opencode_command = str(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)
    command = agent_launch_command(str(team["name"]), workspace, agent, opencode_command, "tmux")
    result = subprocess.run(
        ["tmux", "new-window", "-t", session, "-n", f"agent-{agent}", "-c", str(workspace), command],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        fail(result.stderr.strip() or f"Failed to start agent `{agent}`")
    append_event(workspace, "agent.started.window", agent=agent)
    return True


@legacy_transport_effect
def stop_agent_window(workspace: Path, team: dict[str, Any], agent: str) -> bool:
    preflight_workspace_paths(workspace, team)
    require_legacy_runtime(workspace)
    require_agent(team, agent)
    status, _pane = agent_runtime_status(team, agent)
    if status != "running":
        info(f"{agent} is not running")
        return False

    if runtime_multiplexer(team) == "herdr":
        target = running_herdr_agent_target(team, agent)
        if not target:
            info(f"{agent} is not running")
            return False
        _pane_id, tab_id = target
        result = run_herdr(["tab", "close", tab_id], capture=True)
        if result.returncode != 0:
            fail(result.stderr.strip() or f"Failed to stop Herdr agent tab `{agent}`")
        append_event(workspace, "agent.stopped.tab", agent=agent, multiplexer="herdr")
        return True

    target = agent_window_target(team, agent)
    result = subprocess.run(["tmux", "kill-window", "-t", target], check=False, capture_output=True, text=True)
    if result.returncode != 0:
        fail(result.stderr.strip() or f"Failed to stop agent `{agent}`")
    append_event(workspace, "agent.stopped.window", agent=agent)
    return True


def command_status(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    refresh_dashboard(workspace)
    return print_file(workspace / "dashboard.md", "No dashboard yet\n")


def command_events(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    if args.raw:
        return print_file(workspace / "logs" / "events.jsonl", "No events yet\n")
    return print_pretty_jsonl(workspace / "logs" / "events.jsonl", "events")


def command_messages(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    if args.raw:
        return print_file(workspace / "logs" / "messages.jsonl", "No messages yet\n")
    return print_pretty_jsonl(workspace / "logs" / "messages.jsonl", "messages")


def render_standup(workspace: Path) -> str:
    snapshot = load_workspace_state(workspace)
    team = snapshot.team
    multiplexer = runtime_multiplexer(team)
    tasks, agents, lead = snapshot.tasks, snapshot.agents, snapshot.lead

    def task_line(task: dict[str, Any]) -> str:
        return f"- `{task.get('id')}` — {task.get('title')} ({task.get('owner') or 'unassigned'})"

    done: list[str] = []
    in_progress: list[str] = []
    blocked: list[str] = []
    pending: list[str] = []
    for task in tasks if isinstance(tasks, list) else []:
        if not isinstance(task, dict):
            continue
        status = str(task.get("status", "")).lower()
        line = task_line(task)
        if status in {"completed", "done"}:
            done.append(line)
        elif "block" in status or "wait" in status:
            blocked.append(f"{line} — {task.get('status')}")
        elif status in {"in_progress", "assigned", "active", "review"}:
            in_progress.append(f"{line} — {task.get('status')}")
        else:
            pending.append(f"{line} — {task.get('status') or 'pending'}")

    agent_lines = []
    for agent_state in agents if isinstance(agents, list) else []:
        if not isinstance(agent_state, dict):
            continue
        agent = str(agent_state.get("id"))
        runtime, pane = agent_runtime_status(team, agent)
        pane_text = f" at {pane}" if pane else ""
        label = markdown_text(agent_display_name(team, agent))
        agent_lines.append(f"- `{agent}` ({label}) — state={agent_state.get('status', 'unknown')}, {multiplexer}={runtime}{pane_text}")

    checkpoint_lines = []
    for checkpoint_id in ("plan-approval", "pre-delivery-review"):
        status = checkpoint_status(workspace, checkpoint_id)
        checkpoint_lines.append(f"- `{checkpoint_id}` — {status} (`checkpoints/{checkpoint_id}.md`)")

    lead_lines = ["- None set."]
    if lead:
        agent = str(lead.get("agent"))
        runtime, pane = agent_runtime_status(team, agent)
        pane_text = f" at {pane}" if pane else ""
        lead_lines = [
            f"- `{agent}` — {multiplexer}={runtime}{pane_text}",
            f"- Set: {lead.get('set_at', 'unknown time')}",
            f"- Stop condition: {lead.get('until', 'goal achieved or critical blocker')}",
        ]

    def section(title: str, lines: list[str], empty: str) -> str:
        return f"## {title}\n\n" + ("\n".join(lines) if lines else empty) + "\n"

    suggestions: list[str] = []
    if blocked:
        suggestions.append("- Clear the blocked task(s) or message the owner for a handoff.")
    if checkpoint_status(workspace, "pre-delivery-review") != "approved" and done and not blocked:
        suggestions.append("- Prepare or review the pre-delivery checkpoint when QA/review evidence is ready.")
    if not suggestions:
        suggestions.append("- Continue from the dashboard and current task assignments.")

    return "\n".join(
        [
            f"# Coven Standup: {team['name']}",
            section("Coven Lead", lead_lines, "- None set."),
            section("Done", done, "- Nothing completed yet."),
            section("In Progress", in_progress, "- Nothing in progress."),
            section("Blocked / Waiting", blocked, "- No blockers recorded."),
            section("Pending", pending, "- No pending tasks."),
            section("Agents", agent_lines, "- No agents recorded."),
            section("Checkpoints", checkpoint_lines, "- No checkpoints recorded."),
            section("Suggested Next Action", suggestions, "- Inspect `coven status`."),
        ]
    )


def command_standup(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    print(render_standup(workspace))
    return 0


def checkpoint_requests(workspace: Path) -> list[dict[str, Any]]:
    records = checkpoint_log_records(workspace, "messages") + checkpoint_log_records(workspace, "events")
    requests = [record for record in records if record.get("checkpoint") and "request" in str(record.get("type", ""))]

    def timestamp(record: dict[str, Any]) -> datetime:
        # Best-effort ordering across two legacy logs, not a sequenced ledger.
        try:
            value = datetime.fromisoformat(str(record.get("ts", "")))
            return value.replace(tzinfo=timezone.utc) if value.tzinfo is None else value.astimezone(timezone.utc)
        except (ValueError, OverflowError):
            return datetime.min.replace(tzinfo=timezone.utc)

    return sorted(requests, key=timestamp)


def checkpoint_review_candidates(workspace: Path) -> list[str]:
    preflight_workspace_paths(workspace)
    requested: list[str] = []
    seen: set[str] = set()
    for item in checkpoint_requests(workspace):
        checkpoint = item.get("checkpoint")
        if checkpoint and "request" in str(item.get("type", "")):
            checkpoint_id = record_id(checkpoint, "checkpoint")
            if checkpoint_id not in seen:
                seen.add(checkpoint_id)
                requested.append(checkpoint_id)

    pending_requested = [
        checkpoint
        for checkpoint in requested
        if checkpoint_status(workspace, checkpoint) != "approved"
    ]
    if pending_requested:
        return pending_requested

    checkpoint_dir = managed_path(workspace, "checkpoints", directory=True)
    if not checkpoint_dir.exists():
        return []
    pending_files = []
    for path in sorted(checkpoint_dir.glob("*.md")):
        if checkpoint_status(workspace, path.stem) != "approved":
            pending_files.append(path.stem)
    return pending_files


def prompt_multiline(prompt: str) -> str:
    print(prompt)
    print("Finish with a single '.' on its own line.")
    lines: list[str] = []
    while True:
        try:
            line = input("> ")
        except EOFError:
            break
        if line == ".":
            break
        lines.append(line)
    return "\n".join(lines).strip()


def notify_agents(workspace: Path, team: dict[str, Any], agent_selector: str, message: str, *, event_type: str, checkpoint: str | None = None) -> tuple[int, list[str]]:
    preflight_workspace_paths(workspace, team)
    require_legacy_runtime(workspace)
    agents = team_agents(team) if agent_selector == "all" else [agent_selector]
    for agent in agents:
        require_agent(team, agent)
    sent = 0
    missing: list[str] = []
    for agent in agents:
        if send_to_agent_pane(team, agent, message):
            payload: dict[str, Any] = {"agent": agent}
            if checkpoint:
                payload["checkpoint"] = checkpoint
            append_event(workspace, event_type, **payload)
            sent += 1
        else:
            missing.append(agent)
    return sent, missing


def check_checkpoint_revision(expected: str | None, actual: str) -> None:
    if expected is not None and expected != actual:
        raise storage.RevisionConflict(f"Stale checkpoint revision: expected {expected}; current {actual}; no decision applied")


def checkpoint_proposal(workspace: Path, checkpoint: str, action: str, notes: str, source: str,
                        decision_id: str, expected_revision: str | None):
    data = checkpoint_bytes(workspace, checkpoint)
    before = checkpoints.inspect(checkpoint, data)
    check_checkpoint_revision(expected_revision, before.revision)
    template = checkpoint_file(checkpoint, checkpoint.replace("-", " ").title(), "User approval checkpoint.").encode("utf-8")
    encoded, final = checkpoints.prepare(checkpoint, data, action, notes, decision_id=decision_id,
                                         decided_at=now(), source=source, initial_content=template)
    return before, encoded, final


def deliver_checkpoint_notice(workspace: Path, checkpoint: str, selector: str, result: dict[str, Any]) -> None:
    """Observational delivery only; never hold a workspace lock across runtime I/O."""
    try:
        team = load_team(workspace)
        agents = team_agents(team) if selector == "all" else [selector]
        for agent in agents:
            require_agent(team, agent)
    except (Exception, SystemExit) as exc:
        result["warnings"].append(f"Checkpoint committed; notifications unavailable: {exc}")
        return
    message = (f"Checkpoint `{checkpoint}` decision {result['decision_id']} recorded as {result['action']} "
               f"for scope {result['scope_revision']}. Reinspect current checkpoint status before acting. "
               "This notice is not execution authorization.")
    for agent in agents:
        try:
            current = checkpoint_view(workspace, checkpoint)
            if (not current.decision or current.decision["decision_id"] != result["decision_id"]
                    or current.effective_status != result["action"]):
                result["warnings"].append("Checkpoint notice suppressed: decision superseded or no longer effective")
                break
            if not send_to_agent_pane(team, agent, message):
                result["missing"].append(agent)
                continue
            result["sent"] += 1
            append_event(workspace, "checkpoint.notice.sent", checkpoint=checkpoint, agent=agent,
                         decision_id=result["decision_id"], scope_revision=result["scope_revision"])
        except (Exception, SystemExit) as exc:
            result["warnings"].append(f"Checkpoint committed; notice delivery/history failed for {agent}: {exc}")


def decide_checkpoint(workspace: Path, team: dict[str, Any], checkpoint: str, action: str, notes: str,
                      agent_selector: str = "all", *, expected_revision: str | None = None,
                      source: str = "cli") -> dict[str, Any]:
    checkpoints.validate_revision(expected_revision)
    preflight_workspace_paths(workspace, team)
    checkpoint_path(workspace, checkpoint)
    if agent_selector != "all":
        require_agent(team, agent_selector)
    if action == "changes_requested" and (not isinstance(notes, str) or not notes.strip()):
        fail("Change request cannot be empty")
    decision_id = uuid.uuid4().hex
    # Discard this draft; predictable input failures must not create a lock.
    checkpoint_proposal(workspace, checkpoint, action, notes, source, decision_id, expected_revision)
    with workspace_mutation(workspace) as held:
        team = load_team(workspace)
        if agent_selector != "all":
            require_agent(team, agent_selector)
        before, encoded, final = checkpoint_proposal(workspace, checkpoint, action, notes, source, decision_id, expected_revision)
        result = {"outcome": "committed", "checkpoint": checkpoint, "action": action, "decision_id": decision_id,
                  "selection": "conditional" if expected_revision is not None else "latest",
                  "before_revision": before.revision, "after_revision": final.revision,
                  "scope_revision": final.scope_revision, "warnings": [], "sent": 0, "missing": []}
        path = checkpoint_path(workspace, checkpoint)
        created_parent = not path.parent.exists()
        path.parent.mkdir(parents=True, exist_ok=True)
        if created_parent:
            try:
                storage.sync_directory(path.parent.parent)
            except OSError as exc:
                raise storage.StorageError(f"Checkpoint directory creation could not be synced; no decision published: {exc}") from exc
        try:
            storage.atomic_replace_bytes(path, encoded)
        except storage.CommitUncertain as exc:
            result["outcome"] = "durability_uncertain"
            result["warnings"].append(str(exc))
            return result
        event_type = "checkpoint.approved" if action == "approved" else "checkpoint.changes.requested"
        for label, callback in (
            ("event history incomplete", lambda: append_event(workspace, event_type, lock=held,
                checkpoint=checkpoint, by="user", source=source, decision_id=decision_id,
                scope_revision=final.scope_revision, reviewed_revision=before.revision)),
            ("message history incomplete", lambda: append_message(workspace, "user", "coven", notes, lock=held)),
            ("dashboard stale", lambda: refresh_dashboard(workspace, lock=held)),
        ):
            try:
                callback()
            except (Exception, SystemExit) as exc:
                result["warnings"].append(f"Checkpoint committed; {label}: {exc}")
    deliver_checkpoint_notice(workspace, checkpoint, agent_selector, result)
    return result


def approve_checkpoint(workspace: Path, team: dict[str, Any], checkpoint: str, message: str, agent_selector: str = "all", *,
                       expected_revision: str | None = None, source: str = "cli") -> dict[str, Any]:
    return decide_checkpoint(workspace, team, checkpoint, "approved", message, agent_selector,
                             expected_revision=expected_revision, source=source)


def suggest_checkpoint_changes(workspace: Path, team: dict[str, Any], checkpoint: str, notes: str, agent_selector: str = "all", *,
                                expected_revision: str | None = None, source: str = "cli") -> dict[str, Any]:
    return decide_checkpoint(workspace, team, checkpoint, "changes_requested", notes, agent_selector,
                             expected_revision=expected_revision, source=source)


def print_checkpoint_result(result: dict[str, Any], *, json_output: bool = False) -> int:
    if json_output:
        print(json.dumps(result, ensure_ascii=False))
    else:
        label = "Recorded" if result["outcome"] == "committed" else "Replaced; durability uncertain for"
        info(f"{label} {result['checkpoint']} decision {result['decision_id']}: {result['action']} (selection={result['selection']})")
        if result["missing"]:
            info(f"No running pane found for: {', '.join(result['missing'])}")
        for warning in result["warnings"]:
            print(f"[WARN] {warning}", file=sys.stderr)
    return 0 if result["outcome"] == "committed" else 2


def command_checkpoint_inspect(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    load_team(workspace)
    if len(rest) > 1:
        fail("checkpoint inspect accepts one checkpoint ID")
    checkpoint = rest[0] if rest else "plan-approval"
    view = checkpoint_view(workspace, checkpoint)
    print(json.dumps(view.to_dict(), ensure_ascii=False, indent=None if args.json else 2))
    return 0


def lead_prompt(agent: str, workspace: Path, goal: str) -> str:
    return f"""You are the coven lead for this workspace until the goal is achieved or there is a critical blocker requiring the user.

Workspace: {workspace}
Goal: {goal or 'See goal.md'}

Your operating loop:
1. Run or inspect `coven standup` to see coven status, blockers, checkpoints, and agent runtime state.
2. Use `coven send <agent|all> <message>` to move agents along through the configured terminal multiplexer.
3. Make your best decision to unblock the coven without waiting for the user unless a checkpoint, destructive action, missing requirement, or critical blocker requires user input.
4. Use `coven task list --json`, task add/assign with `--expect-revision` when appropriate, `coven message`, and `coven refresh`; never directly edit shared task JSON, logs, or dashboard. Report unsupported lifecycle updates as messages.
5. Use `work/` for in-progress artifacts, `output/` for delivery artifacts, and `checkpoints/` for user decisions.
6. When the goal is achieved, prepare the pre-delivery review checkpoint and tell the user what to review.

Start now: run a standup mentally/from files, identify the next unblock action, message the relevant agent(s), and record what you did.
"""


def command_lead(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    team = load_team(workspace)
    if getattr(args, "clear", False):
        rest = ["clear"]
    if not rest or rest[0] in {"status", "show"}:
        state = read_lead(workspace)
        if not state:
            print("No coven lead set.")
            return 0
        multiplexer = runtime_multiplexer(team)
        agent = str(state.get("agent"))
        runtime, pane = agent_runtime_status(team, agent)
        pane_text = f" pane={pane}" if pane else ""
        print(f"Coven lead: {agent_label(team, agent)} {multiplexer}={runtime}{pane_text}")
        print(f"Set: {state.get('set_at', 'unknown time')}")
        print(f"Until: {state.get('until', 'goal achieved or critical blocker')}")
        return 0

    if rest[0] in {"clear", "remove", "unset"}:
        with workspace_mutation(workspace) as held:
            previous = read_lead(workspace)
            write_lead(workspace, None, lock=held)
            for callback in (lambda: append_event(workspace, "lead.cleared", previous=previous, lock=held),
                             lambda: refresh_dashboard(workspace, lock=held)):
                try:
                    callback()
                except (Exception, SystemExit) as exc:
                    print(f"[WARN] Lead cleared; secondary update failed: {exc}", file=sys.stderr)
        info("Cleared coven lead")
        return 0

    agent = rest[1] if rest[0] == "set" and len(rest) > 1 else rest[0]
    require_agent(team, agent)
    with workspace_mutation(workspace) as held:
        team = load_team(workspace)
        require_agent(team, agent)
        goal = (managed_text(workspace, "goal.md") or "").strip().removeprefix("# Goal").strip()
        state = {**(read_lead(workspace) or {}), "agent": agent, "set_at": now(),
                 "until": "goal achieved or critical blocker requiring the user",
                 "instructions": "Coordinate using supported task/message operations; runtime launch approval is separate."}
        message = args.message or lead_prompt(agent, workspace, goal)
        write_lead(workspace, state, lock=held)
        for callback in (lambda: append_event(workspace, "lead.set", agent=agent, lock=held),
                         lambda: append_message(workspace, "user", agent, message, lock=held),
                         lambda: refresh_dashboard(workspace, lock=held)):
            try:
                callback()
            except (Exception, SystemExit) as exc:
                print(f"[WARN] Lead committed; secondary update failed: {exc}", file=sys.stderr)
    if team.get("_runtime_launch_mode"):
        sent, missing = 0, [agent]
        info("Lead metadata recorded; legacy pane delivery is disabled in approved-launch mode")
    else:
        try:
            sent, missing = notify_agents(workspace, team, agent, message, event_type="lead.message.sent")
        except (Exception, SystemExit) as exc:
            print(f"[WARN] Lead committed; delivery unavailable: {exc}", file=sys.stderr)
            sent, missing = 0, [agent]
    if missing:
        info(f"No running pane found for: {', '.join(missing)}")
    info(f"Set {agent} as coven lead; notified {sent} agent(s)")
    return 0


def command_next(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    approvals = [
        request for request in checkpoint_requests(workspace)
        if checkpoint_status(workspace, request["checkpoint"]) != "approved"
    ]
    if not approvals:
        directory = managed_path(workspace, "checkpoints", directory=True)
        for path in sorted(directory.glob("*.md")):
            if checkpoint_status(workspace, path.stem) in {"approval_stale", "approved_unbound", "decision_invalid"}:
                approvals.append({"checkpoint": path.stem})
    if approvals:
        checkpoint = approvals[-1].get("checkpoint")
        print(f"Next action: approve checkpoint `{checkpoint}`")
        print("Reason: one or more agents are blocked waiting for user approval.")
        print(f"Review file: {checkpoint_path(workspace, checkpoint)}")
        suggested = f"coven approve {checkpoint}" if workspace == workspace_path(".") else f"coven approve {workspace} {checkpoint}"
        print(f"Suggested: {suggested}")
        return 0
    print("Next action: inspect status/dashboard")
    suggested = "coven status" if workspace == workspace_path(".") else f"coven status {workspace}"
    print(f"Suggested: {suggested}")
    return 0


def command_send(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 2:
        fail("send requires an agent (or 'all') and a message")
    agent_arg = rest[0]
    message = " ".join(rest[1:])
    team = load_team(workspace)
    require_legacy_runtime(workspace)
    agents = team_agents(team) if agent_arg == "all" else [agent_arg]
    for agent in agents:
        require_agent(team, agent)
    missing: list[str] = []
    for agent in agents:
        if send_to_agent_pane(team, agent, message):
            append_message(workspace, "user", agent, message)
            append_event(workspace, "agent.message.sent", agent=agent)
        else:
            missing.append(agent)
    refresh_dashboard(workspace)
    if missing:
        info(f"No running pane found for: {', '.join(missing)}")
    info(f"Sent message to {len(agents) - len(missing)} agent(s)")
    return 0


def command_session(args: argparse.Namespace) -> int:
    workspace = Path(args.workspace).absolute()
    try:
        kitsune_observation._arguments(args.socket, args.timeout, 1048576)
        plan = interactive_sessions.plan(str(workspace), args.session_operation, args.runtime,
            args.executable, args.socket, args.pane, args.expected_terminal, getattr(args, "session", None))
        if args.execute:
            load_team(workspace)
            with legacy_runtime_transport(workspace):
                gate = _transport_gates.entries[str(workspace.resolve())]
                report = interactive_sessions.execute(plan, gate=gate, timeout=args.timeout)
        else:
            report = interactive_sessions.preview(plan)
    except ValueError as exc:
        fail(str(exc))
    if args.json:
        print(json.dumps(report, ensure_ascii=True, allow_nan=False))
    else:
        print(f"{report['outcome']}: {report['operation']} Djinn UI in {report['pane_id']}")
        print(f"Command: {shlex.join(report['argv'])}")
        if report['outcome'] == 'preview':
            print("Preview only; --execute explicitly submits to a quiescent empty shell prompt.")
        else:
            print("Input acknowledgement is not proof of native startup, resume or readiness.")
        for diagnostic in report["diagnostics"]:
            print(f"Diagnostic: {diagnostic}")
    return interactive_sessions.exit_code(report)


def command_session_inspect(args: argparse.Namespace) -> int:
    try:
        report = session_inspection.inspect(Path(args.workspace), args.socket,
            pane_id=getattr(args, "pane", None), expected_terminal_id=getattr(args, "expected_terminal", None),
            runtime=args.runtime, timeout=args.timeout, max_response_bytes=args.max_response_bytes)
    except ValueError as exc:
        fail(str(exc))
    if args.json or args.session_operation == "inspect":
        print(json.dumps(report, indent=None if args.json else 2, ensure_ascii=True, allow_nan=False))
    else:
        print(f"Session target inspection: {report['assessment']}")
        print("Scope: existing panes on the selected endpoint; native activity/readiness remains unknown.")
        for target in report["targets"]:
            declarations = "; ".join(f"{a['group_key']}/{a['member_key']}: {a['role']} [{a['binding_check']}]"
                                     for a in target["associations"]) or "no matching declaration"
            print(visible_text(f"{target['pane_id']}  {target['terminal_id']}  {target['recognition']}  {declarations}"))
        if report["assessment"] == "observed":
            print(f"Unmatched declarations: {len(report['declarations']['unmatched'])}")
        for diagnostic in report["diagnostics"]:
            print(f"Diagnostic: {diagnostic}")
    return session_inspection.exit_code(report)


def command_agent_status(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    team = load_team(workspace)
    multiplexer = runtime_multiplexer(team)
    agents = rest or team_agents(team)
    states = read_agent_states(workspace)
    state_by_id = {str(item.get("id")): item for item in states if isinstance(item, dict) and item.get("id")}
    for agent in agents:
        require_agent(team, agent)
        runtime, pane = agent_runtime_status(team, agent)
        state = state_by_id.get(agent, {})
        pane_text = f" pane={pane}" if pane else ""
        print(f"{agent_label(team, agent)}: {multiplexer}={runtime}{pane_text} state={state.get('status', 'unknown')}")
    return 0


def command_agent_send(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 2:
        fail("agent send requires agent and message")
    args.items = [str(workspace), rest[0], " ".join(rest[1:])]
    return command_send(args)


def command_agent_start(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if not rest:
        fail("agent start requires an agent")
    team = load_team(workspace)
    started = 0
    for agent in rest:
        require_agent(team, agent)
    for agent in rest:
        if start_agent_window(workspace, team, agent):
            started += 1
    refresh_dashboard(workspace)
    info(f"Started {started} agent window(s)")
    return 0


def command_agent_stop(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if not rest:
        fail("agent stop requires an agent")
    team = load_team(workspace)
    stopped = 0
    for agent in rest:
        require_agent(team, agent)
    for agent in rest:
        if stop_agent_window(workspace, team, agent):
            stopped += 1
    refresh_dashboard(workspace)
    info(f"Stopped {stopped} agent window(s)")
    return 0


def command_agent_restart(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if not rest:
        fail("agent restart requires an agent")
    team = load_team(workspace)
    require_legacy_runtime(workspace)
    restarted = 0
    for agent in rest:
        require_agent(team, agent)
    for agent in rest:
        stop_agent_window(workspace, team, agent)
        if start_agent_window(workspace, team, agent):
            restarted += 1
    refresh_dashboard(workspace)
    info(f"Restarted {restarted} agent window(s)")
    return 0


def command_approve(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    checkpoint = rest[0] if rest else "plan-approval"
    team = load_team(workspace)
    message = args.message or f"Checkpoint `{checkpoint}` approved for its local content; no execution permission is implied."
    result = approve_checkpoint(workspace, team, checkpoint, message, args.agent,
                                 expected_revision=getattr(args, "expect_revision", None))
    return print_checkpoint_result(result, json_output=getattr(args, "json", False))


def command_suggest(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 2:
        fail("suggest requires checkpoint and change request text")
    checkpoint = rest[0]
    notes = " ".join(rest[1:])
    team = load_team(workspace)
    result = suggest_checkpoint_changes(workspace, team, checkpoint, notes, args.agent,
                                        expected_revision=getattr(args, "expect_revision", None))
    return print_checkpoint_result(result, json_output=getattr(args, "json", False))


def display_checkpoint(workspace: Path, checkpoint: str, view: checkpoints.View) -> None:
    print("\n" + "=" * 80)
    print(f"Review checkpoint: {checkpoint}")
    print(f"File: {checkpoint_path(workspace, checkpoint)}")
    print(f"Snapshot: {view.revision}")
    print(f"Effective status: {view.effective_status}; binding={view.binding}")
    if view.reason:
        print(f"Reason: {view.reason}")
    print("=" * 80)
    print(view.content if view.content is not None else "Checkpoint does not exist; a decision will create a default local checkpoint.")


def command_review(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    team = load_team(workspace)
    checkpoints = rest or checkpoint_review_candidates(workspace)
    # Reject a bad batch before prompting or applying any earlier decision.
    for checkpoint in checkpoints:
        checkpoint_view(workspace, checkpoint)
    if args.agent != "all":
        require_agent(team, args.agent)
    if not checkpoints:
        print("No pending checkpoint files found.")
        return 0

    exit_status = 0
    for checkpoint in checkpoints:
        view = checkpoint_view(workspace, checkpoint)
        display_checkpoint(workspace, checkpoint, view)

        while True:
            try:
                action = input("[a]pprove, [s]uggest changes, [n]ext, [q]uit > ").strip().lower()
            except EOFError:
                print()
                return exit_status
            if action in {"a", "approve"}:
                message = args.message or f"Checkpoint `{checkpoint}` approved after reviewing the displayed local snapshot."
                try:
                    result = approve_checkpoint(workspace, load_team(workspace), checkpoint, message, args.agent,
                                                 expected_revision=view.revision, source="review")
                    exit_status = max(exit_status, print_checkpoint_result(result))
                except storage.RevisionConflict as exc:
                    print(f"[WARN] {exc}. Reload and review explicitly; decision skipped.", file=sys.stderr)
                    exit_status = max(exit_status, 1)
                break
            if action in {"s", "suggest", "changes", "change"}:
                notes = prompt_multiline("Enter requested changes for the agents.")
                if notes:
                    try:
                        result = suggest_checkpoint_changes(workspace, load_team(workspace), checkpoint, notes, args.agent,
                                                             expected_revision=view.revision, source="review")
                        exit_status = max(exit_status, print_checkpoint_result(result))
                    except storage.RevisionConflict as exc:
                        print(f"[WARN] {exc}. Reload and review explicitly; decision skipped.", file=sys.stderr)
                        exit_status = max(exit_status, 1)
                else:
                    info("No change request saved")
                break
            if action in {"n", "next", "skip"}:
                break
            if action in {"q", "quit", "exit"}:
                return exit_status
            print("Choose approve, suggest changes, next, or quit.")
    return exit_status


def print_console_help() -> None:
    print("""Commands:
  help                         Show this help
  standup                      Show Scrum-style coven summary
  status                       Show dashboard
  next                         Show recommended next action
  agents                       Show agent runtime status
  send <agent|all> <message>   Send a multiplexer-backed message
  approve [checkpoint]         Approve a checkpoint (default: plan-approval)
  suggest <checkpoint> <text>  Save/send requested checkpoint changes
  review [checkpoint]          Interactive checkpoint review
  lead [agent|status|clear]    Set/show/clear the coven lead
  quit                         Exit

Shortcut: `<agent|all> <message>` also sends a message.
""")


def command_console(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    team = load_team(workspace)
    print(f"coven console: {team['name']} ({workspace})")
    print_console_help()
    exit_status = 0
    while True:
        try:
            raw = input("coven> ").strip()
        except EOFError:
            print()
            return exit_status
        if not raw:
            continue
        if raw in {"q", "quit", "exit"}:
            return exit_status
        if raw in {"h", "help", "?"}:
            print_console_help()
            continue
        if raw == "standup":
            print(render_standup(workspace))
            continue
        if raw == "status":
            refresh_dashboard(workspace)
            print_file(workspace / "dashboard.md", "No dashboard yet\n")
            continue
        if raw == "next":
            command_next(argparse.Namespace(workspace=str(workspace)))
            continue
        if raw in {"agents", "agent status"}:
            command_agent_status(argparse.Namespace(items=[str(workspace)]))
            continue
        parts = raw.split(maxsplit=2)
        command = parts[0]
        if command == "review":
            items = [str(workspace)] + ([parts[1]] if len(parts) > 1 else [])
            exit_status = max(exit_status, command_review(argparse.Namespace(items=items, agent="all", message=None)))
            continue
        if command == "lead":
            items = [str(workspace)] + raw.split()[1:]
            command_lead(argparse.Namespace(items=items, message=None))
            continue
        if command == "approve":
            checkpoint = parts[1] if len(parts) > 1 else "plan-approval"
            exit_status = max(exit_status, console_checkpoint_decision(workspace, checkpoint, "approved", "Approved displayed checkpoint content from console."))
            continue
        if command == "suggest":
            if len(parts) < 3:
                print("Usage: suggest <checkpoint> <requested changes>")
                continue
            exit_status = max(exit_status, console_checkpoint_decision(workspace, parts[1], "changes_requested", parts[2]))
            continue
        if command == "send":
            if len(parts) < 3:
                print("Usage: send <agent|all> <message>")
                continue
            command_send(argparse.Namespace(items=[str(workspace), parts[1], parts[2]]))
            continue

        agents = set(team_agents(team)) | {"all"}
        if command in agents and len(parts) >= 2:
            recipient = command
            message = raw.split(maxsplit=1)[1]
            command_send(argparse.Namespace(items=[str(workspace), recipient, message]))
            continue
        print("Unknown command. Type `help`.")
    return exit_status


def console_checkpoint_decision(workspace: Path, checkpoint: str, action: str, notes: str) -> int:
    view = checkpoint_view(workspace, checkpoint)
    display_checkpoint(workspace, checkpoint, view)
    try:
        confirmed = input(f"Record {action} for this displayed snapshot? [y/N] > ").strip().lower()
    except EOFError:
        return 0
    if confirmed not in {"y", "yes"}:
        info("No checkpoint decision saved")
        return 0
    try:
        result = decide_checkpoint(workspace, load_team(workspace), checkpoint, action, notes,
                                    expected_revision=view.revision, source="console")
        return print_checkpoint_result(result)
    except storage.RevisionConflict as exc:
        print(f"[WARN] {exc}. Reinspect and confirm explicitly; no retry was made.", file=sys.stderr)
        return 1


def command_refresh(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    # Discard this draft: deterministic bad inputs must not create a legacy
    # workspace's first lock. Actual outputs are prepared again under the lock.
    prepare_refresh(workspace)
    with workspace_mutation(workspace) as held:
        return refresh_workspace(workspace, held)


def prepare_refresh(workspace: Path) -> tuple[str, dict[str, str], dict[str, str], str, str]:
    team = load_team(workspace)
    agents = team_agents(team)
    opencode_command = str(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)
    multiplexer = runtime_multiplexer(team)
    example = load_example(str(team.get("example"))) if team.get("example") else None

    # Log metadata remains validated, but history never repairs current decisions.
    # Stage generated outputs before writing; state, guidance, and logs are read-only.
    checkpoint_requests(workspace)
    try:
        manifest = tmux_manifest(team["name"], workspace, agents, opencode_command)
        tomllib.loads(manifest)
        example_context = read_example_text(example, "prompt-context.md")
        prompt_files = {
            f"prompts/{agent}.start.txt": starter_prompt_text(agent, workspace) for agent in agents
        }
        missing_prompts = {
            f"prompts/{agent}.md": prompt_text(agent, workspace, example_context)
            for agent in agents if not managed_path(workspace, f"prompts/{agent}.md").exists()
        }
        dashboard = render_dashboard(workspace)
    except (ValueError, OSError) as exc:
        fail(f"Cannot prepare refresh: {exc}")
    return manifest, missing_prompts, prompt_files, dashboard, multiplexer


def refresh_workspace(workspace: Path, lock: storage.WorkspaceLock) -> int:
    # Validate/reuse the token; no runtime actions or interactive input inside.
    with workspace_mutation(workspace, lock):
        manifest, missing_prompts, prompt_files, dashboard, multiplexer = prepare_refresh(workspace)
        manifest_path = managed_path(workspace, "tmux/coven.toml")
        write_if_missing(manifest_path, manifest, force=True)
        for relative, content in missing_prompts.items():
            write_if_missing(managed_path(workspace, relative), content, force=False)
        for relative, content in prompt_files.items():
            write_if_missing(managed_path(workspace, relative), content, force=True)
        storage.atomic_replace_bytes(managed_path(workspace, "dashboard.md"), dashboard.encode("utf-8"))
        info(f"Regenerated tmux/coven.toml and starter prompt files (runtime={multiplexer})")
    return 0


def team_session_name(team: dict[str, Any]) -> str:
    return f"coven-{slugify(str(team['name']))}"


def command_bootstrap(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    team = load_team(workspace)
    require_legacy_runtime(workspace)
    team_session_name(team)
    for agent in team_agents(team):
        managed_text(workspace, f"prompts/{agent}.start.txt")
    with legacy_runtime_transport(workspace):
        return _command_bootstrap_legacy(args)


def _command_bootstrap_legacy(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    team = load_team(workspace)
    require_legacy_runtime(workspace)
    agents = team_agents(team)

    session = team_session_name(team)
    multiplexer = runtime_multiplexer(team)
    sent = 0
    missing: list[str] = []
    prompts = []
    for agent in agents:
        prompt_file = managed_path(workspace, f"prompts/{agent}.start.txt")
        text = managed_text(workspace, f"prompts/{agent}.start.txt")
        if text is None:
            text = starter_prompt_text(agent, workspace)
        prompts.append((agent, prompt_file, text))
    for agent, prompt_file, text in prompts:
        write_if_missing(prompt_file, text, force=False)
        if multiplexer == "herdr":
            target = running_agent_pane(team, agent)
            if not target:
                missing.append(agent)
                continue
            subprocess.run(["herdr", "pane", "send-text", target, text], check=False)
            subprocess.run(["herdr", "pane", "send-keys", target, "enter"], check=False)
            append_event(workspace, "agent.bootstrap.sent", agent=agent, target=target, multiplexer="herdr")
            sent += 1
            continue
        window_target = f"{session}:agent-{agent}"
        pane_lookup = subprocess.run(
            ["tmux", "list-panes", "-t", window_target, "-F", "#{pane_id}"],
            check=False,
            capture_output=True,
            text=True,
        )
        if pane_lookup.returncode != 0 or not pane_lookup.stdout.strip():
            missing.append(agent)
            continue
        target = pane_lookup.stdout.strip().splitlines()[0]
        buffer_name = f"{session}-{agent}-starter"
        subprocess.run(["tmux", "load-buffer", "-b", buffer_name, str(prompt_file)], check=False)
        subprocess.run(["tmux", "paste-buffer", "-b", buffer_name, "-t", target], check=False)
        subprocess.run(["tmux", "send-keys", "-t", target, "Enter"], check=False)
        append_event(workspace, "agent.bootstrap.sent", agent=agent, target=target)
        sent += 1

    if missing:
        info(f"No running agent pane found for: {', '.join(missing)}")
        info("If the coven session is partial or stale, run: coven up --replace <workspace>")
    info(f"Bootstrap prompts sent: {sent}")
    return 0


def tmux_session_exists(session: str) -> bool:
    result = subprocess.run(["tmux", "has-session", "-t", session], check=False, capture_output=True, text=True)
    return result.returncode == 0


def run_tmux(args: list[str], *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["tmux", *args], check=False, capture_output=capture, text=True)


def fail_tmux(result: subprocess.CompletedProcess[str], message: str) -> None:
    if result.returncode != 0:
        fail(result.stderr.strip() or message)


def focus_tmux_session(session: str) -> int:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        info(f"Coven tmux session ready: {session}")
        return 0
    if os.environ.get("TMUX"):
        return run_tmux(["switch-client", "-t", session]).returncode
    return run_tmux(["attach-session", "-t", session]).returncode


def focus_herdr_workspace(workspace_id: str, label: str) -> int:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        info(f"Coven Herdr workspace ready: {label} ({workspace_id})")
        return 0
    result = run_herdr(["workspace", "focus", workspace_id], capture=True)
    if result.returncode != 0:
        fail(result.stderr.strip() or f"Failed to focus Herdr workspace `{label}`")
    return 0


def print_tmux_plan(workspace: Path, team: dict[str, Any]) -> int:
    agents = team_agents(team)
    session = team_session_name(team)
    manifest = workspace / "tmux" / "coven.toml"
    if not manifest.exists():
        fail(f"Missing tmux manifest: {manifest}")
    print(f"Coven tmux plan: {session}")
    print(f"Workspace: {workspace}")
    print(f"Manifest: {manifest}")
    print("Windows:")
    print("- orchestrator: dashboard, events, messages, shell")
    for agent in agents:
        print(f"- agent-{agent}: {agent}")
    return 0


def print_herdr_plan(workspace: Path, team: dict[str, Any]) -> int:
    agents = team_agents(team)
    label = herdr_workspace_label(team)
    print(f"Coven Herdr plan: {label}")
    print(f"Workspace: {workspace}")
    print("Tabs:")
    print("- orchestrator: dashboard, events, messages, shell panes")
    for agent in agents:
        print(f"- agent-{agent}: {agent}")
    return 0


@legacy_transport_effect
def create_coven_tmux_session(workspace: Path, team: dict[str, Any]) -> None:
    preflight_workspace_paths(workspace, team)
    require_legacy_runtime(workspace)
    agents = team_agents(team)
    session = team_session_name(team)
    opencode_command = str(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)
    cli = cli_command()

    dashboard_command = "while true; do clear; cat dashboard.md; sleep 2; done"
    result = run_tmux(["new-session", "-d", "-s", session, "-n", "orchestrator", "-c", str(workspace), dashboard_command], capture=True)
    fail_tmux(result, f"Failed to create coven tmux session `{session}`")
    run_tmux(["select-pane", "-t", f"{session}:orchestrator.0", "-T", "dashboard"])

    monitor_panes = [
        ("events", f"while true; do clear; {cli} events; sleep 2; done"),
        ("messages", f"while true; do clear; {cli} messages; sleep 2; done"),
        ("shell", "printf 'coven workspace: '; pwd; exec /bin/zsh"),
    ]
    for title, command in monitor_panes:
        pane = run_tmux(["split-window", "-t", f"{session}:orchestrator", "-c", str(workspace), "-P", "-F", "#{pane_id}", command], capture=True)
        fail_tmux(pane, f"Failed to create `{title}` pane")
        pane_id = pane.stdout.strip()
        if pane_id:
            run_tmux(["select-pane", "-t", pane_id, "-T", title])
    run_tmux(["select-layout", "-t", f"{session}:orchestrator", "tiled"])

    for agent in agents:
        command = agent_launch_command(str(team["name"]), workspace, agent, opencode_command)
        result = run_tmux(["new-window", "-t", session, "-n", f"agent-{agent}", "-c", str(workspace), command], capture=True)
        fail_tmux(result, f"Failed to create agent window `{agent}`")

    run_tmux(["select-window", "-t", f"{session}:orchestrator"])


@legacy_transport_effect
def create_coven_herdr_workspace(workspace: Path, team: dict[str, Any]) -> str:
    preflight_workspace_paths(workspace, team)
    require_legacy_runtime(workspace)
    agents = team_agents(team)
    label = herdr_workspace_label(team)
    opencode_command = str(team.get("opencode_command") or DEFAULT_OPENCODE_COMMAND)
    cli = cli_command()

    result = run_herdr(["workspace", "create", "--cwd", str(workspace), "--label", label, "--no-focus"], capture=True)
    payload = herdr_result(result, f"Failed to create Coven Herdr workspace `{label}`")
    workspace_id = str(payload.get("workspace", {}).get("workspace_id") or "")
    root_pane = str(payload.get("root_pane", {}).get("pane_id") or "")
    root_tab = str(payload.get("tab", {}).get("tab_id") or "")
    if not workspace_id or not root_pane:
        fail(f"Could not resolve Herdr workspace/pane ids for `{label}`")

    if root_tab:
        run_herdr(["tab", "rename", root_tab, "orchestrator"], capture=True)
    run_herdr(["pane", "rename", root_pane, "dashboard"], capture=True)
    run_herdr(["pane", "run", root_pane, "while true; do clear; cat dashboard.md; sleep 2; done"], capture=True)

    monitor_panes = [
        ("events", f"while true; do clear; {cli} events; sleep 2; done"),
        ("messages", f"while true; do clear; {cli} messages; sleep 2; done"),
        ("shell", "printf 'coven workspace: '; pwd; exec /bin/zsh"),
    ]
    for title, command in monitor_panes:
        pane_result = run_herdr(
            ["pane", "split", "--pane", root_pane, "--direction", "right", "--ratio", "0.5", "--cwd", str(workspace), "--no-focus"],
            capture=True,
        )
        pane_payload = herdr_result(pane_result, f"Failed to create Herdr `{title}` pane")
        pane_id = str(pane_payload.get("pane", {}).get("pane_id") or "")
        if pane_id:
            run_herdr(["pane", "rename", pane_id, title], capture=True)
            run_herdr(["pane", "run", pane_id, command], capture=True)

    for agent in agents:
        command = agent_launch_command(str(team["name"]), workspace, agent, opencode_command, "herdr")
        tab_result = run_herdr(
            ["tab", "create", "--workspace", workspace_id, "--cwd", str(workspace), "--label", f"agent-{agent}", "--no-focus"],
            capture=True,
        )
        tab_payload = herdr_result(tab_result, f"Failed to create Herdr agent tab `{agent}`")
        pane_id = str(tab_payload.get("root_pane", {}).get("pane_id") or "")
        if pane_id:
            run_herdr(["pane", "rename", pane_id, agent], capture=True)
            run_herdr(["pane", "run", pane_id, command], capture=True)

    if root_tab:
        run_herdr(["tab", "focus", root_tab], capture=True)
    return workspace_id


def run_coven_tmux(workspace: Path, team: dict[str, Any], action: str) -> int:
    preflight_workspace_paths(workspace, team)
    if actions.ActionStore(workspace).read_control() is not None:
        return stop_current_action(workspace) if action == "down" else reject_legacy_runtime()
    with legacy_runtime_transport(workspace):
        return _run_coven_tmux_legacy(workspace, team, action)


def _run_coven_tmux_legacy(workspace: Path, team: dict[str, Any], action: str) -> int:
    session = team_session_name(team)
    if action == "plan":
        return print_tmux_plan(workspace, team)
    if action == "down":
        if not tmux_session_exists(session):
            info(f"Coven tmux session is not running: {session}")
            return 0
        return run_tmux(["kill-session", "-t", session]).returncode
    if action == "restart":
        if tmux_session_exists(session):
            result = run_tmux(["kill-session", "-t", session], capture=True)
            fail_tmux(result, f"Failed to stop coven tmux session `{session}`")
        create_coven_tmux_session(workspace, team)
        return focus_tmux_session(session)
    if action == "up":
        if not tmux_session_exists(session):
            create_coven_tmux_session(workspace, team)
        return focus_tmux_session(session)
    fail(f"Unsupported tmux action: {action}")


def run_coven_herdr(workspace: Path, team: dict[str, Any], action: str) -> int:
    preflight_workspace_paths(workspace, team)
    if actions.ActionStore(workspace).read_control() is not None:
        return stop_current_action(workspace) if action == "down" else reject_legacy_runtime()
    with legacy_runtime_transport(workspace):
        return _run_coven_herdr_legacy(workspace, team, action)


def _run_coven_herdr_legacy(workspace: Path, team: dict[str, Any], action: str) -> int:
    label = herdr_workspace_label(team)
    if action == "plan":
        return print_herdr_plan(workspace, team)
    if action == "down":
        workspace_info = find_herdr_workspace(team)
        if not workspace_info:
            info(f"Coven Herdr workspace is not running: {label}")
            return 0
        workspace_id = str(workspace_info.get("workspace_id") or "")
        if workspace_id:
            result = run_herdr(["workspace", "close", workspace_id], capture=True)
            if result.returncode != 0:
                fail(result.stderr.strip() or f"Failed to close Coven Herdr workspace `{label}`")
        return 0
    if action == "restart":
        workspace_info = find_herdr_workspace(team)
        if workspace_info and workspace_info.get("workspace_id"):
            result = run_herdr(["workspace", "close", str(workspace_info["workspace_id"])], capture=True)
            if result.returncode != 0:
                fail(result.stderr.strip() or f"Failed to close Coven Herdr workspace `{label}`")
        workspace_id = create_coven_herdr_workspace(workspace, team)
        return focus_herdr_workspace(workspace_id, label)
    if action == "up":
        workspace_info = find_herdr_workspace(team)
        workspace_id = str(workspace_info.get("workspace_id")) if workspace_info else create_coven_herdr_workspace(workspace, team)
        return focus_herdr_workspace(workspace_id, label)
    fail(f"Unsupported Herdr action: {action}")


def run_coven_runtime(workspace: Path, team: dict[str, Any], action: str) -> int:
    if actions.ActionStore(workspace).read_control() is not None:
        return stop_current_action(workspace) if action == "down" else reject_legacy_runtime()
    multiplexer = runtime_multiplexer(team)
    if multiplexer == "herdr":
        return run_coven_herdr(workspace, team, action)
    return run_coven_tmux(workspace, team, action)


def command_tmux(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    if actions.ActionStore(workspace).read_control() is not None:
        return stop_current_action(workspace) if args.action == "down" else reject_legacy_runtime()
    team = load_team(workspace)
    action = "restart" if args.action == "up" and getattr(args, "replace", False) else args.action
    multiplexer = runtime_multiplexer(team)
    append_event(workspace, f"{multiplexer}.{action}.requested")
    return run_coven_runtime(workspace, team, action)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Manage multi-agent coven workspaces")
    subparsers = parser.add_subparsers(dest="command", required=True)

    init = subparsers.add_parser("init", help="Create a coven workspace")
    init.add_argument("workspace", nargs="?", default=".")
    init.add_argument("--name")
    init.add_argument("--agents", default=DEFAULT_AGENTS)
    init.add_argument("--opencode-command", default=DEFAULT_OPENCODE_COMMAND)
    init.add_argument("--multiplexer", choices=sorted(SUPPORTED_MULTIPLEXERS), default=DEFAULT_MULTIPLEXER, help=f"Runtime multiplexer to use (default: {DEFAULT_MULTIPLEXER})")
    init.add_argument("--example", default=DEFAULT_EXAMPLE_ID, help=f"Example seed to use from examples/ (default: {DEFAULT_EXAMPLE_ID})")
    init.add_argument("--blank", action="store_true", help="Create structure without an example seed")
    init.add_argument("--force", action="store_true", help="Deprecated compatibility flag; never overwrites existing workspace files")
    init.set_defaults(func=command_init)

    goal = subparsers.add_parser("goal", help="Set or show the unified goal")
    goal.add_argument("items", nargs="*", help="Optional workspace followed by goal text; defaults to current directory")
    goal.set_defaults(func=command_goal)

    status = subparsers.add_parser("status", help="Render and print dashboard")
    status.add_argument("workspace", nargs="?", default=".")
    status.set_defaults(func=command_status)

    events = subparsers.add_parser("events", help="Print append-only event log")
    events.add_argument("workspace", nargs="?", default=".")
    events.add_argument("--pretty", action="store_true", help="Accepted for compatibility; pretty output is now the default")
    events.add_argument("--raw", action="store_true", help="Print raw JSONL")
    events.set_defaults(func=command_events)

    messages = subparsers.add_parser("messages", help="Print append-only message log")
    messages.add_argument("workspace", nargs="?", default=".")
    messages.add_argument("--pretty", action="store_true", help="Accepted for compatibility; pretty output is now the default")
    messages.add_argument("--raw", action="store_true", help="Print raw JSONL")
    messages.set_defaults(func=command_messages)

    standup = subparsers.add_parser("standup", help="Print a Scrum-style coven standup summary")
    standup.add_argument("workspace", nargs="?", default=".")
    standup.set_defaults(func=command_standup)

    next_cmd = subparsers.add_parser("next", help="Show the recommended next user action")
    next_cmd.add_argument("workspace", nargs="?", default=".")
    next_cmd.set_defaults(func=command_next)

    send = subparsers.add_parser("send", help="Send a message to an agent pane through the configured multiplexer")
    send.add_argument("items", nargs="+", help="Optional workspace, then agent id (or 'all') and message")
    send.set_defaults(func=command_send)

    approve = subparsers.add_parser("approve", help="Approve a checkpoint and notify agents")
    approve.add_argument("items", nargs="*", help="Optional workspace and optional checkpoint (default: plan-approval)")
    approve.add_argument("--agent", default="all", help="Agent id to notify, or 'all' (default)")
    approve.add_argument("--message", help="Custom approval message")
    approve.add_argument("--expect-revision", help="Apply only to the exact checkpoint snapshot token inspected")
    approve.add_argument("--json", action="store_true", help="Return decision identity, revisions, outcome, and delivery warnings")
    approve.set_defaults(func=command_approve)

    suggest = subparsers.add_parser("suggest", help="Request changes for a checkpoint and notify agents")
    suggest.add_argument("items", nargs="+", help="Optional workspace, then checkpoint and change request text")
    suggest.add_argument("--agent", default="all", help="Agent id to notify, or 'all' (default)")
    suggest.add_argument("--expect-revision", help="Apply only to the exact checkpoint snapshot token inspected")
    suggest.add_argument("--json", action="store_true", help="Return decision identity, revisions, outcome, and delivery warnings")
    suggest.set_defaults(func=command_suggest)

    checkpoint = subparsers.add_parser("checkpoint", help="Inspect checkpoint-local scope and exact snapshot revisions")
    checkpoint_sub = checkpoint.add_subparsers(dest="checkpoint_command", required=True)
    checkpoint_inspect = checkpoint_sub.add_parser("inspect", help="Read-only checkpoint content, binding, and revision diagnostics")
    checkpoint_inspect.add_argument("items", nargs="*", help="Optional workspace and checkpoint (default: plan-approval)")
    checkpoint_inspect.add_argument("--json", action="store_true")
    checkpoint_inspect.set_defaults(func=command_checkpoint_inspect)

    review = subparsers.add_parser("review", help="Interactively review checkpoint files and approve or request changes")
    review.add_argument("items", nargs="*", help="Optional workspace and optional checkpoint(s)")
    review.add_argument("--agent", default="all", help="Agent id to notify, or 'all' (default)")
    review.add_argument("--message", help="Custom approval message")
    review.set_defaults(func=command_review)

    console = subparsers.add_parser("console", help="Interactive console for reviewing and sending multiplexer-backed agent commands")
    console.add_argument("workspace", nargs="?", default=".")
    console.set_defaults(func=command_console)

    lead = subparsers.add_parser("lead", help="Set/show/clear the coven lead")
    lead.add_argument("items", nargs="*", help="Optional workspace, then agent, status, or clear")
    lead.add_argument("--clear", action="store_true", help="Clear the current coven lead")
    lead.add_argument("--message", help="Custom coven lead prompt")
    lead.set_defaults(func=command_lead)

    refresh = subparsers.add_parser("refresh", help="Regenerate generated prompts and runtime manifests without resetting state")
    refresh.add_argument("workspace", nargs="?", default=".")
    refresh.set_defaults(func=command_refresh)

    bootstrap = subparsers.add_parser("bootstrap", help="Paste starter prompts into running agent panes")
    bootstrap.add_argument("workspace", nargs="?", default=".")
    bootstrap.set_defaults(func=command_bootstrap)

    message = subparsers.add_parser("message", help="Append a message between agents")
    message.add_argument("items", nargs="+", help="Optional workspace, then sender recipient body")
    message.set_defaults(func=command_message)

    session = subparsers.add_parser("session", help="Read-only target inspection and explicit interactive UI submission; not worker dispatch")
    session_sub = session.add_subparsers(dest="session_operation", required=True)
    for operation in ("list", "inspect"):
        inspection = session_sub.add_parser(operation, help="Read-only generic pane/process evidence and declared group/role associations")
        inspection.add_argument("workspace", help="Existing Coven workspace for read-only group declarations")
        inspection.add_argument("--socket", required=True, help="Explicit existing local Kitsune API socket")
        inspection.add_argument("--runtime", choices=["djinn"], default="djinn", help="Process-family interpreter, not a native execution request")
        if operation == "inspect":
            inspection.add_argument("--pane", required=True, help="Exact current pane ID")
            inspection.add_argument("--expected-terminal", help="Optional terminal pin; never follows a replacement")
        inspection.add_argument("--timeout", type=float, default=3.0, help="Shared API/filesystem acceptance budget in seconds (0, 30]")
        inspection.add_argument("--max-response-bytes", type=int, default=1048576, help="Shared API wire-byte allowance (1024 through 8388608)")
        inspection.add_argument("--json", action="store_true")
        inspection.set_defaults(func=command_session_inspect)
    for operation in ("start", "resume"):
        interactive = session_sub.add_parser(operation, help=f"Preview an interactive {operation}; --execute submits once")
        interactive.add_argument("workspace", help="Existing Coven control workspace, not a cwd or sandbox")
        interactive.add_argument("--runtime", choices=["djinn"], required=True)
        interactive.add_argument("--executable", required=True, help="Absolute trusted native CLI; never executed during preview")
        interactive.add_argument("--socket", required=True, help="Explicit existing local Kitsune API socket")
        interactive.add_argument("--pane", required=True, help="Exact current public pane ID")
        interactive.add_argument("--expected-terminal", required=True, help="Terminal ID copied from snapshot")
        if operation == "resume":
            interactive.add_argument("--session", required=True, help="Literal native UI/folder/name selector, not verified identity")
        interactive.add_argument("--execute", action="store_true", help="Explicit unrestricted UI submission; requires a quiescent empty shell prompt")
        interactive.add_argument("--timeout", type=float, default=3.0, help="API acceptance budget in seconds (0, 30]; gate acquisition is separately bounded")
        interactive.add_argument("--json", action="store_true")
        interactive.set_defaults(func=command_session)

    agent = subparsers.add_parser("agent", help="Target specific coven agents")
    agent_sub = agent.add_subparsers(dest="agent_command", required=True)
    agent_status = agent_sub.add_parser("status", help="Show one agent's runtime/status, or all agents")
    agent_status.add_argument("items", nargs="*", help="Optional workspace and optional agent(s)")
    agent_status.set_defaults(func=command_agent_status)
    agent_send = agent_sub.add_parser("send", help="Send a message to one agent")
    agent_send.add_argument("items", nargs="+", help="Optional workspace, then agent and message")
    agent_send.set_defaults(func=command_agent_send)
    agent_start = agent_sub.add_parser("start", help="Start one or more agent windows")
    agent_start.add_argument("items", nargs="+", help="Optional workspace, then agent(s)")
    agent_start.set_defaults(func=command_agent_start)
    agent_stop = agent_sub.add_parser("stop", help="Stop one or more agent windows")
    agent_stop.add_argument("items", nargs="+", help="Optional workspace, then agent(s)")
    agent_stop.set_defaults(func=command_agent_stop)
    agent_restart = agent_sub.add_parser("restart", help="Restart one or more agent windows")
    agent_restart.add_argument("items", nargs="+", help="Optional workspace, then agent(s)")
    agent_restart.set_defaults(func=command_agent_restart)

    task = subparsers.add_parser("task", help="Manage task state")
    task_sub = task.add_subparsers(dest="task_command", required=True)
    task_add = task_sub.add_parser("add")
    task_add.add_argument("items", nargs="+", help="Optional workspace followed by task title")
    task_add.add_argument("--owner")
    task_add.add_argument("--notes")
    task_add.add_argument("--expect-revision", help="Reject unless the current whole-task snapshot matches this token")
    task_add.add_argument("--request-id", help="Durable request key, required only after explicit task-ledger import")
    task_add.add_argument("--json", action="store_true", help="Return mutation outcome, task, revisions, and warnings")
    task_add.set_defaults(func=command_task_add)
    task_assign = task_sub.add_parser("assign")
    task_assign.add_argument("items", nargs="+", help="Optional workspace followed by task_id and agent")
    task_assign.add_argument("--expect-revision", help="Reject unless the current whole-task snapshot matches this token")
    task_assign.add_argument("--request-id", help="Durable request key, required only after explicit task-ledger import")
    task_assign.add_argument("--json", action="store_true", help="Return mutation outcome, task, revisions, and warnings")
    task_assign.set_defaults(func=command_task_assign)
    task_list = task_sub.add_parser("list")
    task_list.add_argument("workspace", nargs="?", default=".")
    task_list.add_argument("--json", action="store_true", help="Return tasks and an exact content revision for conditional mutations")
    task_list.set_defaults(func=command_task_list)

    def execution_request_flags(parser, *, tasks_revision=False):
        parser.add_argument("--expect-execution-revision", required=True, help="Exact execution-control revision, or execution-disabled:v1 for enable")
        if tasks_revision:
            parser.add_argument("--expect-tasks-revision", required=True, help="Exact authoritative task revision inspected")
        parser.add_argument("--request-id", required=True, help="Durable execution-authority request key; reuse only the original exact binding")
        parser.add_argument("--json", action="store_true")

    attempt = task_sub.add_parser("attempt", help="Prepare/cancel declarative local intent; never dispatches a worker")
    attempt_sub = attempt.add_subparsers(dest="attempt_command", required=True)
    attempt_prepare = attempt_sub.add_parser("prepare", help="Freeze one enrolled task/owner/intent snapshot without execution permission")
    attempt_prepare.add_argument("workspace", nargs="?", default=".")
    attempt_prepare.add_argument("task_id")
    preparation = attempt_prepare.add_mutually_exclusive_group(required=True)
    preparation.add_argument("--intent", help="Operator-selected bounded JSON task-intent document; unmanaged tasks only in v2")
    preparation.add_argument("--graph-revision", help="Derive intent from this exact current approved v2 graph revision")
    execution_request_flags(attempt_prepare, tasks_revision=True)
    attempt_prepare.set_defaults(func=command_attempt_prepare)
    attempt_cancel = attempt_sub.add_parser("cancel", help="Cancel a local preparation before submission; not native cancellation")
    attempt_cancel.add_argument("workspace", nargs="?", default=".")
    attempt_cancel.add_argument("attempt_id")
    attempt_cancel.add_argument("--reason", required=True)
    execution_request_flags(attempt_cancel)
    attempt_cancel.set_defaults(func=command_attempt_cancel)
    attempt_list = attempt_sub.add_parser("list", help="Read preparations/transitions and derived source freshness")
    attempt_list.add_argument("workspace", nargs="?", default=".")
    attempt_list.add_argument("--task")
    attempt_list.add_argument("--json", action="store_true")
    attempt_list.set_defaults(func=command_attempt_list)
    attempt_inspect = attempt_sub.add_parser("inspect", help="Read an immutable preparation and its current local state")
    attempt_inspect.add_argument("workspace", nargs="?", default=".")
    attempt_inspect.add_argument("attempt_id")
    attempt_inspect.add_argument("--json", action="store_true")
    attempt_inspect.set_defaults(func=command_attempt_inspect)

    graph = task_sub.add_parser("graph", help="Reviewed immutable DAG criteria, never a scheduler or execution grant")
    graph_sub = graph.add_subparsers(dest="graph_command", required=True)
    graph_propose = graph_sub.add_parser("propose", help="Freeze a DAG from explicitly enrolled current tasks")
    graph_propose.add_argument("workspace", nargs="?", default=".")
    graph_propose.add_argument("--document", required=True)
    execution_request_flags(graph_propose, tasks_revision=True)
    graph_propose.set_defaults(func=command_graph_propose)
    graph_decide = graph_sub.add_parser("decide", help="Record planning review for the current immutable graph")
    graph_decide.add_argument("workspace", nargs="?", default=".")
    graph_decide.add_argument("graph_revision")
    graph_decide.add_argument("--decision", choices=["approve", "changes-requested"], required=True)
    graph_decide.add_argument("--reason", required=True)
    graph_decide.add_argument("--expect-graph-digest", required=True)
    graph_decide.add_argument("--expect-tasks-revision", help="Required for approve; omitted for source-independent changes-requested")
    execution_request_flags(graph_decide)
    graph_decide.set_defaults(func=command_graph_decide)
    graph_inspect = graph_sub.add_parser("inspect", help="Read planning review, immutable criteria, topology, and source drift")
    graph_inspect.add_argument("items", nargs="*", help="Optional Coven workspace and graph revision; current revision by default")
    graph_inspect.add_argument("--json", action="store_true")
    graph_inspect.set_defaults(func=command_graph_inspect)
    graph_export = graph_sub.add_parser("export", help="Read the frozen manifest, not current mutable source")
    graph_export.add_argument("workspace", nargs="?", default=".")
    graph_export.add_argument("graph_revision")
    graph_export.add_argument("--json", action="store_true")
    graph_export.set_defaults(func=command_graph_export)

    execution = subparsers.add_parser("execution", help="Explicit preparation-only task enrollment authority; no native execution")
    execution_sub = execution.add_subparsers(dest="execution_command", required=True)
    execution_inspect = execution_sub.add_parser("inspect", help="Read execution authority and coordinated source diagnostics without creating a lock")
    execution_inspect.add_argument("workspace", nargs="?", default=".")
    execution_inspect.add_argument("--json", action="store_true")
    execution_inspect.set_defaults(func=command_execution_inspect)
    execution_enable = execution_sub.add_parser("enable", help="Explicitly initialize empty execution control; enrolls zero tasks")
    execution_enable.add_argument("workspace", nargs="?", default=".")
    execution_request_flags(execution_enable)
    execution_enable.set_defaults(func=command_execution_enable)
    execution_upgrade = execution_sub.add_parser("upgrade", help="Explicit backed-up v1-to-v2 graph support; requires no prepared attempts")
    execution_upgrade.add_argument("workspace", nargs="?", default=".")
    execution_request_flags(execution_upgrade)
    execution_upgrade.set_defaults(func=command_execution_upgrade)
    execution_enroll = execution_sub.add_parser("enroll", help="Enroll exactly one selected task source without changing the task")
    execution_enroll.add_argument("workspace", nargs="?", default=".")
    execution_enroll.add_argument("task_id")
    execution_enroll.add_argument("--rebind-source", action="store_true", help="Explicitly retain/rebind an existing enrollment after authority identity changes")
    execution_request_flags(execution_enroll, tasks_revision=True)
    execution_enroll.set_defaults(func=command_execution_enroll)
    execution_recover = execution_sub.add_parser("recover", help="Confirm current durability or preserve/archive only a missing-control activation")
    execution_recover.add_argument("workspace", nargs="?", default=".")
    execution_recovery = execution_recover.add_mutually_exclusive_group(required=True)
    execution_recovery.add_argument("--archive-pending", action="store_true")
    execution_recovery.add_argument("--confirm-authority", action="store_true")
    execution_recover.add_argument("--expect-execution-revision")
    execution_recover.add_argument("--json", action="store_true")
    execution_recover.set_defaults(func=command_execution_recover)

    state = subparsers.add_parser("state", help="Inspect, explicitly import, or recover task authority")
    state_sub = state.add_subparsers(dest="state_command", required=True)
    inspect = state_sub.add_parser("inspect", help="Read-only task authority and projection diagnostics")
    inspect.add_argument("workspace", nargs="?", default=".")
    inspect.add_argument("--json", action="store_true")
    inspect.set_defaults(func=command_state_inspect)
    import_tasks = state_sub.add_parser("import-tasks", help="Back up exact snapshot bytes and activate a task-only ledger")
    import_tasks.add_argument("workspace", nargs="?", default=".")
    import_tasks.add_argument("--expect-revision", required=True)
    import_tasks.add_argument("--json", action="store_true")
    import_tasks.set_defaults(func=command_state_import_tasks)
    recover = state_sub.add_parser("recover-tasks", help="Explicitly repair a projection or resume/abort an unfinished import")
    recover.add_argument("workspace", nargs="?", default=".")
    action = recover.add_mutually_exclusive_group(required=True)
    action.add_argument("--repair-projection", action="store_true")
    action.add_argument("--resume-import", action="store_true")
    action.add_argument("--abort-import", action="store_true")
    recover.add_argument("--expect-revision")
    recover.add_argument("--json", action="store_true")
    recover.set_defaults(func=command_state_recover_tasks)

    action = subparsers.add_parser("action", help="Opt-in reviewed, single-use unrestricted runtime startup")
    action_sub = action.add_subparsers(dest="action_command", required=True)
    enable = action_sub.add_parser("enable-runtime-launch", help="Explicitly disable legacy launch/delivery routes for this workspace")
    enable.add_argument("workspace", nargs="?", default=".")
    enable.add_argument("--json", action="store_true")
    enable.set_defaults(func=command_action_enable)
    prepare = action_sub.add_parser("prepare-runtime-launch", help="Freeze current startup inputs and create a dedicated approval checkpoint")
    prepare.add_argument("workspace", nargs="?", default=".")
    prepare.add_argument("--json", action="store_true")
    prepare.set_defaults(func=command_action_prepare)
    action_inspect = action_sub.add_parser("inspect", help="Read-only immutable bundle and consumed-attempt diagnostics")
    action_inspect.add_argument("workspace", nargs="?", default=".")
    action_inspect.add_argument("action_id")
    action_inspect.add_argument("--json", action="store_true")
    action_inspect.set_defaults(func=command_action_inspect)
    submit = action_sub.add_parser("submit", help="Consume exact bound approval once before an unrestricted cold-start attempt")
    submit.add_argument("workspace", nargs="?", default=".")
    submit.add_argument("action_id")
    submit.add_argument("--expect-manifest", required=True)
    submit.add_argument("--expect-checkpoint", required=True)
    submit.add_argument("--expect-decision", required=True)
    submit.add_argument("--json", action="store_true")
    submit.set_defaults(func=command_action_submit)
    acknowledge = action_sub.add_parser("acknowledge-unknown", help="Explicitly accept an unresolved attempt; never retries it or proves it stopped")
    acknowledge.add_argument("workspace", nargs="?", default=".")
    acknowledge.add_argument("action_id")
    acknowledge.add_argument("--expect-claim", required=True)
    acknowledge.add_argument("--json", action="store_true")
    acknowledge.set_defaults(func=command_action_acknowledge)
    action_stop = action_sub.add_parser("stop", help="Explicitly stop the action-scoped container without resetting consumption")
    action_stop.add_argument("workspace", nargs="?", default=".")
    action_stop.add_argument("action_id")
    action_stop.add_argument("--json", action="store_true")
    action_stop.set_defaults(func=command_action_stop)

    group = subparsers.add_parser("group", help="Explicit role membership for existing participants; optional annotation-only publication")
    group_sub = group.add_subparsers(dest="group_command", required=True)
    group_create = group_sub.add_parser("create", help="Create an empty named group without launching agents")
    group_create.add_argument("workspace", help="Existing Coven workspace")
    group_create.add_argument("group", help="Lowercase human group selector, e.g. auth-review")
    group_create.add_argument("--name", help="Display name, at most 64 UTF-8 bytes; defaults to the selector")
    group_create.add_argument("--expect-revision", help="Optional exact group-content revision")
    group_create.add_argument("--json", action="store_true")
    group_create.set_defaults(func=command_group_create)
    group_add = group_sub.add_parser("add", help="Observe one existing Kitsune agent and associate a Coven participant/role")
    group_add.add_argument("workspace")
    group_add.add_argument("group")
    group_add.add_argument("--member", required=True, help="Stable human participant selector within this group")
    group_add.add_argument("--role", required=True, help="Display role only, not a native agent configuration or execution policy")
    group_add.add_argument("--socket", required=True, help="Explicit existing Kitsune API socket")
    group_add.add_argument("--pane", required=True, help="Exact current pane ID; default requires an agent inventory entry")
    group_add.add_argument("--process-evidence", action="store_true", help="Opt into Coven-observed Djinn executable-family evidence and schema-2 storage (older Coven readers reject it); no native identity or execution grant")
    group_add.add_argument("--rebind", action="store_true", help="Explicitly accept changed provenance/terminal/native-reference/process evidence")
    group_add.add_argument("--expect-revision")
    group_add.add_argument("--json", action="store_true")
    group_add.set_defaults(func=command_group_add)
    group_remove = group_sub.add_parser("remove", help="Remove membership only; existing labels expire naturally")
    group_remove.add_argument("workspace")
    group_remove.add_argument("group")
    group_remove.add_argument("--member", required=True)
    group_remove.add_argument("--expect-revision")
    group_remove.add_argument("--json", action="store_true")
    group_remove.set_defaults(func=command_group_remove)
    group_list = group_sub.add_parser("list", help="Read declared groups and sourced native-binding evidence without contacting runtimes")
    group_list.add_argument("workspace")
    group_list.add_argument("--json", action="store_true")
    group_list.set_defaults(func=command_group_list)
    attach_live = group_sub.add_parser("attach-live", help="Explicitly associate one existing participant with a fresh pinned live source; opts into schema 3 (older readers reject it)")
    attach_live.add_argument("workspace")
    attach_live.add_argument("group")
    attach_live.add_argument("--member", required=True)
    attach_live.add_argument("--file", required=True)
    attach_live.add_argument("--runtime", required=True)
    attach_live.add_argument("--instance", required=True)
    attach_live.add_argument("--session", required=True)
    attach_live.add_argument("--reattach", action="store_true", help="Explicitly replace live-source pins only, never the terminal binding")
    attach_live.add_argument("--expect-revision")
    attach_live.add_argument("--timeout", type=float, default=1.0, help="Live-reader acceptance deadline in (0, 1] seconds")
    attach_live.add_argument("--json", action="store_true")
    attach_live.set_defaults(func=command_group_attach_live)
    detach_live = group_sub.add_parser("detach-live", help="Remove a live-source declaration without reading/deleting its artifact or controlling agents")
    detach_live.add_argument("workspace")
    detach_live.add_argument("group")
    detach_live.add_argument("--member", required=True)
    detach_live.add_argument("--expect-revision")
    detach_live.add_argument("--json", action="store_true")
    detach_live.set_defaults(func=command_group_detach_live)
    group_status = group_sub.add_parser("status", help="Read revision-scoped participant busy/unknown evidence; no writes, labels, idle or job-status inference")
    group_status.add_argument("workspace")
    group_status.add_argument("group")
    group_status.add_argument("--member", help="Optional exact member selector; default inspects all declared members")
    group_status.add_argument("--timeout", type=float, default=1.0, help="Whole-projection acceptance deadline in (0, 1] seconds, not a syscall interrupt")
    group_status.add_argument("--json", action="store_true")
    group_status.set_defaults(func=command_group_status)
    group_publish = group_sub.add_parser("publish", help="Foreground, annotation-only labels for an explicitly enabled source; never launches/stops agents")
    group_publish.add_argument("workspace")
    group_publish.add_argument("group")
    group_publish.add_argument("--socket", required=True)
    group_publish.add_argument("--source", required=True, help="Dedicated source already enabled in Kitsune, e.g. coven")
    group_publish.add_argument("--duration", type=int, default=60, help="Foreground run seconds, 1..3600 (default: 60); Ctrl-C stops refreshing")
    group_publish.add_argument("--live", action="store_true", help="Opt-in pinned legacy runner busy suffix; requires Kitsune deadline support and compatible clocks; unknown remains role-only, never idle or job completion")
    group_publish.add_argument("--json", action="store_true")
    group_publish.set_defaults(func=command_group_publish)

    observe = subparsers.add_parser("observe", help="One-shot optional observations; never dispatches agents or controls multiplexer UI")
    observe_sub = observe.add_subparsers(dest="observation_adapter", required=True)
    observe_kitsune = observe_sub.add_parser("kitsune", help="Read an existing selected JSON API socket; no discovery, autostart, or annotations")
    observe_kitsune.add_argument("--socket", required=True, help="Explicit absolute path to an existing private Kitsune JSON API socket")
    observe_kitsune.add_argument("--process-pane", help="Opt-in shadow Djinn executable-family observation for one exact pane ID; no lifecycle or control")
    observe_kitsune.add_argument("--timeout", type=float, default=3.0, help="Overall observation seconds, finite and in (0, 30]")
    observe_kitsune.add_argument("--max-response-bytes", type=int, default=1048576, help="Total response budget including LF, 1024 through 8388608")
    observe_kitsune.add_argument("--json", action="store_true", help="Compact ASCII-escaped JSON observation report")
    observe_kitsune.set_defaults(func=command_observe_kitsune)
    observe_djinn = observe_sub.add_parser("djinn", help="Read persisted folder evidence using one explicit trusted Djinn executable; never launches agent work")
    observe_djinn.add_argument("--executable", required=True, help="Explicit absolute trusted Djinn binary supporting session observe")
    observe_djinn.add_argument("--folder", required=True, help="Explicit absolute existing folder capsule, not a UI/native session ID")
    observe_djinn.add_argument("--timeout", type=float, default=3.0, help="Subprocess deadline in seconds, finite and in (0, 30]; executable hashing is separate")
    observe_djinn.add_argument("--max-output-bytes", type=int, default=131072, help="Combined stdout/stderr budget, 1024 through 1048576")
    observe_djinn.add_argument("--json", action="store_true", help="Compact ASCII-escaped observation report")
    observe_djinn.set_defaults(func=command_observe_djinn)
    observe_live = observe_sub.add_parser("djinn-live", help="Read one pinned existing private heartbeat; never starts a runtime or claims idle")
    observe_live.add_argument("--file", required=True, help="Explicit private artifact named RUNTIME_UUID.json")
    observe_live.add_argument("--runtime", required=True, help="Expected source runtime UUID; no automatic following after restart")
    observe_live.add_argument("--instance", required=True, help="Expected source instance UUID; no directory/session inference")
    observe_live.add_argument("--session", required=True, help="Exact opaque UI session ID, not a folder/native agent ID lookup")
    observe_live.add_argument("--timeout", type=float, default=1.0, help="Positive acceptance deadline in (0, 1] seconds; does not interrupt filesystem syscalls")
    observe_live.add_argument("--json", action="store_true", help="Compact ASCII-escaped live observation report")
    observe_live.set_defaults(func=command_observe_djinn_live)

    repository = subparsers.add_parser("repository", help="Explicit immutable checkout/base facts; never authorizes Git writes or execution")
    repository_sub = repository.add_subparsers(dest="repository_command", required=True)
    repository_inspect = repository_sub.add_parser("inspect", help="Read frozen registration without Git; optionally observe the current checkout")
    repository_inspect.add_argument("workspace", nargs="?", default=".")
    repository_inspect.add_argument("--verify", action="store_true", help="Bounded read-only Git observations; never refreshes registration")
    repository_inspect.add_argument("--json", action="store_true")
    repository_inspect.set_defaults(func=command_repository_inspect)
    repository_register = repository_sub.add_parser("register", help="Pin one existing checkout and full commit; no creation, replacement, or execution")
    repository_register.add_argument("workspace", nargs="?", default=".")
    repository_register.add_argument("--path", required=True, help="Existing checkout root; parent traversal is rejected")
    repository_register.add_argument("--base", required=True, help="Full lowercase commit object ID, not a ref or revision expression")
    repository_register.add_argument("--delivery-target", required=True, help="Intended refs/heads/... target only; grants no mutation permission")
    repository_register.add_argument("--expect-repository-revision", required=True, help="repository-unregistered:v1 for fresh registration; exact original value on retry")
    repository_register.add_argument("--json", action="store_true")
    repository_register.set_defaults(func=command_repository_register)

    worker = subparsers.add_parser("worker", help="Inspect native adapter compatibility without worker execution")
    worker_sub = worker.add_subparsers(dest="worker_command", required=True)
    readiness = worker_sub.add_parser("readiness", help="Help-only fail-closed Djinn adapter inspection; never grants execution permission")
    readiness.add_argument("workspace", nargs="?", default=".")
    readiness.add_argument("--adapter", choices=["djinn"], default="djinn")
    readiness.add_argument("--profile", choices=["research-readonly"], default="research-readonly")
    readiness.add_argument("--executable", help="Explicit trusted Djinn executable path; default resolves djinn from PATH")
    readiness.add_argument("--timeout", type=float, default=3.0, help="Per-probe seconds, greater than 0 and at most 30")
    readiness.add_argument("--max-output-bytes", type=int, default=131072, help="Combined output budget per help probe (1024 through 1048576)")
    readiness.add_argument("--json", action="store_true")
    readiness.set_defaults(func=command_worker_readiness)

    for name, action in (("plan", "plan"), ("up", "up"), ("start", "up"), ("down", "down"), ("stop", "down"), ("restart", "restart")):
        sub = subparsers.add_parser(name, help=f"Run coven {action} for the configured multiplexer")
        sub.add_argument("workspace", nargs="?", default=".")
        if name in {"up", "start"}:
            sub.add_argument("--replace", action="store_true", help="Kill and recreate the coven runtime before launching")
        sub.set_defaults(func=command_tmux, action=action)

    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        return args.func(args)
    except storage.StorageError as exc:
        fail(str(exc))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
