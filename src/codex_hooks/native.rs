//! Validation-only DTOs for openai/codex 0a94398fa60c4fac0e7659e4213a0a36e613dd46,
//! protocol/src/hooks.rs and config/src/hook_config.rs. Independently declared
//! format facts, not a provider dependency. Never serialize these DTOs: preserve
//! the original JSON values and editable TOML, including ignored native metadata.
#![allow(dead_code)]

use crate::{Error, Result, error::require};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) const EVENTS: [&str; 14] = [
    "PreModelRequest",
    "PostModelRequest",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
];

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct HookEvents {
    pre_model_request: Vec<MatcherGroup>,
    post_model_request: Vec<MatcherGroup>,
    pre_tool_use: Vec<MatcherGroup>,
    permission_request: Vec<MatcherGroup>,
    post_tool_use: Vec<MatcherGroup>,
    pre_compact: Vec<MatcherGroup>,
    post_compact: Vec<MatcherGroup>,
    session_start: Vec<MatcherGroup>,
    session_end: Vec<MatcherGroup>,
    user_prompt_submit: Vec<MatcherGroup>,
    subagent_start: Vec<MatcherGroup>,
    subagent_stop: Vec<MatcherGroup>,
    stop: Vec<MatcherGroup>,
    interrupt: Vec<MatcherGroup>,
}

#[derive(Deserialize)]
struct MatcherGroup {
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    hooks: Vec<Handler>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Handler {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default, rename = "commandWindows", alias = "command_windows")]
        command_windows: Option<String>,
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default)]
        r#async: bool,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
        #[serde(default, rename = "additionalContextLimit")]
        additional_context_limit: Option<usize>,
    },
    #[serde(rename = "mcp_tool")]
    McpTool {
        server: String,
        tool: String,
        #[serde(default, deserialize_with = "mcp_input")]
        input: Map<String, Value>,
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
    },
    #[serde(rename = "prompt")]
    Prompt {},
    #[serde(rename = "agent")]
    Agent {},
}

fn mcp_input<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Map<String, Value>, D::Error> {
    fn representable(value: &Value) -> bool {
        match value {
            Value::Null => false,
            Value::Number(n) => !n.is_u64() || n.as_i64().is_some(),
            Value::Array(items) => items.iter().all(representable),
            Value::Object(items) => items.values().all(representable),
            Value::Bool(_) | Value::String(_) => true,
        }
    }
    let input = Map::<String, Value>::deserialize(deserializer)?;
    if !input.values().all(representable) {
        return Err(de::Error::custom(
            "MCP hook input must be representable as TOML",
        ));
    }
    Ok(input)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HooksFile {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    hooks: HookEvents,
}

#[derive(Deserialize)]
struct HookState {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    trusted_hash: Option<String>,
    #[serde(default)]
    trusted_content_hash: Option<String>,
}

#[derive(Deserialize)]
struct HooksToml {
    #[serde(flatten)]
    events: HookEvents,
    #[serde(default)]
    state: BTreeMap<String, HookState>,
}

#[derive(Deserialize)]
struct ConfigHooks {
    hooks: Option<HooksToml>,
}

pub(super) fn validate_json(value: &Value) -> Result<()> {
    // Serde's derived structs also accept positional sequences. The native JSON
    // schema requires objects, and the editable-tree policy relies on that shape.
    require(value.is_object(), "codex_hooks_invalid_hooks")?;
    if let Some(events) = value.get("hooks") {
        require(events.is_object(), "codex_hooks_invalid_hooks")?;
        for event in EVENTS {
            if let Some(groups) = events.get(event) {
                let groups = groups
                    .as_array()
                    .ok_or(Error("codex_hooks_invalid_hooks"))?;
                require(
                    groups.iter().all(Value::is_object),
                    "codex_hooks_invalid_hooks",
                )?;
            }
        }
    }
    serde_json::from_value::<HooksFile>(value.clone())
        .map(|_| ())
        .map_err(|_| Error("codex_hooks_invalid_hooks"))
}

pub(super) fn validate_toml(text: &str) -> Result<()> {
    toml::from_str::<ConfigHooks>(text)
        .map(|_| ())
        .map_err(|_| Error("codex_hooks_invalid_hooks"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn generated_hook_is_accepted_by_the_pinned_native_dto() {
        let (bytes, migrated) = super::super::policy::install_json(
            None,
            Path::new("/owned/config"),
            "/explicit/coven",
            "/explicit/receiver",
            false,
        )
        .unwrap();
        assert!(!migrated);
        let file: HooksFile = serde_json::from_slice(&bytes).unwrap();
        assert!(file.description.is_none());
        let groups = &file.hooks.session_start;
        assert_eq!(groups.len(), 1);
        assert!(groups[0].matcher.is_none());
        assert_eq!(groups[0].hooks.len(), 1);
        let Handler::Command {
            command,
            command_windows,
            timeout,
            r#async,
            status_message,
            additional_context_limit,
        } = &groups[0].hooks[0]
        else {
            panic!("Coven must generate only a command handler");
        };
        assert!(command.contains("agent report --provider codex"));
        assert_eq!(*timeout, Some(2));
        assert!(!r#async);
        assert!(
            command_windows.is_none()
                && status_message.is_none()
                && additional_context_limit.is_none()
        );
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["hooks"].as_object().unwrap().len(), 1);
    }
}
