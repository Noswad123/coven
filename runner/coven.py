#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

if __package__:
    from . import records
else:
    import records

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
MANAGED_DIRECTORIES = ("agents", "prompts", "state", "logs", "checkpoints", "work", "output", "tmux")
MANAGED_FILES = (
    "coven.json", "coven.toml", "goal.md", "spec.md", "dashboard.md",
    "state/agents.json", "state/tasks.json", "state/lead.json",
    "logs/events.jsonl", "logs/messages.jsonl", "tmux/coven.toml",
    "work/README.md", "output/README.md",
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
    for relative in MANAGED_FILES:
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


def append_event(workspace: Path, event_type: str, **payload: Any) -> None:
    preflight_workspace_paths(workspace)
    event = {"ts": now(), "type": event_type, **payload}
    path = managed_path(workspace, "logs/events.jsonl")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(event, sort_keys=True) + "\n")


def append_message(workspace: Path, sender: str, recipient: str, body: str) -> None:
    preflight_workspace_paths(workspace)
    managed_path(workspace, "logs/events.jsonl")
    message = {"ts": now(), "from": sender, "to": recipient, "body": body}
    path = managed_path(workspace, "logs/messages.jsonl")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(message, sort_keys=True) + "\n")
    append_event(workspace, "message.sent", sender=sender, recipient=recipient)


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
6. `state/tasks.json`
7. `logs/messages.jsonl`
8. `checkpoints/*.md`

Rules:
- Use `logs/events.jsonl` and `logs/messages.jsonl` as append-only history.
- Update `state/tasks.json` and regenerate/maintain `dashboard.md` when task state changes.
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
1. Read `goal.md`, `spec.md` if present, `dashboard.md`, `coven.toml`, `agents/{agent}.md`, and `state/tasks.json`.
2. Find your assigned task(s).
3. Append a short `agent.started` event to `logs/events.jsonl`.
4. Inspect `checkpoints/*.md`, `work/`, and `output/`.
5. Work only within your role and current assignment.
6. If you reach a checkpoint requiring user approval, stop and write the request to `logs/messages.jsonl`, `dashboard.md`, and the relevant `checkpoints/<id>.md` file.

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
source_of_truth = "logs/events.jsonl"
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


def refresh_dashboard(workspace: Path) -> None:
    managed_path(workspace, "dashboard.md").write_text(render_dashboard(workspace), encoding="utf-8")


def read_lead(workspace: Path) -> dict[str, Any] | None:
    return read_state_records(workspace, "state/lead.json", None, records.lead)


def write_lead(workspace: Path, state: dict[str, Any] | None) -> None:
    preflight_workspace_paths(workspace)
    try:
        records.lead(state)
    except ValueError as exc:
        fail(f"Invalid proposed lead record: {exc}")
    path = managed_path(workspace, "state/lead.json")
    if state is None:
        if path.exists():
            path.unlink()
        return
    write_json(path, state)


def checkpoint_status(workspace: Path, checkpoint: str) -> str:
    content = checkpoint_text(workspace, checkpoint)
    if content is None:
        return "missing"
    header, _ = checkpoint_header(content)
    for _, _, line in markdown_lines(header):
        status = re.match(r"^ {0,3}status:([^\r\n]*)$", line, re.IGNORECASE)
        if status:
            return status.group(1).strip().lower() or "unknown"
    return "unknown"


def markdown_lines(content: str, *, strict: bool = False) -> list[tuple[int, int, str]]:
    """Offsets of non-code lines for the small generated checkpoint format."""
    visible = []
    fence: tuple[str, int] | None = None
    offset = 0
    for raw in content.splitlines(keepends=True):
        line = raw.rstrip("\r\n")
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence:
            if marker and marker[1][0] == fence[0] and len(marker[1]) >= fence[1] and not marker[2].strip():
                fence = None
        elif marker:
            fence = (marker[1][0], len(marker[1]))
        elif not line.startswith(("    ", "\t")):
            visible.append((offset, offset + len(raw), line))
        offset += len(raw)
    if strict and fence:
        fail("Cannot edit checkpoint: unterminated fenced code block")
    return visible


def markdown_heading(line: str) -> tuple[int, str] | None:
    match = re.match(r"^ {0,3}(#{1,6})[ \t]+(.+?)[ \t]*$", line)
    if not match:
        return None
    title = re.sub(r"[ \t]+#+[ \t]*$", "", match[2])
    return len(match[1]), title.lower()


