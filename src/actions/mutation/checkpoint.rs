use super::*;

// Python's sort_keys=True, ensure_ascii=True, indent=2, without routing numbers
// through serde_json (which would narrow historical arbitrary-precision values).
fn sorted(value: &Value) -> Result<Value> {
    Ok(match value {
        Value::Object(object) => {
            let mut entries = object.entries().iter().collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::object(
                entries
                    .into_iter()
                    .map(|(k, v)| Ok((k.clone(), sorted(v)?)))
                    .collect::<Result<_>>()?,
            )?
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect::<Result<_>>()?),
        value => value.clone(),
    })
}
pub(super) fn bytes(bundle: &Value) -> Result<Vec<u8>> {
    let id = text(bundle, "action_id")?;
    let plan = get(bundle, "plan")?;
    let summary = object(vec![
        ("action", get(bundle, "action")?.clone()),
        ("action_id", string(id)),
        ("workspace_id", get(bundle, "workspace_id")?.clone()),
        ("epoch_id", get(bundle, "epoch_id")?.clone()),
        (
            "manifest_digest",
            string(authority::bundle(bundle, id, &mut Budget::default())?),
        ),
        ("location", get(bundle, "location")?.clone()),
        ("runtime", get(plan, "multiplexer")?.clone()),
        ("namespace", get(plan, "namespace")?.clone()),
        ("roster", get(plan, "agents")?.clone()),
        ("commands", get(plan, "steps")?.clone()),
    ])?;
    let pretty = sorted(&summary)?.storage()?;
    let encoded = crate::output::ascii_json(std::str::from_utf8(&pretty)?.trim_end_matches('\n'))?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('`', "\\u0060");
    let bytes = format!("# Runtime Launch Review\nStatus: pending\n\n## Covered Action\nWARNING: This authorizes one unrestricted COLD runtime launch, not a read-only sandbox, task dispatch, native worker acceptance, or readiness guarantee. Other authorized runtimes may still be running.\n\n```json\n{encoded}\n```\n\nInspect the immutable manifest with `coven action inspect {id}` before approving. Sources are provenance, not authentication.\n\n## User Decision\n- [ ] Approved\n- [ ] Rejected / needs changes\n").into_bytes();
    require(
        bytes.len() <= MAX_CHECKPOINT,
        "unsupported_checkpoint_input_profile",
    )?;
    Ok(bytes)
}
