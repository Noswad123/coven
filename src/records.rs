//! Validated views retain the complete ordered value, including extensions and
//! absent versus null fields. No lifecycle/roster reconciliation is inferred.
use crate::{
    Error, Result,
    error::require,
    values::{Object, Value},
};
use std::collections::HashSet;

pub(crate) fn python_space(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}
pub(crate) fn trimmed(value: &str) -> &str {
    value.trim_matches(python_space)
}

fn identifier(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or(Error("invalid_record_identifier"))?;
    require(
        !value.is_empty()
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "invalid_record_identifier",
    )?;
    Ok(value)
}
fn text(value: &Value, nonblank: bool) -> Result<&str> {
    let value = value
        .as_str()
        .ok_or(Error("invalid_workspace_record_text"))?;
    require(
        !nonblank || !trimmed(value).is_empty(),
        "invalid_workspace_record_text",
    )?;
    Ok(value)
}
fn required<'a>(object: &'a Object, field: &str) -> Result<&'a Value> {
    object
        .get(field)
        .ok_or(Error("missing_workspace_record_field"))
}
fn optional_text(object: &Object, fields: &[&str]) -> Result<()> {
    for field in fields {
        if let Some(value) = object.get(field) {
            text(value, false)?;
        }
    }
    Ok(())
}

pub struct TeamRecord {
    value: Value,
}
impl TeamRecord {
    pub fn validate(value: Value) -> Result<Self> {
        let object = value.as_object().ok_or(Error("invalid_team_record"))?;
        let name = text(required(object, "name")?, true)?;
        require(
            name.bytes().any(|c| c.is_ascii_alphanumeric()),
            "invalid_team_name",
        )?;
        let agents = required(object, "agents")?
            .as_array()
            .ok_or(Error("invalid_team_roster"))?;
        require(!agents.is_empty(), "invalid_team_roster")?;
        let mut ids = HashSet::new();
        let mut aliases = HashSet::new();
        for value in agents {
            let id = identifier(value)?;
            require(ids.insert(id), "duplicate_record_identifier")?;
            if id.bytes().any(|c| c.is_ascii_alphanumeric()) {
                require(
                    aliases.insert(id.to_ascii_lowercase().trim_matches('-').to_owned()),
                    "ambiguous_team_alias",
                )?;
            }
        }
        if let Some(version) = object.get("version") {
            require(version.is_integer_one(), "unsupported_workspace_version")?;
        }
        optional_text(object, &["created_at", "source_of_truth"])?;
        for field in ["opencode_command", "multiplexer"] {
            if let Some(value) = object.get(field) {
                text(value, true)?;
            }
        }
        if let Some(value) = object.get("multiplexer") {
            multiplexer(text(value, true)?)?;
        }
        if let Some(value) = object.get("example") {
            if !matches!(value, Value::Null) {
                text(value, true)?;
            }
        }
        if let Some(names) = object.get("display_names") {
            let names = names.as_object().ok_or(Error("invalid_display_names"))?;
            for (id, name) in names.entries() {
                identifier(&Value::String(id.clone()))?;
                text(name, true)?;
            }
        }
        Ok(Self { value })
    }
    pub(crate) fn agent_ids(&self) -> Result<Vec<&str>> {
        let object = self.value.as_object().ok_or(Error("invalid_team_record"))?;
        required(object, "agents")?
            .as_array()
            .ok_or(Error("invalid_team_roster"))?
            .iter()
            .map(identifier)
            .collect()
    }
    pub(crate) fn configured_multiplexer(&self) -> Option<&str> {
        self.value.as_object()?.get("multiplexer")?.as_str()
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
}

pub(crate) fn multiplexer(value: &str) -> Result<()> {
    require(
        matches!(trimmed(value).to_lowercase().as_str(), "herdr" | "tmux"),
        "unsupported_multiplexer",
    )
}

fn record_list<'a>(value: &'a Value, fields: &[&str]) -> Result<Vec<&'a Object>> {
    let array = value
        .as_array()
        .ok_or(Error("invalid_workspace_record_list"))?;
    let mut ids = HashSet::new();
    let mut objects = Vec::new();
    for value in array {
        let object = value.as_object().ok_or(Error("invalid_workspace_record"))?;
        require(
            ids.insert(identifier(required(object, "id")?)?),
            "duplicate_record_identifier",
        )?;
        for field in fields {
            text(required(object, field)?, false)?;
        }
        objects.push(object);
    }
    Ok(objects)
}

pub struct TaskRecords {
    value: Value,
}
impl TaskRecords {
    pub fn validate(value: Value) -> Result<Self> {
        for object in record_list(&value, &["title", "status"])? {
            if let Some(owner) = object.get("owner") {
                if !matches!(owner, Value::Null) {
                    identifier(owner)?;
                }
            }
            optional_text(object, &["notes", "created_at", "updated_at"])?;
        }
        Ok(Self { value })
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn into_value(self) -> Value {
        self.value
    }
}

pub(crate) fn agents(value: &Value) -> Result<()> {
    record_list(value, &["status", "role"])?;
    Ok(())
}
pub(crate) fn lead(value: &Value) -> Result<()> {
    if matches!(value, Value::Null) {
        return Ok(());
    }
    let object = value.as_object().ok_or(Error("invalid_lead_record"))?;
    identifier(required(object, "agent")?)?;
    optional_text(object, &["set_at", "until", "instructions"])
}