def checkpoint_header(content: str) -> tuple[str, str]:
    """Only metadata before the first H2 describes the current decision."""
    for start, _, line in markdown_lines(content):
        heading = markdown_heading(line)
        if heading and heading[0] == 2:
            return content[:start], content[start:]
    return content, ""


def checkpoint_decision_text(content: str, status: str) -> str:
    markdown_lines(content, strict=True)
    header, body = checkpoint_header(content)
    status_lines = [(start, end) for start, end, line in markdown_lines(header)
                    if re.match(r"^ {0,3}status:", line, re.IGNORECASE)]
    if len(status_lines) > 1:
        fail("Cannot edit checkpoint: ambiguous current status headers")
    if status_lines:
        start, end = status_lines[0]
        header = header[:start] + f"Status: {status}\n" + header[end:]
    else:
        header += ("" if header.endswith("\n") else "\n") + f"\nStatus: {status}\n\n"

    approved = status == "approved"
    decisions = (("Approved", approved), ("Rejected / needs changes", not approved))
    visible = markdown_lines(body)
    sections = [(start, end) for start, end, line in visible if markdown_heading(line) == (2, "user decision")]
    if len(sections) > 1:
        fail("Cannot edit checkpoint: ambiguous User decision sections")
    if not sections:
        current = "\n".join(f"- [{'x' if checked else ' '}] {label}" for label, checked in decisions)
        return header + f"## User decision\n\n{current}\n\n" + body

    _, start = sections[0]
    end = next((offset for offset, _, line in visible if offset >= start and markdown_heading(line)), len(body))
    current = body[start:end]
    replacements = []
    missing = []
    for label, checked in decisions:
        pattern = re.compile(r"^ {0,3}-[ \t]+\[[ xX]\][ \t]+" + re.escape(label) + r"[ \t]*$", re.IGNORECASE)
        checkbox = f"- [{'x' if checked else ' '}] {label}"
        matches = [(offset, stop) for offset, stop, line in markdown_lines(current) if pattern.match(line)]
        if len(matches) > 1:
            fail(f"Cannot edit checkpoint: ambiguous current `{label}` checkboxes")
        if matches:
            replacements.append((*matches[0], checkbox + "\n"))
        else:
            missing.append(checkbox)
    for offset, stop, checkbox in sorted(replacements, reverse=True):
        current = current[:offset] + checkbox + current[stop:]
    if missing:
        current += ("" if current.endswith("\n") else "\n") + "\n".join(missing) + "\n\n"
    return header + body[:start] + current + body[end:]


def prepare_checkpoint_decision(workspace: Path, checkpoint: str, status: str, heading: str, notes: str) -> tuple[Path, str]:
    path = checkpoint_path(workspace, checkpoint)
    content = checkpoint_text(workspace, checkpoint)
    if content is None:
        checkpoint_id = path.stem
        content = checkpoint_file(checkpoint_id, checkpoint_id.replace("-", " ").title(), "User approval checkpoint.")
    content = checkpoint_decision_text(content, status)
    content += f"\n## {heading} — {now()}\n\n{notes}\n"
    # Notes are historical, but must not introduce an ambiguous live section or
    # leave an open fence that would prevent the next explicit decision.
    checkpoint_decision_text(content, status)
    return path, content


