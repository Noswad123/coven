use super::*;

// Not Clone, not public, and constructed only by a fresh committed replacement.
// Neither historical lookup nor confirmation can mint execution permission.
struct ExecutionPermit {
    claim: Value,
}
enum Admission {
    Existing(CommandResult),
    Fresh(ExecutionPermit),
}

fn result(claim: Value, replayed: bool, json: bool) -> Result<CommandResult> {
    render(
        &object(vec![
            ("outcome", get(&claim, "status")?.clone()),
            ("replayed", Value::Bool(replayed)),
            ("claim", claim),
            ("warnings", Value::Array(vec![])),
        ])?,
        json,
    )
}
fn admit(
    root: &Path,
    id: &str,
    manifest: &str,
    checkpoint_ref: &str,
    decision: &str,
    json: bool,
    io: &impl Io,
) -> Result<Admission> {
    authority::action_id(id)?;
    let expected = ExpectedRefs::new(manifest, checkpoint_ref, decision)?;
    let guard = WorkspaceGuard::execution_maintenance(root, TIMEOUT)?;
    let (mut control, raw) = enabled(root)?;
    let validated = ValidatedControl::validate(
        control.clone(),
        &platform::workspace_location(root)?,
        &mut Budget::default(),
    )?;
    match validated.lookup(id, Some(&expected))? {
        Lookup::Match(claim) => {
            let output = result(claim.clone(), true, json)?;
            return Ok(Admission::Existing(
                if matches!(io.confirm(&guard, &raw), Ok(Delivery::Committed)) {
                    output
                } else {
                    uncertain("claim", claim.clone(), json)?
                },
            ));
        }
        Lookup::Conflict(_) => return Err(Error("action_consumed_references_conflict")),
        Lookup::Absent => {}
        Lookup::Existing(_) => return Err(Error("invalid_action_lookup")),
    }
    available(&control)?;
    let raw = read(
        root,
        Path::new(&format!("state/actions/{id}.json")),
        values::MAX_INPUT,
        "unsupported_action_input_profile",
    )?
    .ok_or(Error("action_bundle_missing"))?;
    let bundle = parse_authority(&raw)?;
    require(
        authority::bundle(&bundle, id, &mut Budget::default())? == manifest,
        "action_manifest_digest_mismatch",
    )?;
    for field in ["workspace_id", "epoch_id", "location"] {
        require(
            get(&bundle, field)?.canonical()? == get(&control, field)?.canonical()?,
            "action_bundle_activation_changed",
        )?;
    }
    require(
        text(&bundle, "control_revision")?
            == authority::control_revision(&control, &mut Budget::default())?,
        "action_bundle_control_stale",
    )?;
    let captured = inputs::capture(root)?;
    require(
        captured.inventory.canonical()? == get(&bundle, "inputs")?.canonical()?,
        "action_captured_inputs_changed",
    )?;
    require(
        captured.plan(&control, id)?.canonical()? == get(&bundle, "plan")?.canonical()?,
        "action_frozen_plan_inputs_changed",
    )?;
    capacity::admit(&control, id, get(&bundle, "plan")?)?;
    let cp = format!("runtime-launch-{id}");
    let approved = read(
        root,
        Path::new(&format!("checkpoints/{cp}.md")),
        MAX_CHECKPOINT,
        "unsupported_checkpoint_input_profile",
    )?
    .ok_or(Error("action_checkpoint_missing"))?;
    let view = crate::checkpoints::inspect_bytes(&cp, Some(&approved))?;
    let generated = crate::checkpoints::inspect_bytes(&cp, Some(&checkpoint::bytes(&bundle)?))?;
    let scope = text(&generated, "scope_revision")?;
    require(
        text(&view, "binding")? == "bound"
            && text(&view, "effective_status")? == "approved"
            && text(&view, "revision")? == checkpoint_ref
            && text(get(&view, "decision")?, "decision_id")? == decision
            && text(&view, "scope_revision")? == scope,
        "action_checkpoint_not_exact_bound_approval",
    )?;
    let claim = capacity::claim(
        id,
        manifest,
        checkpoint_ref,
        decision,
        scope,
        get(&bundle, "plan")?.clone(),
    )?;
    replace_claim(&mut control, id, claim.clone())?;
    control.set_field("head", string(id))?;
    let bytes = validated_bytes(root, &control)?;
    // Pre-render both possible receipts before crossing the consumption boundary.
    let uncertain = uncertain("claim", claim.clone(), json)?;
    result(claim.clone(), true, json)?;
    if io.replace(&guard, &bytes)? == Delivery::DurabilityUncertain {
        return Ok(Admission::Existing(uncertain));
    }
    Ok(Admission::Fresh(ExecutionPermit { claim }))
}

