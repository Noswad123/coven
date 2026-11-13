use super::native;
use crate::{Error, Result, error::require};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use toml_edit::{DocumentMut, Item, TableLike};

pub(super) const LIMIT: usize = 1024 * 1024;
const MARKER: &str = "coven-managed:codex-hooks:v1";
pub(super) const KIT_SHA: &str = "2343713ae8e665baa22968e155ced9fdc1269f43b490af230316754a18caaf9a";

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn absolute(path: &Path) -> Result<()> {
    require(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
            && path
                .to_str()
                .is_some_and(|s| !s.chars().any(char::is_control)),
        "codex_hooks_absolute_path_required",
    )
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
fn block(exe: &str, socket: &str) -> Value {
    json!({"hooks":[{"type":"command", "command":format!(
        "{} agent report --provider codex {} --event SessionStart >/dev/null 2>&1 || true # {MARKER}",
        quote(exe), crate::integration_route::shell(socket)), "timeout":2}]})
}

pub(super) fn parse_json(raw: &[u8]) -> Result<Value> {
    require(raw.len() <= LIMIT, "codex_hooks_size_limit")?;
    let v = crate::strict_json::decode(raw).map_err(|_| Error("codex_hooks_invalid_json"))?;
    native::validate_json(&v)?;
    Ok(v)
}
pub(super) fn parse_toml(raw: &[u8]) -> Result<DocumentMut> {
    require(raw.len() <= LIMIT, "codex_hooks_size_limit")?;
    let text = std::str::from_utf8(raw).map_err(|_| Error("codex_hooks_invalid_toml"))?;
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|_| Error("codex_hooks_invalid_toml"))?;
    if let Some(features) = doc.get("features") {
        let table = features
            .as_table()
            .ok_or(Error("codex_hooks_unsupported_feature_layout"))?;
        require(!table.is_dotted(), "codex_hooks_unsupported_feature_layout")?;
        for key in ["hooks", "codex_hooks"] {
            require(
                table.get(key).is_none_or(|v| v.as_bool().is_some()),
                "codex_hooks_invalid_features",
            )?;
        }
    }
    if let Some(hooks) = doc.get("hooks") {
        require(hooks.is_table_like(), "codex_hooks_invalid_hooks")?;
    }
    native::validate_toml(text)?;
    Ok(doc)
}

// Exact native-format facts from Kitsune targets.rs, independently implemented.
// No Kit code is executed or used as authority. A suspicious substring refuses.
fn kit_command(command: &str, event: &str, root: &Path, consent: bool) -> Result<bool> {
    require(
        !command.contains(MARKER),
        "codex_hooks_unreceipted_ownership",
    )?;
    if !command.contains("kitsune-agent-state") {
        return Ok(false);
    }
    let actions: &[&str] = match event {
        "SessionStart" => &["session", "idle"],
        "UserPromptSubmit" | "PreToolUse" => &["working"],
        "PermissionRequest" => &["blocked"],
        "Stop" => &["idle"],
        _ => &[],
    };
    let script = root.join("kitsune-agent-state.sh");
    let prefix = format!(
        "bash {} ",
        quote(
            script
                .to_str()
                .ok_or(Error("codex_hooks_absolute_path_required"))?
        )
    );
    require(
        actions.iter().any(|a| command == format!("{prefix}{a}")),
        "codex_hooks_ambiguous_kitsune",
    )?;
    require(consent, "codex_hooks_migration_consent_required")?;
    Ok(true)
}
fn encode(v: &Value) -> Result<Vec<u8>> {
    native::validate_json(v)?;
    let mut bytes = serde_json::to_vec_pretty(v)?;
    bytes.push(b'\n');
    require(bytes.len() <= LIMIT, "codex_hooks_size_limit")?;
    Ok(bytes)
}
pub(super) fn install_json(
    raw: Option<&[u8]>,
    root: &Path,
    exe: &str,
    socket: &str,
    consent: bool,
) -> Result<(Vec<u8>, bool)> {
    let mut v = parse_json(raw.unwrap_or(b"{}"))?;
    let hooks = v
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert(json!({}))
        .as_object_mut()
        .unwrap();
    let mut migrated = false;
    for (event, entries) in hooks.iter_mut() {
        if !native::EVENTS.contains(&event.as_str()) {
            continue;
        }
        let entries = entries.as_array_mut().unwrap();
        let mut empty = Vec::new();
        for (index, entry) in entries.iter_mut().enumerate() {
            let Some(children) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            let mut remove = Vec::new();
            for (i, child) in children.iter().enumerate() {
                if child["type"] != "command" {
                    continue;
                }
                if let Some(cmd) = child.get("command").and_then(Value::as_str) {
                    if kit_command(cmd, event, root, consent)? {
                        require(child["type"] == "command", "codex_hooks_ambiguous_kitsune")?;
                        remove.push(i);
                    }
                }
            }
            if !remove.is_empty() {
                migrated = true;
                for i in remove.into_iter().rev() {
                    children.remove(i);
                }
                if children.is_empty() {
                    empty.push(index);
                }
            }
        }
        for i in empty.into_iter().rev() {
            entries.remove(i);
        }
    }
    hooks
        .entry("SessionStart")
        .or_insert(json!([]))
        .as_array_mut()
        .unwrap()
        .push(block(exe, socket));
    Ok((encode(&v)?, migrated))
}
pub(super) fn uninstall_json(raw: &[u8], exe: &str, socket: &str) -> Result<Vec<u8>> {
    let mut v = parse_json(raw)?;
    let entries = v["hooks"]["SessionStart"]
        .as_array_mut()
        .ok_or(Error("codex_hooks_invalid_receipt"))?;
    let owned = block(exe, socket);
    require(
        entries.iter().filter(|e| **e == owned).count() == 1,
        "codex_hooks_invalid_receipt",
    )?;
    entries.retain(|e| *e != owned);
    encode(&v)
}

