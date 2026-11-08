//! Strict producer wire profile, including extensions which are never projected.
use crate::{
    Error, Result,
    values::{Number, Value},
};

pub(super) fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error("schema_error"))
    }
}
pub(super) fn object(entries: Vec<(&str, Value)>) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}
pub(super) fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}
pub(super) fn field<'a>(v: &'a Value, k: &str) -> Result<&'a Value> {
    v.as_object()
        .and_then(|o| o.get(k))
        .ok_or(Error("schema_error"))
}
fn string(v: &Value) -> Result<&str> {
    v.as_str().ok_or(Error("schema_error"))
}
fn project(v: &Value, keys: &[&str]) -> Result<Value> {
    object(
        keys.iter()
            .map(|k| Ok((*k, field(v, k)?.clone())))
            .collect::<Result<_>>()?,
    )
}
fn null(v: &Value) -> bool {
    matches!(v, Value::Null)
}
fn count(v: &Value, cap: u64) -> Result<u64> {
    let Value::Number(Number::Integer(n)) = v else {
        return Err(Error("schema_error"));
    };
    let n = n
        .as_str()
        .parse::<u64>()
        .map_err(|_| Error("schema_error"))?;
    require(n <= cap)?;
    Ok(n)
}
fn bounded(v: &Value, limit: usize) -> Result<&str> {
    let s = string(v)?;
    require(!s.is_empty() && s.len() <= limit)?;
    Ok(s)
}
pub(super) fn selector(s: &str) -> Result<()> {
    require(
        s.starts_with('/')
            && !s.ends_with('/')
            && s.len() <= 4096
            && !s.split('/').any(|s| s == "..")
            && !s
                .chars()
                .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c)),
    )
}
fn normalized(s: &str) -> bool {
    // POSIX normpath preserves exactly two leading slashes, but no other repeats.
    let body = if s.starts_with("//") && !s.starts_with("///") {
        &s[2..]
    } else {
        &s[1..]
    };
    body.split('/')
        .all(|c| !c.is_empty() && c != "." && c != "..")
}
fn native_id(v: &Value) -> Result<()> {
    if null(v) {
        return Ok(());
    }
    let s = bounded(v, 128)?;
    require(s.strip_prefix("agt_").is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    }))
}
fn code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
fn timestamp(v: &Value, utc: bool) -> Result<()> {
    let s = bounded(v, 64)?.as_bytes();
    let digits = |at: usize, n: usize| -> Result<u32> {
        let bytes = s.get(at..at + n).ok_or(Error("schema_error"))?;
        require(bytes.iter().all(u8::is_ascii_digit))?;
        Ok(bytes.iter().fold(0, |v, b| v * 10 + u32::from(b - b'0')))
    };
    require(
        s.len() >= 20
            && s[4] == b'-'
            && s[7] == b'-'
            && matches!(s[10], b'T' | b't')
            && s[13] == b':'
            && s[16] == b':',
    )?;
    let year = digits(0, 4)?;
    let month = digits(5, 2)?;
    let day = digits(8, 2)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        28 + u32::from(leap),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    require((1..=12).contains(&month))?;
    require(
        day >= 1
            && day <= days[(month - 1) as usize]
            && digits(11, 2)? <= 23
            && digits(14, 2)? <= 59
            && digits(17, 2)? <= 60,
    )?;
    let mut at = 19;
    if s.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while s.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        require(at > start)?;
    }
    let zone = s.get(at..).ok_or(Error("schema_error"))?;
    if matches!(zone, b"Z" | b"z") {
        return Ok(());
    }
    require(zone.len() == 6 && matches!(zone[0], b'+' | b'-') && zone[3] == b':')?;
    require(digits(at + 1, 2)? <= 23 && digits(at + 4, 2)? <= 59 && (!utc || zone == b"+00:00"))
}

