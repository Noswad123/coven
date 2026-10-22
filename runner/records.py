"""Structural contracts for the v1 file-backed workspace; no migrations or I/O.

Legacy workspaces without an explicit version use v1. State remains JSON lists,
not new envelopes. Validators preserve identities, unknown fields, and values;
lifecycle rules and reference reconciliation belong to later roadmap slices.
"""

import re
from typing import Any


def identifier(value: Any, field: str, kind: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ValueError(f"{field}: Invalid {kind} ID {value!r}; expected ASCII letters, digits, underscores, or hyphens")
    return value


def text(value: Any, field: str, *, nonblank: bool = False) -> str:
    if not isinstance(value, str) or (nonblank and not value.strip()):
        raise ValueError(f"{field} must be a {'nonblank ' if nonblank else ''}string")
    try:
        value.encode("utf-8")
    except UnicodeError as exc:
        raise ValueError(f"{field} must contain valid Unicode scalar text") from exc
    return value


def optional_text(record: dict[str, Any], fields: tuple[str, ...], source: str) -> None:
    for field in fields:
        if field in record:
            text(record[field], f"{source}.{field}")


def agent_ids(value: Any) -> list[str]:
    if not isinstance(value, list) or not value:
        raise ValueError("coven.json.agents must be a nonempty list of IDs")
    for index, agent in enumerate(value):
        identifier(agent, f"coven.json.agents[{index}]", "agent")
    if len(set(value)) != len(value):
        raise ValueError("coven.json.agents contains duplicate IDs")
    aliases = [agent.lower().strip("-") for agent in value if re.search(r"[A-Za-z0-9]", agent)]
    if len(set(aliases)) != len(aliases):
        raise ValueError("coven.json.agents contains ambiguous normalized agent ID aliases; inspect the roster without renaming it automatically")
    return value


def team(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError("coven.json must contain an object")
    name = text(value.get("name"), "coven.json.name", nonblank=True)
    if not re.search(r"[a-zA-Z0-9]", name):
        raise ValueError("coven.json.name must contain at least one letter or number for runtime naming")
    agent_ids(value.get("agents"))
    if "version" in value and (type(value["version"]) is not int or value["version"] != 1):
        raise ValueError("coven.json.version must be supported schema version 1")
    optional_text(value, ("created_at", "source_of_truth"), "coven.json")
    for field in ("opencode_command", "multiplexer"):
        if field in value:
            text(value[field], f"coven.json.{field}", nonblank=True)
    if "multiplexer" in value and value["multiplexer"].strip().lower() not in {"herdr", "tmux"}:
        raise ValueError("coven.json.multiplexer must be herdr or tmux")
    if value.get("example") is not None:
        text(value["example"], "coven.json.example", nonblank=True)
    if "display_names" in value:
        if not isinstance(value["display_names"], dict):
            raise ValueError("coven.json.display_names must be an object keyed by agent ID")
        for agent, label in value["display_names"].items():
            identifier(agent, "coven.json.display_names key", "agent")
            text(label, f"coven.json.display_names[{agent!r}]", nonblank=True)
    return value


def record_list(value: Any, source: str, kind: str, fields: tuple[str, ...]) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        raise ValueError(f"{source} must contain a list of records")
    seen = set()
    for index, record in enumerate(value):
        location = f"{source}[{index}]"
        if not isinstance(record, dict):
            raise ValueError(f"{location} must be an object")
        record_id = identifier(record.get("id"), f"{location}.id", kind)
        if record_id in seen:
            raise ValueError(f"{location}.id duplicates ID {record_id!r}")
        seen.add(record_id)
        for field in fields:
            text(record.get(field), f"{location}.{field}")
    return value


def tasks(value: Any) -> list[dict[str, Any]]:
    record_list(value, "state/tasks.json", "task", ("title", "status"))
    for index, task in enumerate(value):
        location = f"state/tasks.json[{index}]"
        if task.get("owner") is not None:
            identifier(task["owner"], f"{location}.owner", "agent")
        optional_text(task, ("notes", "created_at", "updated_at"), location)
    return value


def agents(value: Any) -> list[dict[str, Any]]:
    return record_list(value, "state/agents.json", "agent", ("status", "role"))


def lead(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    if not isinstance(value, dict):
        raise ValueError("state/lead.json must contain an object or null")
    identifier(value.get("agent"), "state/lead.json.agent", "agent")
    optional_text(value, ("set_at", "until", "instructions"), "state/lead.json")
    return value
