//! Native single-use launch writers. Historical readers retain their own profile.
//! Static managed-path checks coordinate cooperating local writers, not hostile edits.
//! New preparations capture at most 2 MiB total source bytes, 768 KiB per file;
//! bundles/control are capped at 4 MiB, plans at 128 KiB, claims at 128. Pending
//! outcomes reserve 128 KiB and 2048 nodes each. Checkpoints retain the 1 MiB
//! reader limit. These admission limits do not restrict historical lookup/stop.
use super::{CONTROL, MAX_CHECKPOINT, authority, object, parse_authority, read, string};
use crate::{
    Error, Result,
    error::require,
    platform,
    storage::{self, Delivery, WorkspaceGuard},
    values::{self, Profile, Value},
};
use authority::{Budget, ExpectedRefs, Lookup, ValidatedControl, get};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

mod capacity;
mod checkpoint;
mod inputs;
mod submission;
#[cfg(test)]
mod tests;

/// JSON body without a terminal newline, and the contract's process exit status.
#[derive(Debug)]
pub struct CommandResult {
    pub body: String,
    pub exit_code: u8,
}

const TIMEOUT: Duration = Duration::from_secs(5);

trait Io {
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        storage::atomic_replace_bytes(guard, Path::new(CONTROL), bytes)
    }
    fn confirm(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        let delivery = storage::confirm_existing(guard, Path::new(CONTROL), bytes)?;
        if delivery == Delivery::Committed && platform::sync_directory(guard.root()).is_err() {
            return Ok(Delivery::DurabilityUncertain);
        }
        Ok(delivery)
    }
    fn publish(&self, guard: &WorkspaceGuard, relative: &Path, bytes: &[u8]) -> Result<Delivery> {
        storage::publish_new(guard, relative, bytes)
    }
    fn execute(&self, plan: &Value) -> Value {
        crate::launch::execute(plan)
    }
    fn stop(&self, plan: &Value) -> Value {
        crate::launch::stop(plan)
    }
    fn history(&self, guard: &WorkspaceGuard, id: &str, claim: &Value) -> Result<()> {
        crate::task_delivery::append_event(
            guard.root(),
            "runtime_launch.transport.recorded",
            &object(vec![
                ("action_id", string(id)),
                ("claim_id", get(claim, "claim_id")?.clone()),
                ("status", get(claim, "status")?.clone()),
            ])?,
            guard,
        )
    }
}
struct SystemIo;
impl Io for SystemIo {}

fn root(workspace: &Path) -> Result<PathBuf> {
    platform::ensure_supported()?;
    let root = platform::workspace_argument(workspace)?.canonicalize()?;
    require(root.is_dir(), "invalid_action_workspace")?;
    root.to_str().ok_or(Error("non_unicode_path_unsupported"))?;
    Ok(root)
}

fn control(root: &Path) -> Result<Option<(Value, Vec<u8>)>> {
    let Some(raw) = read(
        root,
        Path::new(CONTROL),
        values::MAX_INPUT,
        "unsupported_action_input_profile",
    )?
    else {
        super::validate_workspace_control(root)?;
        return Ok(None);
    };
    let validated = ValidatedControl::validate(
        parse_authority(&raw)?,
        &platform::workspace_location(root)?,
        &mut Budget::default(),
    )?;
    Ok(Some((validated.value().clone(), raw)))
}
fn enabled(root: &Path) -> Result<(Value, Vec<u8>)> {
    control(root)?.ok_or(Error("runtime_launch_disabled"))
}
fn available(control: &Value) -> Result<()> {
    if let Some(id) = get(control, "head")?.as_str() {
        require(
            text(get(get(control, "claims")?, id)?, "status")? != "claimed",
            "action_unresolved_claim",
        )?;
    }
    Ok(())
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    get(value, field)?
        .as_str()
        .ok_or(Error("invalid_action_text"))
}
fn location(root: &Path) -> Result<Value> {
    let loc = platform::workspace_location(root)?;
    object(vec![
        ("root", string(loc.root)),
        ("device", Value::integer(&loc.device.to_string())?),
        ("inode", Value::integer(&loc.inode.to_string())?),
    ])
}
fn validated_bytes(root: &Path, value: &Value) -> Result<Vec<u8>> {
    let bytes = value.canonical()?;
    require(
        bytes.len() <= values::MAX_INPUT,
        "unsupported_action_input_profile",
    )?;
    ValidatedControl::validate(
        values::parse(&bytes, Profile::Authority)?,
        &platform::workspace_location(root)?,
        &mut Budget::default(),
    )?;
    Ok(bytes)
}
fn replace_claim(control: &mut Value, id: &str, claim: Value) -> Result<()> {
    let mut claims = get(control, "claims")?.clone();
    claims.set_field(id, claim)?;
    control.set_field("claims", claims)
}
fn render(value: &Value, json: bool) -> Result<CommandResult> {
    let exit_code = match text(value, "outcome")? {
        "claimed" | "outcome_unknown" | "durability_uncertain" => 2,
        "transport_failed_or_partial" => 1,
        _ => 0,
    };
    let bytes = if json {
        value.compact()?
    } else {
        value.storage()?
    };
    Ok(CommandResult {
        body: std::str::from_utf8(&bytes)?
            .trim_end_matches('\n')
            .to_owned(),
        exit_code,
    })
}
fn uncertain(field: &str, value: Value, json: bool) -> Result<CommandResult> {
    render(
        &object(vec![
            ("outcome", string("durability_uncertain")),
            (field, value),
            ("replayed", Value::Bool(true)),
            (
                "warnings",
                Value::Array(vec![string(
                    "Control durability uncertain; no transport attempted. Never automatically relaunch a consumed action.",
                )]),
            ),
        ])?,
        json,
    )
}