pub(super) fn wire(payload: &[u8]) -> Result<Value> {
    crate::observation_wire::decode_native(payload)
}
fn source(v: &Value, path: &str, cap: u64) -> Result<Value> {
    require(string(field(v, "path")?)? == path)?;
    let n = count(field(v, "bytes")?, cap)?;
    let digest = string(field(v, "sha256")?)?;
    require(
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    )?;
    require(
        n != 0 || digest == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    )?;
    project(v, &["path", "bytes", "sha256"])
}
fn event(v: &Value, lifecycle: bool) -> Result<Value> {
    let keys: &[&str] = if lifecycle {
        &["state", "mode", "event_id", "session_id", "recorded_at"]
    } else {
        &["kind", "event_id", "session_id", "recorded_at"]
    };
    let projected = project(v, keys)?;
    if lifecycle {
        require(matches!(
            string(field(v, "state")?)?,
            "created" | "running" | "paused" | "completed" | "failed" | "cancelled"
        ))?;
        let mode = field(v, "mode")?;
        require(null(mode) || matches!(mode.as_str(), Some("foreground" | "background")))?;
    } else {
        require(code(string(field(v, "kind")?)?))?;
    }
    let id = field(v, "event_id")?;
    if !null(id) {
        let id = bounded(id, 256)?;
        require(
            !id.chars()
                .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c)),
        )?;
    }
    native_id(field(v, "session_id")?)?;
    let ts = field(v, "recorded_at")?;
    if !null(ts) {
        timestamp(ts, false)?;
    }
    Ok(projected)
}
fn equal(a: &Value, b: &Value) -> Result<bool> {
    Ok(a.compact()? == b.compact()?)
}
fn facts(v: &Value) -> Result<Value> {
    let mut out = project(
        v,
        &[
            "recorded_native_session_id",
            "recorded_created_at",
            "sources",
            "event_count",
            "last_event",
            "last_lifecycle",
            "consistency",
        ],
    )?;
    require(string(field(v, "consistency")?)? == "stable_per_file_not_transactional")?;
    native_id(field(v, "recorded_native_session_id")?)?;
    let created = field(v, "recorded_created_at")?;
    if !null(created) {
        timestamp(created, false)?;
    }
    let sources = field(v, "sources")?;
    let manifest = source(field(sources, "manifest")?, "djinn.toml", 65536)?;
    let events = field(sources, "folder_events")?;
    require(string(field(events, "path")?)? == "events.jsonl")?;
    let last = field(v, "last_event")?;
    let life = field(v, "last_lifecycle")?;
    let n = field(v, "event_count")?;
    let events = match string(field(events, "status")?)? {
        "missing" => {
            require(
                null(field(events, "bytes")?)
                    && null(field(events, "sha256")?)
                    && null(n)
                    && null(last)
                    && null(life),
            )?;
            project(events, &["path", "status", "bytes", "sha256"])?
        }
        "observed" => {
            let mut projected = source(events, "events.jsonl", 4194304)?;
            let n = count(n, 16384)?;
            require(if n == 0 {
                null(last) && null(life)
            } else {
                count(field(events, "bytes")?, 4194304)? > 0 && !null(last)
            })?;
            projected.set_field("status", text("observed"))?;
            projected
        }
        _ => return Err(Error("schema_error")),
    };
    let last = if null(last) {
        Value::Null
    } else {
        event(last, false)?
    };
    let life = if null(life) {
        Value::Null
    } else {
        event(life, true)?
    };
    if !null(&last) && field(&last, "kind")?.as_str() == Some("session_lifecycle_updated") {
        require(!null(&life))?;
        for k in ["event_id", "session_id", "recorded_at"] {
            require(equal(field(&last, k)?, field(&life, k)?)?)?;
        }
    } else if !null(&last) && !null(&life) {
        let a = field(&last, "event_id")?;
        let b = field(&life, "event_id")?;
        require(null(a) || null(b) || !equal(a, b)?)?;
    }
    if !null(&life) && n.is_integer_one() {
        require(
            !null(&last) && field(&last, "kind")?.as_str() == Some("session_lifecycle_updated"),
        )?;
    }
    let mut identity = None;
    for id in [
        field(v, "recorded_native_session_id")?,
        if null(&last) {
            &Value::Null
        } else {
            field(&last, "session_id")?
        },
        if null(&life) {
            &Value::Null
        } else {
            field(&life, "session_id")?
        },
    ] {
        if !null(id) {
            if let Some(before) = identity {
                require(equal(before, id)?)?;
            }
            identity = Some(id);
        }
    }
    out.set_field(
        "sources",
        object(vec![("manifest", manifest), ("folder_events", events)])?,
    )?;
    out.set_field("last_event", last)?;
    out.set_field("last_lifecycle", life)?;
    Ok(out)
}
pub(super) fn decode(payload: &[u8], folder: &str) -> Result<(&'static str, Value, Value)> {
    let v = wire(payload)?;
    let version = field(&v, "schema_version")?;
    require(matches!(version, Value::Number(Number::Integer(_))))?;
    let contract = string(field(&v, "contract")?)?;
    if !version.is_integer_one() || contract != "djinn.session.observation.v1" {
        return Err(Error("unsupported_contract"));
    }
    require(string(field(&v, "selected_folder")?)? == folder)?;
    for (k, expected) in [
        ("authorization", "none"),
        ("runtime_fact_freshness", "unknown"),
        ("native_lifecycle", "unestablished"),
        ("job_completion", "unestablished"),
        ("activity", "unknown"),
    ] {
        require(string(field(&v, k)?)? == expected)?;
    }
    require(matches!(field(&v, "dispatchable")?, Value::Bool(false)))?;
    let caps = field(&v, "capabilities")?;
    require(caps.as_object().is_some_and(|o| o.entries().len() == 4))?;
    for (key, b) in [
        ("persisted_folder_events", true),
        ("live_status", false),
        ("ui_sessions", false),
        ("native_store", false),
    ] {
        require(matches!(field(caps,key)?,Value::Bool(x) if *x == b))?;
    }
    timestamp(field(&v, "started_at")?, true)?;
    timestamp(field(&v, "observed_at")?, true)?;
    let resolved = field(&v, "resolved_folder")?;
    if !null(resolved) {
        let s = string(resolved)?;
        selector(s)?;
        require(normalized(s))?;
    }
    let diagnostics = field(&v, "diagnostics")?
        .as_array()
        .ok_or(Error("schema_error"))?;
    require(diagnostics.len() <= 16 && diagnostics.iter().all(|d| d.as_str().is_some_and(code)))?;
    let observation = field(&v, "observation")?;
    let assessment = string(field(&v, "assessment")?)?;
    let facts = if assessment == "observed" {
        require(!null(resolved) && diagnostics.is_empty())?;
        facts(observation)?
    } else {
        require(assessment == "unavailable" && !diagnostics.is_empty() && null(observation))?;
        Value::Null
    };
    let producer = project(
        &v,
        &[
            "contract",
            "started_at",
            "observed_at",
            "resolved_folder",
            "capabilities",
        ],
    )?;
    Ok((
        if assessment == "observed" {
            "observed"
        } else {
            "unavailable"
        },
        facts,
        producer,
    ))
}
