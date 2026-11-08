use super::*;

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .as_object()
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .ok_or(Error("invalid_checkpoint_result"))
}

fn block(kind: &str, value: &Value) -> Result<Vec<u8>> {
    let canonical = value.canonical()?;
    let payload = crate::output::ascii_json(std::str::from_utf8(&canonical)?)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    Ok(format!("<!-- coven-checkpoint-{kind}:v1\n{payload}\n-->\n").into_bytes())
}

/// Pure Python-compatible writer. Neither input nor generated output may exceed
/// the native reader's 1 MiB profile; refusal precedes all workspace effects.
// Preserve the shared action-store API rather than introducing a second request type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    checkpoint: &str,
    data: Option<&[u8]>,
    action: &str,
    notes: &str,
    decision_id: &str,
    decided_at: &str,
    source: &str,
    initial: Option<&[u8]>,
) -> Result<(Vec<u8>, Value)> {
    let before = inspect_bytes(checkpoint, data)?;
    require(
        notes.len() <= MAX_INPUT,
        "unsupported_checkpoint_output_profile",
    )?;
    let original = data
        .or(initial)
        .ok_or(Error("missing_checkpoint_initial_content"))?;
    let base = inspect_bytes(checkpoint, Some(original))?;
    let mut decision = Value::object(vec![("version".into(), Value::integer("1")?)])?;
    for (key, text) in [
        ("checkpoint", checkpoint),
        ("decision_id", decision_id),
        ("action", action),
        ("scope_revision", field(&base, "scope_revision")?),
        ("reviewed_revision", field(&before, "revision")?),
        ("decided_at", decided_at),
        ("source", source),
    ] {
        decision.set_field(key, Value::String(text.into()))?;
    }
    record(&decision, checkpoint, false)?;
    let mut history = decision.clone();
    history.set_field("notes", Value::String(notes.into()))?;
    record(&history, checkpoint, true)?;
    let scan = scan(original, checkpoint)?;
    require(
        !scan
            .history
            .iter()
            .any(|item| field(item, "decision_id").ok() == Some(decision_id)),
        "invalid_checkpoint_duplicate_history",
    )?;
    let header_end = scan
        .visible
        .iter()
        .find(|line| heading(line.text).is_some_and(|(level, _)| level == 2))
        .map_or(original.len(), |line| line.start);
    let mut replacements = Vec::new();
    if scan.decision.is_some() {
        let &(start, end) = scan
            .excluded
            .iter()
            .find(|&&(start, end)| {
                original[start..end].starts_with(b"<!-- coven-checkpoint-decision:v1")
            })
            .ok_or(Error("invalid_checkpoint_decision_position"))?;
        replacements.push((start, end, Vec::new()));
    }
    let mut has_status = false;
    for line in &scan.visible {
        if line.start < header_end
            && unindent(line.text).is_some_and(|s| {
                s.get(..7)
                    .is_some_and(|s| s.eq_ignore_ascii_case(b"status:"))
            })
        {
            has_status = true;
            let mut replacement = format!("Status: {action}").into_bytes();
            replacement.extend_from_slice(&original[line.start + line.text.len()..line.end]);
            replacements.push((line.start, line.end, replacement));
        }
    }
    if let Some(section) = scan
        .visible
        .iter()
        .find(|line| heading(line.text) == Some((2, b"user decision".to_vec())))
    {
        let stop = scan
            .visible
            .iter()
            .find(|line| line.start >= section.end && heading(line.text).is_some())
            .map_or(original.len(), |line| line.start);
        for line in &scan.visible {
            if line.start < section.end || line.start >= stop {
                continue;
            }
            for (label, approved) in [
                (b"Approved".as_slice(), true),
                (b"Rejected / needs changes".as_slice(), false),
            ] {
                if checkbox(line.text, label).is_some() {
                    let mut replacement = original[line.start..line.end].to_vec();
                    let at = replacement
                        .iter()
                        .position(|c| *c == b'[')
                        .ok_or(Error("invalid_checkpoint_status"))?;
                    replacement[at + 1] = if approved == (action == "approved") {
                        b'x'
                    } else {
                        b' '
                    };
                    replacements.push((line.start, line.end, replacement));
                }
            }
        }
    }
    let mut result = original.to_vec();
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    for (start, end, replacement) in replacements {
        result.splice(start..end, replacement);
    }
    let mut output = block("decision", &decision)?;
    if !has_status {
        output.extend_from_slice(format!("Status: {action}\n").as_bytes());
    }
    output.extend_from_slice(&result);
    output.push(b'\n');
    output.extend_from_slice(&block("history", &history)?);
    require(
        output.len() <= MAX_INPUT,
        "unsupported_checkpoint_output_profile",
    )?;
    let final_view = inspect_bytes(checkpoint, Some(&output))?;
    require(
        field(&final_view, "scope_revision")? == field(&base, "scope_revision")?,
        "checkpoint_scope_changed",
    )?;
    require(
        field(&final_view, "effective_status")? == action
            && final_view
                .as_object()
                .and_then(|o| o.get("decision"))
                .map(Value::canonical)
                .transpose()?
                == Some(decision.canonical()?),
        "checkpoint_prepared_decision_mismatch",
    )?;
    Ok((output, final_view))
}

#[cfg(test)]
mod tests;
