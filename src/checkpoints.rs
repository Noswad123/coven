//! Pure, exact-byte checkpoint inspection. Bindings are not authentication or
//! permission to approve. No linked targets, workspace state, or files are read.
//!
//! Native profile: at most 1 MiB of input and UTF-8 scalar text. Authority JSON
//! is bounded to depth 64, 262144 nodes, 16384 object members, 65536 array items,
//! 1 MiB strings, 8192-byte number tokens and 4300 integer digits (`values`).
//! Machine records are closed; decided_at is arbitrary scalar text, not a date.
use crate::{
    Error, Result,
    error::require,
    records::trimmed,
    values::{self, Profile, Value},
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MAX_INPUT: usize = 1_048_576;
type Span = (usize, usize);

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}

fn hash(kind: &str, checkpoint: &str, data: &[u8]) -> String {
    let domain = format!("checkpoint-{kind}-v1");
    let mut digest = Sha256::new();
    for part in [domain.as_bytes(), checkpoint.as_bytes(), data] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{domain}:{:x}", digest.finalize())
}

fn space(c: &u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 11 | 12)
}
fn blank(line: &[u8]) -> bool {
    line.iter().all(space)
}
fn horizontal(c: &u8) -> bool {
    matches!(c, b' ' | b'\t')
}
fn unindent(line: &[u8]) -> Option<&[u8]> {
    let indent = line.iter().take_while(|c| **c == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

fn heading(line: &[u8]) -> Option<(usize, Vec<u8>)> {
    let line = unindent(line)?;
    let level = line.iter().take_while(|c| **c == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    let spaces = rest.iter().take_while(|c| horizontal(c)).count();
    if spaces == 0 || rest.len() < 2 {
        return None;
    }
    // Regex [ \t]+ is greedy but must leave one byte for (.+?).
    let rest = &rest[spaces.min(rest.len() - 1)..];
    let end = rest
        .iter()
        .rposition(|c| !horizontal(c))
        .map_or(1, |i| i + 1);
    let mut title = &rest[..end];
    let hashes = title.iter().rev().take_while(|c| **c == b'#').count();
    if hashes > 0 {
        let before = &title[..title.len() - hashes];
        let spaces = before.iter().rev().take_while(|c| horizontal(c)).count();
        if spaces > 0 {
            title = &before[..before.len() - spaces];
        }
    }
    Some((level, title.to_ascii_lowercase()))
}

fn fence_marker(line: &[u8]) -> Option<(u8, usize, &[u8])> {
    let line = unindent(line)?;
    let first = *line.first()?;
    if !matches!(first, b'`' | b'~') {
        return None;
    }
    let length = line.iter().take_while(|c| **c == first).count();
    (length >= 3).then_some((first, length, &line[length..]))
}

fn list_width(line: &[u8]) -> Option<usize> {
    let rest = unindent(line)?;
    let indent = line.len() - rest.len();
    let marker = if matches!(rest.first(), Some(b'-' | b'+' | b'*')) {
        1
    } else {
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || !matches!(rest.get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    let spaces = rest[marker..].iter().take_while(|c| horizontal(c)).count();
    (spaces > 0).then_some(indent + marker + spaces)
}

fn contains(data: &[u8], needle: &[u8]) -> bool {
    data.windows(needle.len()).any(|w| w == needle)
}
fn raw_line(raw: &[u8]) -> &[u8] {
    let end = raw
        .iter()
        .rposition(|c| !matches!(c, b'\r' | b'\n'))
        .map_or(0, |i| i + 1);
    &raw[..end]
}

// bytes.splitlines(keepends=True): only CR, LF and CRLF; no terminal empty line.
fn lines(data: &[u8]) -> Vec<(usize, &[u8])> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < data.len() {
        if matches!(data[at], b'\r' | b'\n') {
            at += if data[at] == b'\r' && data.get(at + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            result.push((start, &data[start..at]));
            start = at;
        } else {
            at += 1;
        }
    }
    if start < data.len() {
        result.push((start, &data[start..]));
    }
    result
}

fn token(value: &str, prefix: &str, length: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|tail| {
        tail.len() == length
            && tail
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

fn json_bytes(payload: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    use std::borrow::Cow;
    // Match json.loads(bytes) encoding detection, without changing covered bytes.
    // BOM-bearing UTF-16/32 already fails the whole-checkpoint UTF-8 contract.
    if let Some(json) = payload.strip_prefix(b"\xef\xbb\xbf") {
        return Ok(Cow::Borrowed(json));
    }
    let (width, little) = if payload.len() >= 4 && payload[0] == 0 {
        (if payload[1] == 0 { 4 } else { 2 }, false)
    } else if payload.len() >= 4 && payload[1] == 0 {
        (
            if payload[2] == 0 && payload[3] == 0 {
                4
            } else {
                2
            },
            true,
        )
    } else if payload.len() == 2 && payload.contains(&0) {
        (2, payload[0] != 0)
    } else {
        return Ok(Cow::Borrowed(payload));
    };
    require(
        payload.len() % width == 0,
        "invalid_checkpoint_decision_json",
    )?;
    let text = if width == 2 {
        let units: Vec<_> = payload
            .chunks_exact(2)
            .map(|pair| {
                let pair = [pair[0], pair[1]];
                if little {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| Error("invalid_checkpoint_decision_json"))?
    } else {
        payload
            .chunks_exact(4)
            .map(|part| {
                let part = [part[0], part[1], part[2], part[3]];
                let scalar = if little {
                    u32::from_le_bytes(part)
                } else {
                    u32::from_be_bytes(part)
                };
                char::from_u32(scalar).ok_or(Error("invalid_checkpoint_decision_json"))
            })
            .collect::<Result<String>>()?
    };
    Ok(Cow::Owned(text.into_bytes()))
}

fn record(value: &Value, checkpoint: &str, history: bool) -> Result<()> {
    let object = value
        .as_object()
        .ok_or(Error("invalid_checkpoint_record_fields"))?;
    let fields = [
        "version",
        "checkpoint",
        "decision_id",
        "action",
        "scope_revision",
        "reviewed_revision",
        "decided_at",
        "source",
    ];
    require(
        object.entries().len() == fields.len() + usize::from(history)
            && fields.iter().all(|field| object.get(field).is_some())
            && (!history || object.get("notes").is_some()),
        "invalid_checkpoint_record_fields",
    )?;
    require(
        object.get("version").is_some_and(Value::is_integer_one),
        "invalid_checkpoint_decision_version",
    )?;
    let text = |field| {
        object
            .get(field)
            .and_then(Value::as_str)
            .ok_or(Error("invalid_checkpoint_record_text"))
    };
    let owner = text("checkpoint")?;
    require(
        identifier(owner) && owner == checkpoint,
        "invalid_checkpoint_record_checkpoint",
    )?;
    require(
        token(text("decision_id")?, "", 32),
        "invalid_checkpoint_decision_identity",
    )?;
    require(
        matches!(text("action")?, "approved" | "changes_requested"),
        "invalid_checkpoint_decision_action",
    )?;
    require(
        token(text("scope_revision")?, "checkpoint-scope-v1:", 64),
        "invalid_checkpoint_decision_scope",
    )?;
    let revision = text("reviewed_revision")?;
    require(
        token(revision, "checkpoint-snapshot-v1:", 64)
            || revision
                .strip_prefix("checkpoint-missing-v1:")
                .is_some_and(|id| identifier(id) && id == checkpoint),
        "invalid_checkpoint_reviewed_revision",
    )?;
    require(
        matches!(text("source")?, "cli" | "review" | "console"),
        "invalid_checkpoint_decision_source",
    )?;
    text("decided_at")?;
    if history {
        let notes = text("notes")?;
        // This precondition prevents recursive machine records in the notes scan.
        require(
            !notes.contains("coven-checkpoint-") && !notes.contains("-->"),
            "invalid_checkpoint_notes_machinery",
        )?;
        let scan = scan(notes.as_bytes(), "notes")?;
        require(
            !scan
                .visible
                .iter()
                .any(|line| heading(line.text) == Some((2, b"user decision".to_vec()))),
            "invalid_checkpoint_notes_section",
        )?;
    }
    Ok(())
}

struct Line<'a> {
    start: usize,
    end: usize,
    text: &'a [u8],
}
struct Scan<'a> {
    visible: Vec<Line<'a>>,
    excluded: Vec<Span>,
    decision: Option<Value>,
    history: Vec<Value>,
    history_spans: Vec<Span>,
}

fn scan<'a>(data: &'a [u8], checkpoint: &str) -> Result<Scan<'a>> {
    let lines = lines(data);
    let mut result = Scan {
        visible: Vec::new(),
        excluded: Vec::new(),
        decision: None,
        history: Vec::new(),
        history_spans: Vec::new(),
    };
    let mut history_ids = HashSet::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut comment = false;
    let mut list_indent = None;
    let mut index = 0;
    let mut first_h2 = false;
    while index < lines.len() {
        let (offset, raw) = lines[index];
        let line = raw_line(raw);
        let end = offset + raw.len();
        let marker = fence_marker(line);
        let indentation = line.iter().take_while(|c| **c == b' ').count();
        let nested = list_indent
            .is_some_and(|width| !blank(line) && (line.starts_with(b"\t") || indentation >= width));
        if fence.is_none() && !comment && !blank(line) && !nested {
            list_indent = list_width(line);
        }
        if let Some((character, length)) = fence {
            if marker.is_some_and(|(c, n, tail)| c == character && n >= length && blank(tail)) {
                fence = None;
            }
        } else if nested || line.starts_with(b"    ") || line.starts_with(b"\t") {
        } else if let Some((character, length, _)) = marker.filter(|_| !comment) {
            fence = Some((character, length));
        } else if contains(line, b"coven-checkpoint-") {
            let history = match line {
                b"<!-- coven-checkpoint-decision:v1" => false,
                b"<!-- coven-checkpoint-history:v1" => true,
                _ => return Err(Error("invalid_checkpoint_reserved_machinery")),
            };
            require(
                !comment && index + 2 < lines.len(),
                "invalid_checkpoint_reserved_machinery",
            )?;
            let payload = lines[index + 1].1;
            let closing = lines[index + 2].1;
            require(
                raw_line(closing) == b"-->"
                    && !contains(payload, b"-->")
                    && !contains(payload, b"<!--"),
                "invalid_checkpoint_reserved_block",
            )?;
            let json = json_bytes(payload)?;
            // Resource refusals stay fatal even when the record is also malformed.
            let value = values::parse(&json, Profile::Authority).map_err(|error| {
                let code = error.code();
                if (code.starts_with("unsupported_json_") && code.ends_with("_profile"))
                    || matches!(
                        code,
                        "unsupported_integer_profile" | "unsupported_number_token_profile"
                    )
                {
                    error
                } else {
                    Error("invalid_checkpoint_decision_json")
                }
            })?;
            record(&value, checkpoint, history)?;
            let stop = end + payload.len() + closing.len();
            let mut start = offset;
            if history {
                require(
                    offset > 0 && data[offset - 1] == b'\n',
                    "invalid_checkpoint_history_separator",
                )?;
                start -= 1;
                let id = value
                    .as_object()
                    .and_then(|v| v.get("decision_id"))
                    .and_then(Value::as_str)
                    .ok_or(Error("invalid_checkpoint_decision_identity"))?;
                require(
                    history_ids.insert(id.to_owned()),
                    "invalid_checkpoint_duplicate_history",
                )?;
                result.history.push(value);
                result.history_spans.push((start, stop));
            } else {
                require(
                    result.decision.is_none() && !first_h2,
                    "invalid_checkpoint_decision_position",
                )?;
                result.decision = Some(value);
            }
            result.excluded.push((start, stop));
            index += 3;
            continue;
        } else if comment {
            if contains(line, b"-->") {
                comment = false;
            }
        } else if let Some(at) = line.windows(4).position(|w| w == b"<!--") {
            comment = !contains(&line[at + 4..], b"-->");
        } else {
            if heading(line).is_some_and(|(level, _)| level == 2) {
                first_h2 = true;
            }
            result.visible.push(Line {
                start: offset,
                end,
                text: line,
            });
        }
        index += 1;
    }
    require(fence.is_none(), "invalid_checkpoint_unterminated_fence")?;
    Ok(result)
}

fn checkbox(line: &[u8], label: &[u8]) -> Option<bool> {
    let line = unindent(line)?.strip_prefix(b"-")?;
    let spaces = line.iter().take_while(|c| horizontal(c)).count();
    if spaces == 0 {
        return None;
    }
    let line = &line[spaces..];
    if line.first() != Some(&b'[')
        || !matches!(line.get(1), Some(b' ' | b'x' | b'X'))
        || line.get(2) != Some(&b']')
    {
        return None;
    }
    let rest = &line[3..];
    let spaces = rest.iter().take_while(|c| horizontal(c)).count();
    if spaces == 0 {
        return None;
    }
    let rest = &rest[spaces..];
    let end = rest
        .iter()
        .rposition(|c| !horizontal(c))
        .map_or(0, |i| i + 1);
    rest[..end]
        .eq_ignore_ascii_case(label)
        .then_some(line[1] != b' ')
}

/// Inspect only the supplied bytes. A missing checkpoint is not an empty file.
pub fn inspect_bytes(checkpoint: &str, data: Option<&[u8]>) -> Result<Value> {
    require(identifier(checkpoint), "invalid_checkpoint_identifier")?;
    let mut fields = vec![
        ("checkpoint".into(), Value::String(checkpoint.into())),
        (
            "revision".into(),
            Value::String(format!("checkpoint-missing-v1:{checkpoint}")),
        ),
        ("scope_revision".into(), Value::Null),
        ("declared_status".into(), Value::String("missing".into())),
        ("effective_status".into(), Value::String("missing".into())),
        ("binding".into(), Value::String("none".into())),
        ("decision".into(), Value::Null),
        ("reason".into(), Value::Null),
        ("content".into(), Value::Null),
        ("history".into(), Value::Array(Vec::new())),
    ];
    let Some(data) = data else {
        return Value::object(fields);
    };
    require(
        data.len() <= MAX_INPUT,
        "unsupported_checkpoint_input_profile",
    )?;
    let content = std::str::from_utf8(data).map_err(|_| Error("invalid_checkpoint_utf8"))?;
    let mut scan = scan(data, checkpoint)?;
    let header_end = scan
        .visible
        .iter()
        .find(|line| heading(line.text).is_some_and(|(level, _)| level == 2))
        .map_or(data.len(), |line| line.start);
    let statuses: Vec<_> = scan
        .visible
        .iter()
        .filter(|line| {
            line.start < header_end
                && unindent(line.text).is_some_and(|s| {
                    s.get(..7)
                        .is_some_and(|s| s.eq_ignore_ascii_case(b"status:"))
                })
        })
        .collect();
    let sections: Vec<_> = scan
        .visible
        .iter()
        .filter(|line| heading(line.text) == Some((2, b"user decision".to_vec())))
        .collect();
    require(
        statuses.len() <= 1 && sections.len() <= 1,
        "invalid_checkpoint_ambiguous_presentation",
    )?;
    let mut checks = Vec::new();
    if let Some(section) = sections.first() {
        let stop = scan
            .visible
            .iter()
            .find(|line| line.start >= section.end && heading(line.text).is_some())
            .map_or(data.len(), |line| line.start);
        for (label, approved) in [
            (b"Approved".as_slice(), true),
            (b"Rejected / needs changes".as_slice(), false),
        ] {
            let mut count = 0;
            for line in &scan.visible {
                if line.start >= section.end && line.start < stop {
                    if let Some(checked) = checkbox(line.text, label) {
                        count += 1;
                        checks.push((line.start, line.end, approved, checked));
                    }
                }
            }
            require(count <= 1, "invalid_checkpoint_ambiguous_checkbox")?;
        }
    }
    scan.excluded
        .extend(statuses.iter().map(|line| (line.start, line.end)));
    scan.excluded
        .extend(checks.iter().map(|&(start, end, _, _)| (start, end)));
    scan.excluded.sort_unstable();
    let (mut cursor, mut last_covered) = (0, 0);
    for &(start, end) in &scan.excluded {
        if start > cursor {
            last_covered = start;
        }
        cursor = cursor.max(end);
    }
    if cursor < data.len() {
        last_covered = data.len();
    }
    let internal: HashSet<_> = scan
        .history_spans
        .iter()
        .copied()
        .filter(|&(_, end)| end <= last_covered)
        .collect();
    scan.excluded.retain(|span| !internal.contains(span));
    let mut remaining = Vec::new();
    cursor = 0;
    for &(start, end) in &scan.excluded {
        remaining.extend_from_slice(&data[cursor..cursor.max(start)]);
        cursor = cursor.max(end);
    }
    remaining.extend_from_slice(&data[cursor..]);
    let scope = hash("scope", checkpoint, &remaining);
    let declared = if let Some(line) = statuses.first() {
        let at = line
            .text
            .iter()
            .position(|c| *c == b':')
            .ok_or(Error("invalid_checkpoint_status"))?;
        let text = std::str::from_utf8(&line.text[at + 1..])
            .map_err(|_| Error("invalid_checkpoint_utf8"))?;
        let status = trimmed(text).to_lowercase();
        if status.is_empty() {
            "unknown".into()
        } else {
            status
        }
    } else {
        "unknown".into()
    };
    let mut effective = declared.clone();
    let mut binding = "none";
    let mut reason = None;
    if let Some(decision) = &scan.decision {
        let object = decision
            .as_object()
            .ok_or(Error("invalid_checkpoint_record_fields"))?;
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .ok_or(Error("invalid_checkpoint_decision_action"))?;
        binding = "bound";
        if declared != action
            || checks
                .iter()
                .any(|&(_, _, approved, checked)| checked != (approved == (action == "approved")))
        {
            binding = "inconsistent";
            effective = "decision_invalid".into();
            reason = Some("Current presentation contradicts the bound decision");
        } else if object.get("scope_revision").and_then(Value::as_str) != Some(scope.as_str()) {
            effective = if action == "approved" {
                "approval_stale"
            } else {
                action
            }
            .into();
            reason = Some("Covered checkpoint bytes differ from the decision scope");
        } else {
            effective = action.into();
        }
    } else if declared == "approved" {
        binding = "legacy_unbound";
        effective = "approved_unbound".into();
    }
    fields[1].1 = Value::String(hash("snapshot", checkpoint, data));
    fields[2].1 = Value::String(scope);
    fields[3].1 = Value::String(declared);
    fields[4].1 = Value::String(effective);
    fields[5].1 = Value::String(binding.into());
    fields[6].1 = scan.decision.unwrap_or(Value::Null);
    fields[7].1 = reason.map_or(Value::Null, |text| Value::String(text.into()));
    fields[8].1 = Value::String(content.into());
    fields[9].1 = Value::Array(scan.history);
    Value::object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_empty_digest_vectors_and_missing_envelope() {
        let empty = inspect_bytes("plan", Some(b"")).unwrap();
        let object = empty.as_object().unwrap();
        assert_eq!(
            object.get("revision").unwrap().as_str(),
            Some(
                "checkpoint-snapshot-v1:928859b8f8f129c2b2eeab608e81f060ba50cb41c98d2285eb43df0b0c92ec65"
            )
        );
        assert_eq!(
            object.get("scope_revision").unwrap().as_str(),
            Some(
                "checkpoint-scope-v1:561c68f1a5a88f08320eb1d214b2b930f979b8d920b4b3ccda56ebc285db585d"
            )
        );
        let missing = inspect_bytes("plan", None).unwrap();
        assert_eq!(missing.canonical().unwrap(), br#"{"binding":"none","checkpoint":"plan","content":null,"decision":null,"declared_status":"missing","effective_status":"missing","history":[],"reason":null,"revision":"checkpoint-missing-v1:plan","scope_revision":null}"#);
    }

    #[test]
    fn byte_lines_and_heading_edges() {
        let input = b"a\r\nb\rc\n\x0b\x0c\x1c\xc2\x85";
        assert_eq!(
            lines(input),
            vec![
                (0, &input[..3]),
                (3, &input[3..5]),
                (5, &input[5..7]),
                (7, &input[7..])
            ]
        );
        assert_eq!(
            heading(b"   ## USER DECISION \t###\t"),
            Some((2, b"user decision".to_vec()))
        );
        assert_eq!(heading(b"#  "), Some((1, b" ".to_vec())));
        for line in [
            b"    ## User decision".as_slice(),
            b"####### no",
            b"##no",
            b"## ",
        ] {
            assert_eq!(heading(line), None);
        }
        assert_eq!(checkbox(b" -\t[X]\tAPPROVED \t", b"Approved"), Some(true));
        assert_eq!(checkbox(b"- [x] Approved extra", b"Approved"), None);
    }

    #[test]
    fn independent_mixed_line_digest_vectors() {
        let data = b"# Plan\r\nStatus: pending\r## User decision\n- [ ] Approved\r\nTail\x0b\x0c";
        let view = inspect_bytes("plan", Some(data)).unwrap();
        let object = view.as_object().unwrap();
        assert_eq!(
            object.get("revision").unwrap().as_str(),
            Some(
                "checkpoint-snapshot-v1:f40612f316849a77421b7b4d7e017dc873568100f5a077619815c13e0e2d8a9d"
            )
        );
        assert_eq!(
            object.get("scope_revision").unwrap().as_str(),
            Some(
                "checkpoint-scope-v1:96e6d5ae1d55032a87174d50fac553559c029704e70ed6733327b3f2cd7359dd"
            )
        );
        assert_eq!(
            object.get("content").unwrap().as_str().unwrap().as_bytes(),
            data
        );
    }

    #[test]
    fn literal_scanner_precedence_and_offsets() {
        let data = b"12) List\r\n    Status: nested\r\n\r\n\t## User decision\r\n\
Status: pending\r<!-- comment\r## Hidden\r-->\r\
~~~~lang\r```\r~~~\r~~~~~ \t\r## User decision ###\r- [ ] Approved\r";
        let result = scan(data, "plan").unwrap();
        let visible: Vec<_> = result.visible.iter().map(|line| line.text).collect();
        assert_eq!(
            visible,
            vec![
                b"12) List".as_slice(),
                b"",
                b"Status: pending",
                b"## User decision ###",
                b"- [ ] Approved"
            ]
        );
        for line in result.visible {
            assert_eq!(line.text, raw_line(&data[line.start..line.end]));
        }
        for raw in [
            b"<!--\ncoven-checkpoint-any\n-->\n".as_slice(),
            b"inline <!-- coven-checkpoint-any -->",
            b"```\n~~~\n",
        ] {
            assert!(scan(raw, "plan").is_err());
        }
        assert!(scan(b"    coven-checkpoint-any\n", "plan").is_ok());
        assert!(scan(b"```\ncoven-checkpoint-any\n```\n", "plan").is_ok());
    }

    #[test]
    fn nonscalar_machine_records_preserve_resource_refusals() {
        let full_array = format!("[{}]", vec!["0"; values::MAX_ITEMS].join(","));
        let cases = [
            (
                format!(
                    "{}{}",
                    "[".repeat(values::MAX_DEPTH + 1),
                    "]".repeat(values::MAX_DEPTH + 1)
                ),
                "unsupported_json_depth_profile",
            ),
            (
                "1".repeat(values::MAX_INTEGER_DIGITS + 1),
                "unsupported_integer_profile",
            ),
            (
                format!("0.{}", "0".repeat(values::MAX_NUMBER)),
                "unsupported_number_token_profile",
            ),
            (
                format!("[{}0]", "0,".repeat(values::MAX_ITEMS)),
                "unsupported_json_array_profile",
            ),
            (
                format!(
                    "{{{}}}",
                    (0..=values::MAX_MEMBERS)
                        .map(|i| format!("\"{i}\":0"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                "unsupported_json_object_profile",
            ),
            (
                format!("[{0},{0},{0},{0}]", full_array),
                "unsupported_json_node_profile",
            ),
        ];
        for kind in ["decision", "history"] {
            for (payload, expected) in &cases {
                let data = format!("\n<!-- coven-checkpoint-{kind}:v1\n{payload}\n-->\n");
                assert!(data.len() < MAX_INPUT);
                assert_eq!(
                    inspect_bytes("plan", Some(data.as_bytes()))
                        .unwrap_err()
                        .code(),
                    *expected,
                    "{kind}: {expected}"
                );
            }
            for payload in [
                "{",
                r#"{"version":1,"version":1}"#,
                "NaN",
                "Infinity",
                "1e9999",
                r#""\ud800""#,
            ] {
                let data = format!("\n<!-- coven-checkpoint-{kind}:v1\n{payload}\n-->\n");
                assert_eq!(
                    inspect_bytes("plan", Some(data.as_bytes()))
                        .unwrap_err()
                        .code(),
                    "invalid_checkpoint_decision_json",
                    "{kind}: {payload}"
                );
            }
        }
    }

    #[test]
    fn unicode_status_and_native_limit() {
        for (input, expected) in [
            ("Status: \u{1c}\u{a0}ΟΣ\u{1f}\n", "ος"),
            ("Status: ΟΣΑ\n", "οσα"),
            ("Status: \u{130}\n", "i\u{307}"),
        ] {
            let view = inspect_bytes("plan", Some(input.as_bytes())).unwrap();
            assert_eq!(
                view.as_object()
                    .unwrap()
                    .get("declared_status")
                    .unwrap()
                    .as_str(),
                Some(expected)
            );
        }
        assert!(inspect_bytes("plan", Some(&vec![b'a'; MAX_INPUT])).is_ok());
        assert_eq!(
            inspect_bytes("plan", Some(&vec![b'a'; MAX_INPUT + 1]))
                .unwrap_err()
                .code(),
            "unsupported_checkpoint_input_profile"
        );
        assert_eq!(
            inspect_bytes("../plan", None).unwrap_err().code(),
            "invalid_checkpoint_identifier"
        );
    }
}
