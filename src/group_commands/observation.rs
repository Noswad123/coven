use super::*;
use crate::{
    kitsune_observation as kitsune,
    observation_socket::{NoCancel, ReadBudget},
};
use std::time::{Duration, Instant};

pub struct AddRequest<'a> {
    pub group: &'a str,
    pub member: &'a str,
    pub role: &'a str,
    pub socket: &'a str,
    pub pane: &'a str,
    pub process_evidence: bool,
    pub rebind: bool,
    pub expected_revision: Option<&'a str>,
}

pub fn add(workspace: &Path, request: AddRequest<'_>, json: bool) -> Result<PublicCommandResult> {
    let workspace = crate::platform::workspace_argument(workspace)?;
    groups::selector(request.group)?;
    groups::selector(request.member)?;
    groups::display(request.role)?;
    crate::error::require(
        !request.pane.is_empty()
            && request.pane.len() <= 4096
            && request
                .pane
                .chars()
                .all(|c| crate::unicode::printable(c) && !c.is_whitespace()),
        "groups_invalid_pane",
    )?;
    let view = groups::read_groups(&workspace)?;
    crate::error::require(
        request
            .expected_revision
            .is_none_or(|r| r == view.revision()),
        "groups_revision_conflict",
    )?;
    view.group(request.group)?;
    // The local read holds no mutation lock. All source I/O shares one deadline/budget.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut budget = ReadBudget::new(1_048_576);
    let binding = if request.process_evidence {
        kitsune::binding_from_process(
            kitsune::ProcessRequest {
                socket: request.socket,
                pane: request.pane,
                deadline,
                frozen_endpoint: None,
                expected_terminal: None,
            },
            &mut budget,
            &NoCancel,
        )
    } else {
        kitsune::read_publication_snapshot(request.socket, deadline, &mut budget, false, &NoCancel)
            .and_then(|snapshot| kitsune::binding_from_snapshot(&snapshot, request.pane))
    }
    .map_err(|_| Error("groups_observation_failed"))?;
    let binding = fresh_binding(&binding)?;
    mutation(
        groups::add_member(
            &workspace,
            groups::AddMember {
                group: request.group,
                member: request.member,
                role: request.role,
                binding: &binding,
                rebind: request.rebind,
                expected_revision: request.expected_revision,
            },
        )?,
        json,
    )
}

pub(super) fn fresh_binding(produced: &serde_json::Value) -> Result<Value> {
    fn ordered(value: &Value, names: &str) -> Result<Value> {
        Value::object(
            names
                .split_whitespace()
                .map(|name| {
                    Ok((
                        name.to_owned(),
                        field(value, name)
                            .ok_or(Error("groups_invalid_binding"))?
                            .clone(),
                    ))
                })
                .collect::<Result<_>>()?,
        )
    }

    // This is only the bounded fresh producer boundary, never stored authority.
    // Validate the closed shape before restoring Python's insertion order.
    let binding = values::parse(&serde_json::to_vec(produced)?, values::Profile::Authority)?;
    groups::validate_binding(&binding)?;
    let mut binding = ordered(&binding, "presentation evidence")?;
    let mut presentation = ordered(
        field(&binding, "presentation").ok_or(Error("groups_invalid_binding"))?,
        "adapter endpoint terminal_id pane_id_at_binding",
    )?;
    presentation.set_field(
        "endpoint",
        ordered(
            field(&presentation, "endpoint").ok_or(Error("groups_invalid_binding"))?,
            "path device inode",
        )?,
    )?;
    binding.set_field("presentation", presentation)?;
    let evidence = field(&binding, "evidence").ok_or(Error("groups_invalid_binding"))?;
    let process = field(evidence, "process_evidence");
    let mut evidence = ordered(
        evidence,
        if process.is_some() {
            "provenance observed_at agent_kind_hint native_reference native_binding process_evidence"
        } else {
            "provenance observed_at agent_kind_hint native_reference native_binding"
        },
    )?;
    let native = field(&evidence, "native_reference").ok_or(Error("groups_invalid_binding"))?;
    if !matches!(native, Value::Null) {
        evidence.set_field(
            "native_reference",
            ordered(native, "source agent kind value")?,
        )?;
    }
    if let Some(process) = process {
        let mut process = ordered(process, "interpreter matches")?;
        let matches = field(&process, "matches")
            .and_then(Value::as_array)
            .ok_or(Error("groups_invalid_binding"))?
            .iter()
            .map(|item| ordered(item, "pid executable_family rule"))
            .collect::<Result<Vec<_>>>()?;
        process.set_field("matches", Value::Array(matches))?;
        evidence.set_field("process_evidence", process)?;
    }
    binding.set_field("evidence", evidence)?;
    Ok(binding)
}

pub fn status(
    workspace: &Path,
    group: &str,
    member: Option<&str>,
    timeout: f64,
    json: bool,
) -> Result<PublicCommandResult> {
    let value = crate::group_live::status(
        &crate::platform::workspace_argument(workspace)?,
        group,
        member,
        timeout,
    )?;
    report(
        &value,
        json,
        if value["assessment"] == "observed" {
            0
        } else {
            1
        },
    )
}

fn observed(value: serde_json::Value, json: bool) -> Result<PublicCommandResult> {
    let code = match value["assessment"].as_str() {
        Some("observed") => 0,
        Some("incompatible") => 2,
        _ => 1,
    };
    report(&value, json, code)
}

pub fn observe_kitsune(request: &kitsune::Request, json: bool) -> Result<PublicCommandResult> {
    observed(kitsune::inspect(request)?, json)
}

pub fn observe_live(
    request: &crate::djinn_live_observation::Request,
    json: bool,
) -> Result<PublicCommandResult> {
    observed(crate::djinn_live_observation::inspect(request)?, json)
}

pub fn publish(
    request: &crate::group_publisher::Request,
    json: bool,
) -> Result<PublicCommandResult> {
    let request = crate::group_publisher::Request {
        workspace: crate::platform::workspace_argument(&request.workspace)?,
        group: request.group.clone(),
        socket: request.socket.clone(),
        source: request.source.clone(),
        duration: request.duration,
        live: request.live,
    };
    let value = crate::group_publisher::publish(&request)?;
    let code = match value["outcome"].as_str() {
        Some("completed") => 0,
        Some("uncertain") => 2,
        Some("interrupted") => 130,
        _ => 1,
    };
    report(&value, json, code)
}