/// Drain admitted cooperating legacy effects before publishing persistent mode.
pub fn enable(workspace: &Path, json: bool) -> Result<CommandResult> {
    let root = root(workspace)?;
    enable_with(&root, json, &SystemIo)
}
fn enable_with(root: &Path, json: bool, io: &impl Io) -> Result<CommandResult> {
    crate::workspace::load(root)?;
    let gate = crate::transport_gate::TransportGuard::exclusive(root, TIMEOUT)?;
    gate.validate_for(root, true)?;
    let guard = WorkspaceGuard::acquire(gate.root(), TIMEOUT)?;
    crate::workspace::load(root)?;
    let (control, bytes) = if let Some(existing) = control(root)? {
        existing
    } else {
        let control = object(vec![
            ("version", Value::integer("1")?),
            ("action", string("runtime_launch")),
            ("workspace_id", string(platform::random_uuid_hex()?)),
            ("epoch_id", string(platform::random_uuid_hex()?)),
            ("location", location(root)?),
            ("head", Value::Null),
            ("claims", object(vec![])?),
        ])?;
        let bytes = validated_bytes(root, &control)?;
        if io.replace(&guard, &bytes)? == Delivery::DurabilityUncertain {
            return uncertain("control", control, json);
        }
        (control, bytes)
    };
    if !matches!(io.confirm(&guard, &bytes), Ok(Delivery::Committed)) {
        return uncertain("control", control, json);
    }
    render(
        &object(vec![
            ("outcome", string("enabled")),
            ("control", control),
            (
                "warning",
                string(
                    "Legacy launches and pane delivery are disabled; this mode does not sandbox runtime tools",
                ),
            ),
        ])?,
        json,
    )
}

pub fn prepare(workspace: &Path, json: bool) -> Result<CommandResult> {
    prepare_with(&root(workspace)?, json, &SystemIo)
}
fn prepare_with(root: &Path, json: bool, io: &impl Io) -> Result<CommandResult> {
    let guard = WorkspaceGuard::acquire(root, TIMEOUT)?;
    let (control, _) = enabled(root)?;
    available(&control)?;
    let id = platform::random_uuid_hex()?;
    let captured = inputs::capture(root)?;
    let plan = captured.plan(&control, &id)?;
    capacity::admit(&control, &id, &plan)?;
    capacity::checkpoint_slot(root)?;
    let bundle = object(vec![
        ("version", Value::integer("1")?),
        ("action", string("runtime_launch")),
        ("action_id", string(&id)),
        ("workspace_id", get(&control, "workspace_id")?.clone()),
        ("epoch_id", get(&control, "epoch_id")?.clone()),
        ("location", get(&control, "location")?.clone()),
        (
            "control_revision",
            string(authority::control_revision(
                &control,
                &mut Budget::default(),
            )?),
        ),
        ("inputs", captured.inventory),
        ("plan", plan),
    ])?;
    let manifest = authority::bundle(&bundle, &id, &mut Budget::default())?;
    let bytes = bundle.canonical()?;
    require(
        bytes.len() <= capacity::MAX_BUNDLE,
        "unsupported_action_writer_capacity",
    )?;
    // Round-trip before publication: every byte we publish must fit native readers.
    parse_authority(&bytes)?;
    let cp = format!("runtime-launch-{id}");
    let pending = checkpoint::bytes(&bundle)?;
    let view = crate::checkpoints::inspect_bytes(&cp, Some(&pending))?;
    let response = object(vec![
        ("outcome", string("prepared")),
        ("action_id", string(&id)),
        ("manifest_digest", string(manifest)),
        ("checkpoint", string(&cp)),
        ("checkpoint_revision", get(&view, "revision")?.clone()),
        (
            "warning",
            string(
                "Unrestricted cold runtime startup; not read-only work, dispatch, or worker acceptance",
            ),
        ),
    ])?;
    let output = render(&response, json)?;
    storage::prepare_directory(&guard, Path::new("state/actions"))?;
    require(
        io.publish(
            &guard,
            Path::new(&format!("state/actions/{id}.json")),
            &bytes,
        )? == Delivery::Committed,
        "action_bundle_durability_uncertain",
    )?;
    storage::prepare_directory(&guard, Path::new("checkpoints"))?;
    require(
        io.publish(&guard, Path::new(&format!("checkpoints/{cp}.md")), &pending)?
            == Delivery::Committed,
        "action_checkpoint_durability_uncertain",
    )?;
    Ok(output)
}