// Only direct handlers in native matcher groups are ownership positions. Never
// descend into MCP input, ignored metadata, or hook approval state.
fn owned_handler(
    table: &mut dyn TableLike,
    event: &str,
    root: &Path,
    consent: bool,
    migrated: &mut bool,
) -> Result<bool> {
    if table.get("type").and_then(Item::as_str) == Some("command") {
        let cmd = table
            .get("command")
            .ok_or(Error("codex_hooks_invalid_hooks"))?;
        let cmd = cmd.as_str().ok_or(Error("codex_hooks_invalid_hooks"))?;
        if kit_command(cmd, event, root, consent)? {
            *migrated = true;
            return Ok(true);
        }
    }
    Ok(false)
}
fn clean_handlers(
    item: &mut Item,
    event: &str,
    root: &Path,
    consent: bool,
    migrated: &mut bool,
) -> Result<()> {
    if let Some(array) = item.as_array_mut() {
        for i in (0..array.len()).rev() {
            let table = array
                .get_mut(i)
                .unwrap()
                .as_inline_table_mut()
                .ok_or(Error("codex_hooks_invalid_hooks"))?;
            if owned_handler(table, event, root, consent, migrated)? {
                array.remove(i);
            }
        }
    } else if let Some(array) = item.as_array_of_tables_mut() {
        for i in (0..array.len()).rev() {
            if owned_handler(array.get_mut(i).unwrap(), event, root, consent, migrated)? {
                array.remove(i);
            }
        }
    } else {
        return Err(Error("codex_hooks_invalid_hooks"));
    }
    Ok(())
}
fn clean_groups(
    item: &mut Item,
    event: &str,
    root: &Path,
    consent: bool,
    migrated: &mut bool,
) -> Result<()> {
    if let Some(array) = item.as_array_mut() {
        for group in array.iter_mut() {
            let table = group
                .as_inline_table_mut()
                .ok_or(Error("codex_hooks_invalid_hooks"))?;
            if let Some(handlers) = table.get_mut("hooks") {
                // Inline tables expose Values, while the same handler-list
                // routine also accepts the Item used by arrays of tables.
                let mut handlers_item = Item::Value(handlers.clone());
                clean_handlers(&mut handlers_item, event, root, consent, migrated)?;
                *handlers = handlers_item
                    .into_value()
                    .map_err(|_| Error("codex_hooks_invalid_hooks"))?;
            }
        }
    } else if let Some(array) = item.as_array_of_tables_mut() {
        for group in array.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks") {
                clean_handlers(handlers, event, root, consent, migrated)?;
            }
        }
    } else {
        return Err(Error("codex_hooks_invalid_hooks"));
    }
    Ok(())
}
pub(super) fn install_toml(
    raw: Option<&[u8]>,
    root: &Path,
    consent: bool,
) -> Result<(Vec<u8>, bool)> {
    let mut doc = parse_toml(raw.unwrap_or(b""))?;
    let mut migrated = false;
    if let Some(hooks) = doc.get_mut("hooks") {
        let hooks = hooks.as_table_like_mut().unwrap();
        for event in native::EVENTS {
            if let Some(groups) = hooks.get_mut(event) {
                clean_groups(groups, event, root, consent, &mut migrated)?;
            }
        }
    }
    if !doc.contains_key("features") {
        doc["features"] = Item::Table(toml_edit::Table::new());
    }
    let features = doc["features"].as_table_mut().unwrap();
    features.remove("codex_hooks");
    if features.get("hooks").and_then(Item::as_bool) != Some(true) {
        let mut enabled = toml_edit::Value::from(true);
        if let Some(old) = features.get("hooks").and_then(Item::as_value) {
            *enabled.decor_mut() = old.decor().clone();
        }
        if let Some(item) = features.get_mut("hooks") {
            *item = Item::Value(enabled);
        } else {
            features.insert("hooks", Item::Value(enabled));
        }
    }
    let bytes = doc.to_string().into_bytes();
    require(bytes.len() <= LIMIT, "codex_hooks_size_limit")?;
    parse_toml(&bytes)?;
    Ok((bytes, migrated))
}

pub(super) fn feature_change(
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<serde_json::Value> {
    let before = parse_toml(before.unwrap_or(b""))?;
    let after = parse_toml(after.unwrap_or(b""))?;
    let before_alias = before
        .get("features")
        .and_then(|v| v.get("codex_hooks"))
        .is_some();
    let after_alias = after
        .get("features")
        .and_then(|v| v.get("codex_hooks"))
        .is_some();
    Ok(
        json!({"hooks_before":before.get("features").and_then(|v| v.get("hooks")).and_then(Item::as_bool),
        "hooks_after":after.get("features").and_then(|v| v.get("hooks")).and_then(Item::as_bool),
        "remove_legacy_alias":before_alias && !after_alias,
        "restore_legacy_alias":!before_alias && after_alias}),
    )
}
