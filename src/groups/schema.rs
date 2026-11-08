use crate::{
    Error, Result,
    error::require,
    unicode,
    values::{Number, Value},
};
use std::collections::HashSet;

const BAD: &str = "groups_malformed_authority";
pub(super) fn get<'a>(v: &'a Value, k: &str) -> Result<&'a Value> {
    optional(v, k).ok_or(Error(BAD))
}
pub(super) fn optional<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.as_object()?.get(k)
}
pub(super) fn text(v: &Value) -> Result<&str> {
    v.as_str().ok_or(Error(BAD))
}
pub(super) fn array(v: &Value) -> Result<&[Value]> {
    v.as_array().ok_or(Error(BAD))
}
pub(super) fn string(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
pub(super) fn num(n: u64) -> Result<Value> {
    Value::integer(&n.to_string())
}
pub(super) fn obj(fields: &[(&str, Value)]) -> Result<Value> {
    Value::object(
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
}
pub(super) fn equal(a: &Value, b: &Value) -> Result<bool> {
    Ok(a.canonical()? == b.canonical()?)
}
fn fields(v: &Value, expected: &str) -> Result<()> {
    let o = v.as_object().ok_or(Error(BAD))?;
    let keys: Vec<_> = expected.split_whitespace().collect();
    require(
        o.entries().len() == keys.len() && keys.iter().all(|k| o.get(k).is_some()),
        BAD,
    )
}
pub(super) fn bounded(s: &str, max: usize, empty: bool) -> Result<()> {
    require(
        (empty || !s.is_empty()) && s.len() <= max && !s.contains('\0'),
        "groups_invalid_string",
    )
}
fn s(v: &Value, max: usize, empty: bool) -> Result<&str> {
    let s = text(v)?;
    bounded(s, max, empty)?;
    Ok(s)
}
pub(crate) fn selector(s: &str) -> Result<()> {
    require(
        !s.is_empty()
            && s.len() <= 64
            && (s.as_bytes()[0].is_ascii_lowercase() || s.as_bytes()[0].is_ascii_digit())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')),
        "groups_invalid_selector",
    )
}
pub(crate) fn display(s: &str) -> Result<()> {
    bounded(s, 64, false)?;
    require(
        s.chars().all(unicode::printable) && s.chars().any(|c| !c.is_whitespace()),
        "groups_invalid_display",
    )
}
pub(crate) fn source(s: &str) -> Result<()> {
    require(
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')),
        "groups_invalid_source",
    )
}
pub(super) fn uuid(s: &str) -> Result<()> {
    require(
        s.len() == 36
            && s.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            }),
        "groups_invalid_uuid",
    )
}
pub(super) fn uint(v: &Value) -> Result<u64> {
    if let Value::Number(Number::Integer(n)) = v {
        n.as_str().parse().map_err(|_| Error("groups_invalid_u64"))
    } else {
        Err(Error("groups_invalid_u64"))
    }
}
fn nonnegative(v: &Value) -> Result<()> {
    require(
        matches!(v,Value::Number(Number::Integer(n)) if !n.as_str().starts_with('-')),
        "groups_invalid_endpoint_identity",
    )
}
fn absolute(s: &str) -> bool {
    s.starts_with('/')
        && !s.starts_with("//")
        && (s == "/"
            || (!s.ends_with('/')
                && s[1..]
                    .split('/')
                    .all(|c| !c.is_empty() && c != "." && c != "..")))
}
pub(super) fn endpoint(v: &Value) -> Result<()> {
    fields(v, "path device inode")?;
    require(
        absolute(s(get(v, "path")?, 8192, false)?),
        "groups_invalid_endpoint_path",
    )?;
    nonnegative(get(v, "device")?)?;
    nonnegative(get(v, "inode")?)
}
pub(crate) fn validate_binding(v: &Value) -> Result<()> {
    fields(v, "presentation evidence")?;
    let p = get(v, "presentation")?;
    fields(p, "adapter endpoint terminal_id pane_id_at_binding")?;
    require(text(get(p, "adapter")?)? == "kitsune", BAD)?;
    endpoint(get(p, "endpoint")?)?;
    s(get(p, "terminal_id")?, 4096, false)?;
    let pane = s(get(p, "pane_id_at_binding")?, 4096, false)?;
    require(
        pane.chars()
            .all(|c| unicode::printable(c) && !c.is_whitespace()),
        "groups_invalid_pane",
    )?;
    let e = get(v, "evidence")?;
    let process = text(get(e, "provenance")?)? == "kitsune.pane.process_info";
    fields(
        e,
        if process {
            "provenance observed_at agent_kind_hint native_reference native_binding process_evidence"
        } else {
            "provenance observed_at agent_kind_hint native_reference native_binding"
        },
    )?;
    require(
        process || text(get(e, "provenance")?)? == "kitsune.session.snapshot",
        BAD,
    )?;
    if process {
        require(
            text(get(e, "agent_kind_hint")?)? == "djinn"
                && matches!(get(e, "native_reference")?, Value::Null)
                && text(get(e, "native_binding")?)? == "unestablished",
            BAD,
        )?;
        let details = get(e, "process_evidence")?;
        fields(details, "interpreter matches")?;
        require(
            text(get(details, "interpreter")?)? == "coven.kitsune.djinn-process.v1",
            BAD,
        )?;
        let matches = array(get(details, "matches")?)?;
        require((1..=1024).contains(&matches.len()), BAD)?;
        let mut previous = 0;
        for m in matches {
            fields(m, "pid executable_family rule")?;
            let pid = uint(get(m, "pid")?)?;
            require(
                pid > previous && pid <= u32::MAX as u64,
                "groups_invalid_process_order",
            )?;
            previous = pid;
            require(
                matches!(text(get(m, "executable_family")?)?, "djinn" | "djinn-ui")
                    && text(get(m, "rule")?)? == "exact_posix_argv0_basename",
                BAD,
            )?;
        }
    }
    super::timestamp::validate(s(get(e, "observed_at")?, 64, false)?)?;
    if !matches!(get(e, "agent_kind_hint")?, Value::Null) {
        s(get(e, "agent_kind_hint")?, 4096, true)?;
    }
    let native = get(e, "native_reference")?;
    if !matches!(native, Value::Null) {
        fields(native, "source agent kind value")?;
        let kind = text(get(native, "kind")?)?;
        require(matches!(kind, "id" | "path"), BAD)?;
        for key in ["source", "agent", "value"] {
            s(
                get(native, key)?,
                if key == "value" && kind == "path" {
                    8192
                } else {
                    4096
                },
                true,
            )?;
        }
    }
    require(
        text(get(e, "native_binding")?)?
            == if matches!(native, Value::Null) {
                "unestablished"
            } else {
                "observed_reference"
            },
        BAD,
    )
}
pub(crate) fn stable_binding(v: &Value) -> Result<Value> {
    validate_binding(v)?;
    let mut v = v.clone();
    v.set_field(
        "presentation",
        without(get(&v, "presentation")?, "pane_id_at_binding")?,
    )?;
    v.set_field("evidence", without(get(&v, "evidence")?, "observed_at")?)?;
    Ok(v)
}
fn without(v: &Value, key: &str) -> Result<Value> {
    Value::object(
        v.as_object()
            .ok_or(Error(BAD))?
            .entries()
            .iter()
            .filter(|(k, _)| k != key)
            .cloned()
            .collect(),
    )
}
pub(super) fn stable_attachment(v: &Value) -> Result<Value> {
    without(v, "attached_at")
}
pub(crate) fn validate_attachment(v: &Value) -> Result<()> {
    fields(
        v,
        "adapter source_contract artifact_path runtime_id instance_id session_id attached_at",
    )?;
    require(
        text(get(v, "adapter")?)? == "djinn_live"
            && text(get(v, "source_contract")?)? == "djinn.live-observation.v1",
        BAD,
    )?;
    for key in ["runtime_id", "instance_id"] {
        let id = text(get(v, key)?)?;
        uuid(id)?;
        require(
            id.as_bytes()[14] == b'4' && matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "groups_live_uuid_v4_required",
        )?;
    }
    let path = s(get(v, "artifact_path")?, 4096, false)?;
    require(
        absolute(path)
            && path
                .chars()
                .all(|c| (c as u32) >= 32 && !(127..=159).contains(&(c as u32)))
            && path.rsplit('/').next()
                == Some(format!("{}.json", text(get(v, "runtime_id")?)?).as_str()),
        "groups_invalid_artifact_path",
    )?;
    let session = text(get(v, "session_id")?)?;
    require(
        !session.is_empty()
            && session.len() <= 128
            && session.bytes().all(|b| (b'!'..=b'~').contains(&b)),
        "groups_invalid_session_id",
    )?;
    super::timestamp::validate(s(get(v, "attached_at")?, 64, false)?)
}
pub(super) fn validate(state: &Value) -> Result<()> {
    fields(
        state,
        "schema_version workspace_id content_revision groups publication_counters",
    )?;
    let version = uint(get(state, "schema_version")?)?;
    require((1..=3).contains(&version), BAD)?;
    uuid(text(get(state, "workspace_id")?)?)?;
    uint(get(state, "content_revision")?)?;
    let groups = array(get(state, "groups")?)?;
    let counters = array(get(state, "publication_counters")?)?;
    require(groups.len() <= 16 && counters.len() <= 8, BAD)?;
    let (mut ids, mut keys, mut associations) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut participants = 0;
    for g in groups {
        fields(g, "group_id key name members")?;
        let id = text(get(g, "group_id")?)?;
        uuid(id)?;
        let key = text(get(g, "key")?)?;
        selector(key)?;
        let name = text(get(g, "name")?)?;
        display(name)?;
        require(ids.insert(id) && keys.insert(key), "groups_duplicate_group")?;
        let members = array(get(g, "members")?)?;
        require(members.len() <= 16, BAD)?;
        let mut member_keys = HashSet::new();
        for m in members {
            fields(
                m,
                if version == 3 {
                    "participant_id key role binding live_attachment"
                } else {
                    "participant_id key role binding"
                },
            )?;
            if version == 3 && !matches!(get(m, "live_attachment")?, Value::Null) {
                validate_attachment(get(m, "live_attachment")?)?;
            }
            let id = text(get(m, "participant_id")?)?;
            uuid(id)?;
            let key = text(get(m, "key")?)?;
            selector(key)?;
            let role = text(get(m, "role")?)?;
            display(role)?;
            require(name.len() + 4 + role.len() <= 256, BAD)?;
            let b = get(m, "binding")?;
            validate_binding(b)?;
            require(
                version >= 2
                    || text(get(get(b, "evidence")?, "provenance")?)? == "kitsune.session.snapshot",
                "groups_process_requires_schema_2",
            )?;
            let p = get(b, "presentation")?;
            let association = obj(&[
                ("endpoint", get(p, "endpoint")?.clone()),
                ("terminal", get(p, "terminal_id")?.clone()),
            ])?
            .canonical()?;
            require(
                ids.insert(id) && member_keys.insert(key),
                "groups_duplicate_participant",
            )?;
            require(
                associations.insert(association),
                "groups_duplicate_association",
            )?;
            participants += 1;
        }
    }
    require(participants <= 64, "groups_participant_limit")?;
    let mut indices = HashSet::new();
    for c in counters {
        fields(c, "endpoint source epoch watermarks")?;
        endpoint(get(c, "endpoint")?)?;
        source(text(get(c, "source")?)?)?;
        s(get(c, "epoch")?, 4096, false)?;
        let index = obj(&[
            ("endpoint", get(c, "endpoint")?.clone()),
            ("source", get(c, "source")?.clone()),
        ])?
        .canonical()?;
        require(indices.insert(index), "groups_duplicate_counter")?;
        let wm = get(c, "watermarks")?.as_object().ok_or(Error(BAD))?;
        require(wm.entries().len() <= 64, BAD)?;
        for (t, n) in wm.entries() {
            bounded(t, 4096, false)?;
            require(uint(n)? > 0, "groups_invalid_watermark")?;
        }
    }
    Ok(())
}
