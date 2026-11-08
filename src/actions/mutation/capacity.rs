//! Writer-only admission: <=128 claims, <=128 KiB canonical plans, <=4 MiB
//! bundles and projected control, with 128 KiB/2048 nodes reserved per pending
//! outcome. Readers, known retries, acknowledgement and stop are not tightened.
use super::*;
pub(super) const MAX_BUNDLE: usize = 4 * 1_048_576;
pub(super) const REPORT_RESERVE: usize = 128 * 1024;
const MAX_CONTROL: usize = 4 * 1_048_576;
const MAX_PLAN: usize = 128 * 1024;
const MAX_CLAIMS: usize = 128;

pub(super) fn claim(
    id: &str,
    manifest: &str,
    checkpoint: &str,
    decision: &str,
    scope: &str,
    plan: Value,
) -> Result<Value> {
    object(vec![
        ("claim_id", string(platform::random_uuid_hex()?)),
        ("action_id", string(id)),
        ("manifest_digest", string(manifest)),
        ("checkpoint_revision", string(checkpoint)),
        ("decision_id", string(decision)),
        ("scope_revision", string(scope)),
        ("plan", plan),
        ("status", string("claimed")),
        ("report", Value::Null),
        ("operator_acknowledged", Value::Bool(false)),
    ])
}
pub(super) fn admit(control: &Value, id: &str, plan: &Value) -> Result<()> {
    require(
        plan.canonical()?.len() <= MAX_PLAN,
        "unsupported_action_writer_capacity",
    )?;
    let mut candidate = control.clone();
    let claims = get(control, "claims")?
        .as_object()
        .ok_or(Error("invalid_action_claims"))?;
    require(
        claims.entries().len() < MAX_CLAIMS && claims.get(id).is_none(),
        "unsupported_action_writer_capacity",
    )?;
    let claim = claim(
        id,
        &format!("runtime-launch-manifest-v1:{}", "0".repeat(64)),
        &format!("checkpoint-snapshot-v1:{}", "0".repeat(64)),
        &"0".repeat(32),
        &format!("checkpoint-scope-v1:{}", "0".repeat(64)),
        plan.clone(),
    )?;
    replace_claim(&mut candidate, id, claim)?;
    candidate.set_field("head", string(id))?;
    headroom(&candidate)
}
pub(super) fn checkpoint_slot(root: &Path) -> Result<()> {
    let directory = crate::read_fs::managed(root, Path::new("checkpoints"), true)?;
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for (index, entry) in entries.enumerate() {
        entry?;
        require(
            index < 4095,
            "unsupported_action_writer_checkpoint_capacity",
        )?;
    }
    Ok(())
}
pub(super) fn report(value: &Value) -> Result<()> {
    require(
        value.canonical()?.len() <= REPORT_RESERVE && nodes(value) <= 2048,
        "unsupported_action_report_writer_capacity",
    )
}
fn nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(items) => items.iter().map(nodes).sum(),
        Value::Object(object) => object.entries().iter().map(|(_, value)| nodes(value)).sum(),
        _ => 0,
    }
}
fn headroom(control: &Value) -> Result<()> {
    let pending = get(control, "claims")?
        .as_object()
        .ok_or(Error("invalid_action_claims"))?
        .entries()
        .iter()
        .filter(|(_, claim)| get(claim, "report").is_ok_and(|v| matches!(v, Value::Null)))
        .count();
    require(
        control.canonical()?.len() + pending * (REPORT_RESERVE + 64) <= MAX_CONTROL
            && nodes(control) + pending * 2048 + 4096 <= values::MAX_NODES,
        "unsupported_action_writer_capacity",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_byte_headroom_boundary_reserves_late_outcomes_and_acknowledgement() {
        let temp = super::super::tests::activated();
        let root = temp.path();
        let (mut control, _) = enabled(root).unwrap();
        let first = "11111111111111111111111111111111";
        let second = "22222222222222222222222222222222";
        let captured = inputs::capture(root).unwrap();
        let plan = captured.plan(&control, first).unwrap();
        let mut historical = claim(
            first,
            &format!("runtime-launch-manifest-v1:{}", "0".repeat(64)),
            &format!("checkpoint-snapshot-v1:{}", "0".repeat(64)),
            &"0".repeat(32),
            &format!("checkpoint-scope-v1:{}", "0".repeat(64)),
            plan,
        )
        .unwrap();
        historical
            .set_field("status", string("transport_completed"))
            .unwrap();
        let mut report = super::super::tests::report(false);
        // Each historical string remains within the reader's 1 MiB string limit.
        let diagnostic = |n| {
            object(vec![
                ("step", Value::Null),
                ("error", string("")),
                ("message", string("x".repeat(n))),
            ])
            .unwrap()
        };
        report
            .set_field("diagnostics", Value::Array(vec![diagnostic(1_010_000); 3]))
            .unwrap();
        historical.set_field("report", report).unwrap();
        replace_claim(&mut control, first, historical).unwrap();
        control.set_field("head", string(first)).unwrap();
        let plan = captured.plan(&control, second).unwrap();
        let future = claim(
            second,
            &format!("runtime-launch-manifest-v1:{}", "0".repeat(64)),
            &format!("checkpoint-snapshot-v1:{}", "0".repeat(64)),
            &"0".repeat(32),
            &format!("checkpoint-scope-v1:{}", "0".repeat(64)),
            plan.clone(),
        )
        .unwrap();
        let mut projected = control.clone();
        replace_claim(&mut projected, second, future).unwrap();
        projected.set_field("head", string(second)).unwrap();
        let base = projected.canonical().unwrap().len();
        let remaining = MAX_CONTROL - (REPORT_RESERVE + 64) - base;
        assert!(remaining < values::MAX_STRING);
        // Adding one diagnostic has 39 bytes of JSON structure, plus its comma.
        let mut historical = get(get(&control, "claims").unwrap(), first)
            .unwrap()
            .clone();
        let mut report = get(&historical, "report").unwrap().clone();
        let mut diagnostics = get(&report, "diagnostics")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        let overhead = diagnostic(0).canonical().unwrap().len() + 1;
        diagnostics.push(diagnostic(remaining - overhead));
        report
            .set_field("diagnostics", Value::Array(diagnostics.clone()))
            .unwrap();
        historical.set_field("report", report.clone()).unwrap();
        replace_claim(&mut control, first, historical.clone()).unwrap();
        admit(&control, second, &plan).unwrap();
        diagnostics
            .last_mut()
            .unwrap()
            .set_field("message", string("x".repeat(remaining - overhead + 1)))
            .unwrap();
        report
            .set_field("diagnostics", Value::Array(diagnostics))
            .unwrap();
        historical.set_field("report", report).unwrap();
        replace_claim(&mut control, first, historical).unwrap();
        assert_eq!(
            admit(&control, second, &plan).unwrap_err().code(),
            "unsupported_action_writer_capacity"
        );
        // A native-readable historical control can be too full for new writers.
        validated_bytes(root, &control).unwrap();
        let guard = WorkspaceGuard::acquire(root, TIMEOUT).unwrap();
        storage::atomic_replace_bytes(&guard, Path::new(CONTROL), &control.canonical().unwrap())
            .unwrap();
        drop(guard);
        assert!(super::super::super::inspect(root, first).is_ok());
        assert!(
            super::super::prepare_with(root, true, &super::super::tests::Fake::default()).is_err()
        );
        assert_eq!(
            super::super::stop_with(
                root,
                Some(first),
                true,
                &super::super::tests::Fake::default()
            )
            .unwrap()
            .exit_code,
            0
        );
    }
}
