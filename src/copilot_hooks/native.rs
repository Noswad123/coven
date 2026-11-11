//! Independently declared validation of native execution positions; original
//! values are preserved. Pin: github/docs 7c3387d3b513ed43ea26c9b2e4c0e1b12234c010,
//! content/copilot/reference/hooks-reference.md and
//! content/copilot/reference/copilot-cli-reference/cli-config-dir-reference.md.
//! settings.json is native JSONC; this manager deliberately accepts strict,
//! duplicate-free JSON only, refusing JSONC rather than risking lossy edits.
use crate::{Error, Result, error::require};
use serde_json::{Map, Value};

pub(super) const PIN: &str = "github/docs@7c3387d3b513ed43ea26c9b2e4c0e1b12234c010";
pub(super) const LIMIT: usize = 1024 * 1024;

fn valid(condition: bool) -> Result<()> {
    require(condition, "copilot_hooks_invalid_settings")
}

fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|a| a.iter().all(Value::is_string))
}

fn string_map(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|m| m.values().all(Value::is_string))
}

fn handler(h: &Map<String, Value>, event: &str) -> Result<()> {
    // Validate only documented execution fields, never recurse through metadata,
    // prompts, environment values or other arbitrary JSON looking for commands.
    for key in [
        "bash",
        "powershell",
        "command",
        "exec",
        "cwd",
        "matcher",
        "url",
        "prompt",
    ] {
        if let Some(v) = h.get(key) {
            valid(v.is_string())?;
        }
    }
    for key in ["timeout", "timeoutSec"] {
        if let Some(v) = h.get(key) {
            valid(v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0))?;
        }
    }
    for key in ["args", "allowedEnvVars"] {
        if let Some(v) = h.get(key) {
            valid(strings(v))?;
        }
    }
    for key in ["env", "headers"] {
        if let Some(v) = h.get(key) {
            valid(string_map(v))?;
        }
    }
    // Nested Claude matcher groups are not Copilot direct command handlers.
    valid(!h.contains_key("hooks"))?;
    let kind = match h.get("type") {
        None => "command",
        Some(v) => v.as_str().ok_or(Error("copilot_hooks_invalid_settings"))?,
    };
    let shell = ["bash", "powershell", "command"]
        .iter()
        .any(|k| h.contains_key(*k));
    match kind {
        "command" => {
            valid(shell || h.contains_key("exec"))?;
            valid(!(shell && h.contains_key("exec")))?;
            valid(!h.contains_key("args") || h.contains_key("exec"))?;
            valid(!h.contains_key("url") && !h.contains_key("prompt"))?;
            for key in ["bash", "powershell", "command", "exec"] {
                if let Some(v) = h.get(key) {
                    valid(!v.as_str().unwrap().trim().is_empty())?;
                }
            }
        }
        "http" => {
            let url = h
                .get("url")
                .and_then(Value::as_str)
                .ok_or(Error("copilot_hooks_invalid_settings"))?;
            valid(url.starts_with("https://") || url.starts_with("http://"))?;
            if matches!(
                event,
                "preToolUse" | "PreToolUse" | "permissionRequest" | "PermissionRequest"
            ) || h.contains_key("allowedEnvVars")
            {
                valid(url.starts_with("https://"))?;
            }
            valid(
                !shell
                    && !h.contains_key("exec")
                    && !h.contains_key("args")
                    && !h.contains_key("prompt"),
            )?;
        }
        "prompt" => {
            valid(matches!(event, "sessionStart" | "SessionStart"))?;
            valid(h.contains_key("prompt"))?;
            valid(
                !shell
                    && !h.contains_key("exec")
                    && !h.contains_key("args")
                    && !h.contains_key("url"),
            )?;
        }
        _ => valid(false)?,
    }
    Ok(())
}

pub(super) fn parse(raw: &[u8]) -> Result<Value> {
    require(raw.len() <= LIMIT, "copilot_hooks_size_limit")?;
    let v = crate::strict_json::decode(raw).map_err(|_| Error("copilot_hooks_invalid_json"))?;
    valid(v.is_object())?;
    if let Some(disabled) = v.get("disableAllHooks") {
        valid(disabled.is_boolean())?;
    }
    if let Some(hooks) = v.get("hooks") {
        let hooks = hooks
            .as_object()
            .ok_or(Error("copilot_hooks_invalid_settings"))?;
        for (event, entries) in hooks {
            let entries = entries
                .as_array()
                .ok_or(Error("copilot_hooks_invalid_settings"))?;
            for entry in entries {
                handler(
                    entry
                        .as_object()
                        .ok_or(Error("copilot_hooks_invalid_settings"))?,
                    event,
                )?;
            }
        }
    }
    Ok(v)
}