pub(super) fn submit(
    root: &Path,
    id: &str,
    manifest: &str,
    checkpoint: &str,
    decision: &str,
    json: bool,
    io: &impl Io,
) -> Result<CommandResult> {
    let permit = match admit(root, id, manifest, checkpoint, decision, json, io)? {
        Admission::Existing(output) => return Ok(output),
        Admission::Fresh(permit) => permit,
    };
    execute(root, id, permit, json, io)
}
fn execute(
    root: &Path,
    id: &str,
    permit: ExecutionPermit,
    json: bool,
    io: &impl Io,
) -> Result<CommandResult> {
    // admit's WorkspaceGuard is dropped before this sole executor boundary.
    let report = io.execute(get(&permit.claim, "plan")?);
    let recorded = (|| -> Result<(Value, Vec<Value>)> {
        authority::report(&report, &mut Budget::default())?;
        capacity::report(&report)?;
        let guard = WorkspaceGuard::execution_maintenance(root, TIMEOUT)?;
        let (mut control, raw) = enabled(root)?;
        let mut claim = get(get(&control, "claims")?, id)?.clone();
        require(
            text(&claim, "claim_id")? == text(&permit.claim, "claim_id")?,
            "action_claim_identity_mismatch",
        )?;
        // Acknowledgement and a newer head may have won while transport ran.
        if matches!(
            text(&claim, "status")?,
            "transport_completed" | "transport_failed_or_partial"
        ) {
            require(
                get(&claim, "report")?.canonical()? == report.canonical()?,
                "action_terminal_outcome_conflict",
            )?;
            require(
                matches!(io.confirm(&guard, &raw), Ok(Delivery::Committed)),
                "action_outcome_durability_uncertain",
            )?;
        } else {
            claim.set_field("status", get(&report, "status")?.clone())?;
            claim.set_field("report", report.clone())?;
            replace_claim(&mut control, id, claim.clone())?;
            let bytes = validated_bytes(root, &control)?;
            require(
                io.replace(&guard, &bytes)? == Delivery::Committed,
                "action_outcome_durability_uncertain",
            )?;
        }
        let warnings = io
            .history(&guard, id, &claim)
            .err()
            .map(|error| {
                string(format!(
                    "Outcome committed; history incomplete: {}",
                    error.code()
                ))
            })
            .into_iter()
            .collect();
        Ok((claim, warnings))
    })();
    match recorded {
        Ok((claim, warnings)) => render(
            &object(vec![
                ("outcome", get(&claim, "status")?.clone()),
                ("replayed", Value::Bool(false)),
                ("claim", claim),
                ("warnings", Value::Array(warnings)),
            ])?,
            json,
        ),
        Err(error) => render(
            &object(vec![
                ("outcome", string("outcome_unknown")),
                ("replayed", Value::Bool(false)),
                ("claim", permit.claim),
                ("transport_report", report),
                (
                    "warnings",
                    Value::Array(vec![string(format!(
                        "Transport may have run; durable outcome recording failed: {}. Inspect, never relaunch this action.",
                        error.code()
                    ))]),
                ),
            ])?,
            json,
        ),
    }
}
