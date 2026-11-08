//! Explicit CLI boundaries; never infer a workspace or load runtime configuration.
use crate::{
    Error, Result, groups,
    values::{self, Value},
};
use std::path::Path;

mod observation;

pub use observation::{AddRequest, add, observe_kitsune, observe_live, publish, status};

pub struct PublicCommandResult {
    /// Complete JSON body without a terminal LF; the CLI supplies exactly one.
    pub body: String,
    pub exit_code: u8,
}

fn render(value: &Value, json: bool, exit_code: u8) -> Result<PublicCommandResult> {
    let bytes = if json {
        value.compact()?
    } else {
        value.storage()?
    };
    let text = std::str::from_utf8(&bytes)?;
    Ok(PublicCommandResult {
        body: crate::output::ascii_json(text.strip_suffix('\n').unwrap_or(text))?,
        exit_code,
    })
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_object()?.get(name)
}

fn mutation(value: Value, json: bool) -> Result<PublicCommandResult> {
    let uncertain = field(&value, "delivery")
        .and_then(|v| field(v, "outcome"))
        .and_then(Value::as_str)
        == Some("durability_uncertain");
    render(&value, json, if uncertain { 2 } else { 0 })
}

// Only bounded in-process observation reports cross this codec, never authority.
fn report(value: &serde_json::Value, json: bool, code: u8) -> Result<PublicCommandResult> {
    render(
        &values::parse(&serde_json::to_vec(value)?, values::Profile::Authority)?,
        json,
        code,
    )
}

pub fn create(
    workspace: &Path,
    group: &str,
    name: Option<&str>,
    expected: Option<&str>,
    json: bool,
) -> Result<PublicCommandResult> {
    mutation(
        groups::create_group(
            &crate::platform::workspace_argument(workspace)?,
            group,
            name.unwrap_or(group),
            expected,
        )?,
        json,
    )
}

pub fn remove(
    workspace: &Path,
    group: &str,
    member: &str,
    expected: Option<&str>,
    json: bool,
) -> Result<PublicCommandResult> {
    mutation(
        groups::remove_member(
            &crate::platform::workspace_argument(workspace)?,
            group,
            member,
            expected,
        )?,
        json,
    )
}

pub fn list(workspace: &Path, json: bool) -> Result<PublicCommandResult> {
    let view = groups::read_groups(&crate::platform::workspace_argument(workspace)?)?;
    let groups = match view.state() {
        Some(state) => field(state, "groups")
            .ok_or(Error("groups_malformed_authority"))?
            .clone(),
        None => Value::Array(vec![]),
    };
    let value = Value::object(vec![
        ("initialized".into(), Value::Bool(view.state().is_some())),
        ("revision".into(), Value::String(view.revision().into())),
        ("groups".into(), groups),
        ("authorization".into(), Value::String("none".into())),
        ("dispatchable".into(), Value::Bool(false)),
        (
            "native_lifecycle".into(),
            Value::String("unestablished".into()),
        ),
        (
            "job_completion".into(),
            Value::String("unestablished".into()),
        ),
    ])?;
    render(&value, json, 0)
}

pub fn attach(
    workspace: &Path,
    request: crate::group_live::AttachRequest<'_>,
    json: bool,
) -> Result<PublicCommandResult> {
    mutation(
        crate::group_live::attach(&crate::platform::workspace_argument(workspace)?, request)?,
        json,
    )
}

pub fn detach(
    workspace: &Path,
    group: &str,
    member: &str,
    expected: Option<&str>,
    json: bool,
) -> Result<PublicCommandResult> {
    mutation(
        crate::group_live::detach(
            &crate::platform::workspace_argument(workspace)?,
            group,
            member,
            expected,
        )?,
        json,
    )
}

#[cfg(test)]
mod tests;