def write_checkpoint_decision(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


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
        "source_of_truth": "logs/events.jsonl",
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
    (workspace / "goal.md").write_text(f"# Goal\n\n{text}\n", encoding="utf-8")
    append_event(workspace, "goal.updated", goal=text)
    refresh_dashboard(workspace)
    info("Goal updated")
    return 0


def next_task_id(tasks: list[dict[str, Any]]) -> str:
    existing = {str(task.get("id")) for task in tasks if isinstance(task, dict) and task.get("id")}
    return unique_slug("task", existing)


def command_task_add(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    team = load_team(workspace)
    if not rest:
        fail("task add requires a title")
    title = " ".join(rest)
    owner = resolve_agent(team, args.owner) if args.owner else None
    tasks = read_tasks(workspace)
    existing = {str(task.get("id")) for task in tasks if isinstance(task, dict) and task.get("id")}
    try:
        task_id = unique_slug(title, existing)
    except ValueError as exc:
        fail(f"Invalid task title: {exc}")
    task = {
        "id": task_id,
        "title": title,
        "status": "pending" if not owner else "assigned",
        "owner": owner,
        "notes": args.notes or "",
        "created_at": now(),
        "updated_at": now(),
    }
    tasks.append(task)
    try:
        records.tasks(tasks)
    except ValueError as exc:
        fail(f"Invalid proposed task records: {exc}")
    write_json(workspace / "state" / "tasks.json", tasks)
    append_event(workspace, "task.created", task=task)
    refresh_dashboard(workspace)
    print(f"Created {task['id']}: {task['title']}")
    return 0


def command_task_assign(args: argparse.Namespace) -> int:
    items = list(args.items)
    workspace, rest = split_optional_workspace(items)
    if len(rest) < 2:
        fail("task assign requires task_id and agent")
    task_id, agent = rest[0], rest[1]
    team = load_team(workspace)
    agent = resolve_agent(team, agent)
    tasks = read_tasks(workspace)
    for task in tasks:
        if task.get("id") == task_id:
            task["owner"] = agent
            task["status"] = "assigned"
            task["updated_at"] = now()
            records.tasks(tasks)
            write_json(workspace / "state" / "tasks.json", tasks)
            append_event(workspace, "task.assigned", task_id=task_id, agent=agent)
            refresh_dashboard(workspace)
            print(f"Assigned {task_id} to {agent}")
            return 0
    fail(f"Task not found: {task_id}")
    return 1


def command_task_list(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    load_team(workspace)
    tasks = read_tasks(workspace)
    if not tasks:
        print("No tasks yet")
        return 0
    for task in tasks:
        print(f"{task['id']} [{task['status']}] {task.get('owner') or 'unassigned'} - {task['title']}")
    return 0


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


def print_file(path: Path, default: str) -> int:
    if not path.exists():
        print(default)
        return 0
    print(path.read_text(encoding="utf-8"), end="")
    return 0


def iter_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    items: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            items.append(value)
    return items


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
    require_agent(team, agent)
    target = running_agent_pane(team, agent)
    if not target:
        return False
    if runtime_multiplexer(team) == "herdr":
        subprocess.run(["herdr", "pane", "send-text", target, text], check=False)
        subprocess.run(["herdr", "pane", "send-keys", target, "enter"], check=False)
    else:
        subprocess.run(["tmux", "send-keys", "-t", target, text, "Enter"], check=False)
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
    pane = running_agent_pane(team, agent)
    return ("running", pane) if pane else ("stopped", None)


def start_agent_window(workspace: Path, team: dict[str, Any], agent: str) -> bool:
    preflight_workspace_paths(workspace, team)
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


def stop_agent_window(workspace: Path, team: dict[str, Any], agent: str) -> bool:
    preflight_workspace_paths(workspace, team)
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


def approve_checkpoint(workspace: Path, team: dict[str, Any], checkpoint: str, message: str, agent_selector: str = "all") -> tuple[int, list[str]]:
    preflight_workspace_paths(workspace, team)
    checkpoint_text(workspace, checkpoint)
    if agent_selector != "all":
        require_agent(team, agent_selector)
    path, content = prepare_checkpoint_decision(workspace, checkpoint, "approved", "Approval", message)
    append_event(workspace, "checkpoint.approved", checkpoint=checkpoint, by="user")
    append_message(workspace, "user", "coven", message)
    write_checkpoint_decision(path, content)
    sent, missing = notify_agents(
        workspace,
        team,
        agent_selector,
        message,
        event_type="checkpoint.approval.sent",
        checkpoint=checkpoint,
    )
    refresh_dashboard(workspace)
    return sent, missing


def suggest_checkpoint_changes(workspace: Path, team: dict[str, Any], checkpoint: str, notes: str, agent_selector: str = "all") -> tuple[int, list[str]]:
    preflight_workspace_paths(workspace, team)
    checkpoint_text(workspace, checkpoint)
    if agent_selector != "all":
        require_agent(team, agent_selector)
    if not notes.strip():
        fail("Change request cannot be empty")
    path, content = prepare_checkpoint_decision(workspace, checkpoint, "changes_requested", "Changes requested", notes)
    write_checkpoint_decision(path, content)
    message = f"Changes requested for checkpoint `{checkpoint}`:\n\n{notes}"
    append_event(workspace, "checkpoint.changes.requested", checkpoint=checkpoint, by="user")
    append_message(workspace, "user", "coven", message)
    sent, missing = notify_agents(
        workspace,
        team,
        agent_selector,
        message,
        event_type="checkpoint.change_request.sent",
        checkpoint=checkpoint,
    )
    refresh_dashboard(workspace)
    return sent, missing


def lead_prompt(agent: str, workspace: Path, goal: str) -> str:
    return f"""You are the coven lead for this workspace until the goal is achieved or there is a critical blocker requiring the user.

Workspace: {workspace}
Goal: {goal or 'See goal.md'}

Your operating loop:
1. Run or inspect `coven standup` to see coven status, blockers, checkpoints, and agent runtime state.
2. Use `coven send <agent|all> <message>` to move agents along through the configured terminal multiplexer.
3. Make your best decision to unblock the coven without waiting for the user unless a checkpoint, destructive action, missing requirement, or critical blocker requires user input.
4. Keep `logs/events.jsonl`, `logs/messages.jsonl`, `state/tasks.json`, and `dashboard.md` current.
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
        previous = read_lead(workspace)
        write_lead(workspace, None)
        append_event(workspace, "lead.cleared", previous=previous)
        refresh_dashboard(workspace)
        info("Cleared coven lead")
        return 0

    agent = rest[1] if rest[0] == "set" and len(rest) > 1 else rest[0]
    require_agent(team, agent)
    goal = (workspace / "goal.md").read_text(encoding="utf-8").strip() if (workspace / "goal.md").exists() else ""
    if goal.startswith("# Goal"):
        goal = goal.removeprefix("# Goal").strip()
    state = {
        **(read_lead(workspace) or {}),
        "agent": agent,
        "set_at": now(),
        "until": "goal achieved or critical blocker requiring the user",
        "instructions": "Run the coven, use coven standup, and use coven send to move agents along.",
    }
    write_lead(workspace, state)
    message = args.message or lead_prompt(agent, workspace, goal)
    append_event(workspace, "lead.set", agent=agent)
    append_message(workspace, "user", agent, message)
    sent, missing = notify_agents(workspace, team, agent, message, event_type="lead.message.sent")
    refresh_dashboard(workspace)
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
    message = args.message or (
        f"Checkpoint `{checkpoint}` approved by user. Continue your assigned task. "
        "Update state/tasks.json, dashboard.md, logs/events.jsonl, and logs/messages.jsonl as you work."
    )
    sent, missing = approve_checkpoint(workspace, team, checkpoint, message, args.agent)
    if missing:
        info(f"No running pane found for: {', '.join(missing)}")
    info(f"Approved {checkpoint}; notified {sent} agent(s)")
    return 0


def command_suggest(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    if len(rest) < 2:
        fail("suggest requires checkpoint and change request text")
    checkpoint = rest[0]
    notes = " ".join(rest[1:])
    team = load_team(workspace)
    sent, missing = suggest_checkpoint_changes(workspace, team, checkpoint, notes, args.agent)
    if missing:
        info(f"No running pane found for: {', '.join(missing)}")
    info(f"Saved change request for {checkpoint}; notified {sent} agent(s)")
    return 0


def command_review(args: argparse.Namespace) -> int:
    workspace, rest = split_optional_workspace(list(args.items))
    team = load_team(workspace)
    checkpoints = rest or checkpoint_review_candidates(workspace)
    paths = [(checkpoint, checkpoint_path(workspace, checkpoint), checkpoint_text(workspace, checkpoint)) for checkpoint in checkpoints]
    if not checkpoints:
        print("No pending checkpoint files found.")
        return 0

    for checkpoint, path, content in paths:
        print("\n" + "=" * 80)
        print(f"Review checkpoint: {checkpoint}")
        print(f"File: {path}")
        print("=" * 80)
        if content is not None:
            print(content)
        else:
            print("Checkpoint file does not exist yet; it will be created if you approve or suggest changes.")

        while True:
            try:
                action = input("[a]pprove, [s]uggest changes, [n]ext, [q]uit > ").strip().lower()
            except EOFError:
                print()
                return 0
            if action in {"a", "approve"}:
                message = args.message or f"Checkpoint `{checkpoint}` approved by user after interactive review. Continue."
                sent, missing = approve_checkpoint(workspace, team, checkpoint, message, args.agent)
                if missing:
                    info(f"No running pane found for: {', '.join(missing)}")
                info(f"Approved {checkpoint}; notified {sent} agent(s)")
                break
            if action in {"s", "suggest", "changes", "change"}:
                notes = prompt_multiline("Enter requested changes for the agents.")
                if notes:
                    sent, missing = suggest_checkpoint_changes(workspace, team, checkpoint, notes, args.agent)
                    if missing:
                        info(f"No running pane found for: {', '.join(missing)}")
                    info(f"Saved change request for {checkpoint}; notified {sent} agent(s)")
                else:
                    info("No change request saved")
                break
            if action in {"n", "next", "skip"}:
                break
            if action in {"q", "quit", "exit"}:
                return 0
            print("Choose approve, suggest changes, next, or quit.")
    return 0


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
    while True:
        try:
            raw = input("coven> ").strip()
        except EOFError:
            print()
            return 0
        if not raw:
            continue
        if raw in {"q", "quit", "exit"}:
            return 0
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
            command_review(argparse.Namespace(items=items, agent="all", message=None))
            continue
        if command == "lead":
            items = [str(workspace)] + raw.split()[1:]
            command_lead(argparse.Namespace(items=items, message=None))
            continue
        if command == "approve":
            checkpoint = parts[1] if len(parts) > 1 else "plan-approval"
            message = f"Checkpoint `{checkpoint}` approved by user from coven console. Continue."
            sent, missing = approve_checkpoint(workspace, team, checkpoint, message, "all")
            if missing:
                info(f"No running pane found for: {', '.join(missing)}")
            info(f"Approved {checkpoint}; notified {sent} agent(s)")
            continue
        if command == "suggest":
            if len(parts) < 3:
                print("Usage: suggest <checkpoint> <requested changes>")
                continue
            sent, missing = suggest_checkpoint_changes(workspace, team, parts[1], parts[2], "all")
            if missing:
                info(f"No running pane found for: {', '.join(missing)}")
            info(f"Saved change request for {parts[1]}; notified {sent} agent(s)")
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
    return 0


def command_refresh(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
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
    manifest_path = managed_path(workspace, "tmux/coven.toml")
    write_if_missing(manifest_path, manifest, force=True)
    for relative, content in missing_prompts.items():
        write_if_missing(managed_path(workspace, relative), content, force=False)
    for relative, content in prompt_files.items():
        write_if_missing(managed_path(workspace, relative), content, force=True)
    managed_path(workspace, "dashboard.md").write_text(dashboard, encoding="utf-8")
    info(f"Regenerated tmux/coven.toml and starter prompt files (runtime={multiplexer})")
    return 0


def team_session_name(team: dict[str, Any]) -> str:
    return f"coven-{slugify(str(team['name']))}"


def command_bootstrap(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
    team = load_team(workspace)
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


def create_coven_tmux_session(workspace: Path, team: dict[str, Any]) -> None:
    preflight_workspace_paths(workspace, team)
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


def create_coven_herdr_workspace(workspace: Path, team: dict[str, Any]) -> str:
    preflight_workspace_paths(workspace, team)
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
    multiplexer = runtime_multiplexer(team)
    if multiplexer == "herdr":
        return run_coven_herdr(workspace, team, action)
    return run_coven_tmux(workspace, team, action)


def command_tmux(args: argparse.Namespace) -> int:
    workspace = workspace_path(args.workspace)
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
    approve.set_defaults(func=command_approve)

    suggest = subparsers.add_parser("suggest", help="Request changes for a checkpoint and notify agents")
    suggest.add_argument("items", nargs="+", help="Optional workspace, then checkpoint and change request text")
    suggest.add_argument("--agent", default="all", help="Agent id to notify, or 'all' (default)")
    suggest.set_defaults(func=command_suggest)

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
    task_add.set_defaults(func=command_task_add)
    task_assign = task_sub.add_parser("assign")
    task_assign.add_argument("items", nargs="+", help="Optional workspace followed by task_id and agent")
    task_assign.set_defaults(func=command_task_assign)
    task_list = task_sub.add_parser("list")
    task_list.add_argument("workspace", nargs="?", default=".")
    task_list.set_defaults(func=command_task_list)

    for name, action in (("plan", "plan"), ("up", "up"), ("start", "up"), ("down", "down"), ("stop", "down"), ("restart", "restart")):
        sub = subparsers.add_parser(name, help=f"Run coven {action} for the configured multiplexer")
        sub.add_argument("workspace", nargs="?", default=".")
        if name in {"up", "start"}:
            sub.add_argument("--replace", action="store_true", help="Kill and recreate the coven runtime before launching")
        sub.set_defaults(func=command_tmux, action=action)

    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