pub fn submit(
    workspace: &Path,
    action_id: &str,
    manifest: &str,
    checkpoint: &str,
    decision: &str,
    json: bool,
) -> Result<CommandResult> {
    submission::submit(
        &root(workspace)?,
        action_id,
        manifest,
        checkpoint,
        decision,
        json,
        &SystemIo,
    )
}

pub fn acknowledge(
    workspace: &Path,
    action_id: &str,
    claim: &str,
    json: bool,
) -> Result<CommandResult> {
    acknowledge_with(&root(workspace)?, action_id, claim, json, &SystemIo)
}
fn acknowledge_with(
    root: &Path,
    id: &str,
    expected: &str,
    json: bool,
    io: &impl Io,
) -> Result<CommandResult> {
    authority::action_id(id)?;
    authority::action_id(expected)?;
    let guard = WorkspaceGuard::execution_maintenance(root, TIMEOUT)?;
    let (mut control, _) = enabled(root)?;
    let mut claim = get(get(&control, "claims")?, id)?.clone();
    require(
        text(&claim, "claim_id")? == expected && text(&claim, "status")? == "claimed",
        "action_not_identified_unresolved_claim",
    )?;
    claim.set_field("status", string("outcome_unknown"))?;
    claim.set_field("operator_acknowledged", Value::Bool(true))?;
    replace_claim(&mut control, id, claim.clone())?;
    let bytes = validated_bytes(root, &control)?;
    let output = render(
        &object(vec![
            ("outcome", string("acknowledged")),
            ("claim", claim.clone()),
            (
                "warning",
                string(
                    "No process was proven stopped; only a separately prepared and approved new action may launch again",
                ),
            ),
        ])?,
        json,
    )?;
    if io.replace(&guard, &bytes)? == Delivery::DurabilityUncertain {
        return uncertain("claim", claim, json);
    }
    Ok(output)
}

pub fn stop(workspace: &Path, action_id: &str, json: bool) -> Result<CommandResult> {
    stop_with(&root(workspace)?, Some(action_id), json, &SystemIo)
}
pub fn stop_current(workspace: &Path) -> Result<CommandResult> {
    stop_with(&root(workspace)?, None, true, &SystemIo)
}
fn stop_with(
    root: &Path,
    selected: Option<&str>,
    json: bool,
    io: &impl Io,
) -> Result<CommandResult> {
    if let Some(id) = selected {
        authority::action_id(id)?;
    }
    let guard = WorkspaceGuard::execution_maintenance(root, TIMEOUT)?;
    let Some((control, _)) = control(root)? else {
        require(selected.is_none(), "action_has_no_consumed_claim")?;
        return render(
            &object(vec![
                ("outcome", string("transport_completed")),
                (
                    "note",
                    string("No scoped launch attempt recorded; no transport sent"),
                ),
            ])?,
            json,
        );
    };
    let id = selected.or(get(&control, "head")?.as_str());
    let Some(id) = id else {
        return render(
            &object(vec![
                ("outcome", string("transport_completed")),
                (
                    "note",
                    string("No scoped launch attempt recorded; no transport sent"),
                ),
            ])?,
            json,
        );
    };
    let claim = get(get(&control, "claims")?, id)?.clone();
    drop(guard);
    let report = io.stop(get(&claim, "plan")?);
    render(
        &object(vec![
            ("outcome", get(&report, "status")?.clone()),
            ("action_id", string(id)),
            ("transport_report", report),
            (
                "warning",
                string(
                    "Operator stop is not evidence of native worker termination and does not reset launch consumption",
                ),
            ),
        ])?,
        json,
    )
}
