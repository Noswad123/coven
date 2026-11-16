//! Unordered, observation-only native command hook facts. No lifecycle state.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const RPC: &str = "coven.provider-facts.v1";

fn identity(v: &Value, key: &str) -> Result<()> {
    let s = text(v, key)?;
    require(
        !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control),
        "agent_invalid_schema",
    )
}

pub fn events(provider: &str) -> Result<&'static [&'static str]> {
    match provider {
        "codex" => Ok(&[
            "SessionStart",
            "PreModelRequest",
            "PostModelRequest",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "Interrupt",
            "SessionEnd",
        ]),
        "copilot" => Ok(&[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "Stop",
            "SessionEnd",
        ]),
        _ => Err(Error("agent_unsupported_provider")),
    }
}

/// Deliberately supported ISO8601 subset: UTC Z, seconds or 1-3 decimal digits.
fn timestamp(s: &str) -> Result<u64> {
    let bad = || Error("agent_invalid_timestamp");
    let b = s.as_bytes();
    require(
        (20..=24).contains(&b.len())
            && b.is_ascii()
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[16] == b':'
            && b.last() == Some(&b'Z'),
        "agent_invalid_timestamp",
    )?;
    let n = |a: usize, z: usize| -> Result<u64> {
        require(
            b[a..z].iter().all(u8::is_ascii_digit),
            "agent_invalid_timestamp",
        )?;
        s[a..z].parse().map_err(|_| bad())
    };
    let (y, m, d, h, min, sec) = (
        n(0, 4)?,
        n(5, 7)?,
        n(8, 10)?,
        n(11, 13)?,
        n(14, 16)?,
        n(17, 19)?,
    );
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
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
    require(
        (1970..=9999).contains(&y)
            && (1..=12).contains(&m)
            && d > 0
            && d <= days[(m - 1) as usize]
            && h < 24
            && min < 60
            && sec < 60,
        "agent_invalid_timestamp",
    )?;
    let mut total = 0;
    for year in 1970..y {
        total += if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
    }
    total += days[..(m - 1) as usize].iter().sum::<u64>() + d - 1;
    let fraction = if b.len() == 20 {
        0
    } else {
        require(b[19] == b'.' && b.len() >= 22, "agent_invalid_timestamp")?;
        n(20, b.len() - 1)? * 10u64.pow((24 - b.len()) as u32)
    };
    Ok(((total * 24 + h) * 60 * 60 + min * 60 + sec) * 1000 + fraction)
}

pub fn capture(provider: &str, event: &str, raw: &[u8]) -> Result<Value> {
    require(events(provider)?.contains(&event), "agent_invalid_event")?;
    let v = decode(raw, MAX_PAYLOAD)?;
    require(v["hook_event_name"] == event, "agent_invalid_event")?;
    identity(&v, "session_id")?;
    text(&v, "cwd")?;
    let codex = provider == "codex";
    let mut keys = vec![
        "hook_event_name",
        "session_id",
        "cwd",
        "agent_id",
        "agent_type",
    ];
    let mut p = json!({"provider":provider,"profile":format!("{provider}-hook-facts.v1"),"session_id":v["session_id"],"event":event,"correlation":"none"});
    if codex {
        keys.push("transcript_path");
        if event != "SessionEnd" {
            keys.push("model");
            text(&v, "model")?;
        }
        if !matches!(event, "SessionStart" | "SessionEnd") {
            keys.push("turn_id");
            identity(&v, "turn_id")?;
            p["native_turn_id"] = v["turn_id"].clone();
            p["correlation"] = json!("native_turn");
        }
        if !matches!(event, "PreModelRequest" | "PostModelRequest" | "SessionEnd") {
            keys.push("permission_mode");
        }
    } else {
        keys.extend(["timestamp", "parent_agent_id"]);
        p["source_timestamp_ms"] = json!(timestamp(text(&v, "timestamp")?)?);
    }
    match event {
        "SessionStart" => {
            keys.push("source");
            require(
                if codex {
                    matches!(
                        text(&v, "source")?,
                        "startup" | "resume" | "clear" | "compact" | "fork"
                    )
                } else {
                    matches!(text(&v, "source")?, "startup" | "resume" | "new")
                },
                "agent_invalid_schema",
            )?;
            p["source"] = v["source"].clone();
            if !codex {
                keys.push("initial_prompt");
            }
        }
        "SessionEnd" => {
            keys.push("reason");
            require(
                if codex {
                    v["reason"] == "other"
                } else {
                    matches!(
                        text(&v, "reason")?,
                        "complete" | "error" | "abort" | "timeout" | "user_exit"
                    )
                },
                "agent_invalid_schema",
            )?;
            p["reason"] = v["reason"].clone();
        }
        "PostModelRequest" => {
            keys.extend(["status", "error", "token_usage"]);
            require(
                matches!(text(&v, "status")?, "completed" | "failed" | "cancelled"),
                "agent_invalid_schema",
            )?;
            p["status"] = v["status"].clone();
        }
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
            keys.extend(["tool_name", "tool_input"]);
            text(&v, "tool_name")?;
            if codex {
                keys.push("tool_use_id");
                identity(&v, "tool_use_id")?;
                p["native_tool_id"] = v["tool_use_id"].clone();
                p["correlation"] = json!("native_tool");
            }
            if event == "PostToolUse" {
                keys.push(if codex {
                    "tool_response"
                } else {
                    "tool_result"
                });
            }
            if event == "PostToolUseFailure" {
                keys.push("error");
            }
        }
        "Stop" => {
            keys.push("stop_hook_active");
            require(v["stop_hook_active"].is_boolean(), "agent_invalid_schema")?;
            if codex {
                keys.push("last_assistant_message");
            } else {
                keys.extend(["transcript_path", "stop_reason"]);
                require(v["stop_reason"] == "end_turn", "agent_invalid_schema")?;
                p["reason"] = json!("end_turn");
            }
        }
        "UserPromptSubmit" => keys.push("prompt"),
        _ => {}
    }
    require(
        v.as_object()
            .is_some_and(|o| o.keys().all(|k| keys.contains(&k.as_str()))),
        "agent_invalid_schema",
    )?;
    require(
        !["agent_id", "agent_type", "parent_agent_id"]
            .iter()
            .any(|k| v.get(k).is_some_and(|v| !v.is_null())),
        "agent_child_excluded",
    )?;
    // Check optional native metadata without retaining or interpreting its text.
    for key in [
        "transcript_path",
        "last_assistant_message",
        "error",
        "initial_prompt",
        "prompt",
    ] {
        if let Some(value) = v.get(key) {
            require(
                value.is_null() && codex || value.is_string(),
                "agent_invalid_schema",
            )?;
        }
    }
    if let Some(mode) = v.get("permission_mode") {
        require(
            mode.as_str().is_some_and(|m| {
                matches!(
                    m,
                    "default" | "acceptEdits" | "plan" | "dontAsk" | "bypassPermissions"
                )
            }),
            "agent_invalid_schema",
        )?;
    }
    params(&p)?;
    Ok(p)
}

pub(super) fn params(v: &Value) -> Result<()> {
    let provider = text(v, "provider")?;
    let event = text(v, "event")?;
    require(
        events(provider)?.contains(&event) && v["profile"] == format!("{provider}-hook-facts.v1"),
        "agent_unsupported_profile",
    )?;
    identity(v, "session_id")?;
    let mut keys = vec!["provider", "profile", "session_id", "event", "correlation"];
    let codex = provider == "codex";
    let turn = codex && !matches!(event, "SessionStart" | "SessionEnd");
    let tool = codex && matches!(event, "PreToolUse" | "PostToolUse");
    require(
        v["correlation"]
            == if tool {
                "native_tool"
            } else if turn {
                "native_turn"
            } else {
                "none"
            },
        "agent_invalid_schema",
    )?;
    if turn {
        keys.push("native_turn_id");
        identity(v, "native_turn_id")?;
    }
    if tool {
        keys.push("native_tool_id");
        identity(v, "native_tool_id")?;
    }
    if !codex {
        keys.push("source_timestamp_ms");
        require(
            v["source_timestamp_ms"]
                .as_u64()
                .is_some_and(|n| n <= 253_402_300_799_999),
            "agent_invalid_schema",
        )?;
    }
    if event == "SessionStart" {
        keys.push("source");
        require(
            if codex {
                matches!(
                    text(v, "source")?,
                    "startup" | "resume" | "clear" | "compact" | "fork"
                )
            } else {
                matches!(text(v, "source")?, "startup" | "resume" | "new")
            },
            "agent_invalid_schema",
        )?;
    }
    if event == "PostModelRequest" {
        keys.push("status");
        require(
            matches!(text(v, "status")?, "completed" | "failed" | "cancelled"),
            "agent_invalid_schema",
        )?;
    }
    if event == "SessionEnd" || (!codex && event == "Stop") {
        keys.push("reason");
        require(
            if codex {
                v["reason"] == "other"
            } else if event == "Stop" {
                v["reason"] == "end_turn"
            } else {
                matches!(
                    text(v, "reason")?,
                    "complete" | "error" | "abort" | "timeout" | "user_exit"
                )
            },
            "agent_invalid_schema",
        )?;
    }
    require(
        v.as_object()
            .is_some_and(|o| o.len() == keys.len() && o.keys().all(|k| keys.contains(&k.as_str()))),
        "agent_invalid_schema",
    )
}

pub fn report_arguments(
    socket: &str,
    provider: &str,
    event: &str,
    json_mode: bool,
) -> Result<TaskOutput> {
    let mut raw = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_PAYLOAD + 1) as u64)
        .read_to_end(&mut raw)?;
    let p = capture(provider, event, &raw)?;
    output(
        super::unix::call_contract(socket, RPC, "report", p)?,
        json_mode,
    )
}

/// Pure generation: intentionally does not inspect, adopt, enable or mutate roots.
pub fn plan(
    provider: &str,
    executable: &str,
    root: &std::path::Path,
    route: &[String],
    json_mode: bool,
) -> Result<TaskOutput> {
    require(
        root.is_absolute()
            && std::path::Path::new(executable).is_absolute()
            && !executable.chars().any(char::is_control),
        "agent_invalid_schema",
    )?;
    if let Some(socket) = route
        .get(1)
        .filter(|_| route.first().is_some_and(|s| s == "--socket"))
    {
        crate::observation_socket::validate_path(socket)?;
    }
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut hooks = serde_json::Map::new();
    for event in events(provider)? {
        let mut args = vec![
            "agent".into(),
            "report-fact".into(),
            "--provider".into(),
            provider.into(),
            "--event".into(),
            (*event).into(),
            "--best-effort".into(),
        ];
        args.extend_from_slice(route);
        let handler = if provider == "codex" {
            let command = std::iter::once(executable)
                .chain(args.iter().map(String::as_str))
                .map(quote)
                .collect::<Vec<_>>()
                .join(" ");
            json!([{"hooks":[{"type":"command","command":command,"timeout":5}]}])
        } else {
            json!([{"type":"command","exec":executable,"args":args,"timeoutSec":5}])
        };
        hooks.insert((*event).into(), handler);
    }
    output(
        json!({"contract":"coven.native-hook-plan.v1","provider":provider,"profile":format!("{provider}-hook-facts.v1"),"mode":"inert_manual_handoff","applied":false,"configuration_snippet":{"hooks":hooks},"installation":"not_implemented","source_qualification":if provider == "codex" {"openai/codex@0a94398fa60c4fac0e7659e4213a0a36e613dd46 (rust-v0.167.1); source-qualified, not live-certified"} else {"github/docs@7c3387d3b513ed43ea26c9b2e4c0e1b12234c010; documentation only, not binary certified"},"policy":"preserve existing trust, disable, allow and enable settings; never merge into a managed reference receipt image"}),
        json_mode,
    )
}

struct Record {
    provider: String,
    event: String,
    at: Instant,
    source: Option<u64>,
    age: u64,
    ambiguous: bool,
}
#[derive(Default)]
pub(super) struct FactsStore {
    records: BTreeMap<String, Record>,
}
impl FactsStore {
    pub fn report(&mut self, p: &Value, now: Instant, wall: u64) -> Result<Value> {
        params(p)?;
        self.records
            .retain(|_, r| now.saturating_duration_since(r.at) < Duration::from_secs(600));
        let source = p["source_timestamp_ms"].as_u64();
        let id = format!(
            "{:x}",
            Sha256::digest(
                [
                    b"coven.provider-facts.v1\0".as_slice(),
                    text(p, "provider")?.as_bytes(),
                    b"\0",
                    text(p, "session_id")?.as_bytes()
                ]
                .concat()
            )
        );
        if let Some(old) = self.records.get_mut(&id) {
            if let (Some(s), Some(o)) = (source, old.source) {
                if s <= o {
                    if s == o && p["event"] != old.event {
                        old.ambiguous = true;
                    }
                    return Ok(json!({"type":"accepted","delivery":"volatile"}));
                }
            }
        }
        if let Some(s) = source {
            require(
                s <= wall.saturating_add(1000) && wall.saturating_sub(s) < 30_000,
                "agent_source_time_rejected",
            )?;
        }
        require(
            self.records.contains_key(&id) || self.records.len() < 128,
            "agent_capacity",
        )?;
        self.records.insert(
            id,
            Record {
                provider: text(p, "provider")?.into(),
                event: text(p, "event")?.into(),
                at: now,
                source,
                age: source.map_or(0, |s| wall.saturating_sub(s)),
                ambiguous: false,
            },
        );
        Ok(json!({"type":"accepted","delivery":"volatile"}))
    }
    pub fn rows(&self, now: Instant) -> Vec<Value> {
        self.records.iter().filter(|(_,r)| now.saturating_duration_since(r.at) < Duration::from_secs(600)).map(|(id,r)| {
            let elapsed = now.saturating_duration_since(r.at).as_millis().min(u64::MAX as u128) as u64;
            json!({"observation_id":id,"provider":r.provider,"profile":format!("{}-hook-facts.v1",r.provider),"origin":"provider_native_fact","capability":"native_hook_fact","reported_state":"unknown","age_ms":elapsed,"freshness":if r.source.is_none() {"unknown"} else if r.age.saturating_add(elapsed) < 30_000 {"fresh"} else {"expired"},"reference_trust":"caller_supplied_unverified","root_eligibility":"unestablished","native_identity":"unestablished","job_completion":"unestablished","binding":"none","attribution":"Coven","last_received_event":if r.ambiguous {"ambiguous_same_timestamp"} else {&r.event},"ordering":"unordered","time_basis":if r.source.is_none() {"local_receipt_only"} else {"source_age_at_receipt_plus_monotonic"},"source_time":if r.source.is_none() {"unknown"} else {"caller_supplied_utc"}})
        }).collect()
    }
}
pub(super) fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(crate) fn validate_row(v: &Value) -> Result<()> {
    let provider = text(v, "provider")?;
    let keys = [
        "observation_id",
        "provider",
        "profile",
        "origin",
        "capability",
        "reported_state",
        "age_ms",
        "freshness",
        "reference_trust",
        "root_eligibility",
        "native_identity",
        "job_completion",
        "binding",
        "attribution",
        "last_received_event",
        "ordering",
        "time_basis",
        "source_time",
    ];
    require(
        v.as_object()
            .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
            && v["profile"] == format!("{provider}-hook-facts.v1")
            && v["origin"] == "provider_native_fact"
            && v["capability"] == "native_hook_fact"
            && v["reported_state"] == "unknown"
            && v["age_ms"].as_u64().is_some()
            && v["reference_trust"] == "caller_supplied_unverified"
            && v["root_eligibility"] == "unestablished"
            && v["native_identity"] == "unestablished"
            && v["job_completion"] == "unestablished"
            && v["binding"] == "none"
            && v["attribution"] == "Coven"
            && v["ordering"] == "unordered"
            && (events(provider)?.contains(&text(v, "last_received_event")?)
                || provider == "copilot" && v["last_received_event"] == "ambiguous_same_timestamp")
            && if provider == "codex" {
                v["time_basis"] == "local_receipt_only"
                    && v["source_time"] == "unknown"
                    && v["freshness"] == "unknown"
            } else {
                v["time_basis"] == "source_age_at_receipt_plus_monotonic"
                    && v["source_time"] == "caller_supplied_utc"
                    && matches!(text(v, "freshness")?, "fresh" | "expired")
            },
        "agent_invalid_schema",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(provider: &str, event: &str) -> Value {
        let mut v = json!({"session_id":"PRIVATE-SID","hook_event_name":event,"cwd":"/PRIVATE"});
        if provider == "codex" {
            if event != "SessionEnd" {
                v["model"] = json!("PRIVATE-MODEL");
            }
            if !matches!(event, "SessionStart" | "SessionEnd") {
                v["turn_id"] = json!("PRIVATE-TURN");
            }
        } else {
            v["timestamp"] = json!("2024-02-29T00:00:00.000Z");
        }
        match event {
            "SessionStart" => v["source"] = json!("startup"),
            "SessionEnd" => {
                v["reason"] = json!(if provider == "codex" {
                    "other"
                } else {
                    "complete"
                })
            }
            "PostModelRequest" => {
                v["status"] = json!("completed");
                v["error"] = json!("PRIVATE-ERROR");
                v["token_usage"] = Value::Null;
            }
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                v["tool_name"] = json!("PRIVATE-TOOL");
                v["tool_input"] = json!({"secret":"PRIVATE"});
                if provider == "codex" {
                    v["tool_use_id"] = json!("PRIVATE-TOOL-ID");
                }
            }
            "Stop" => {
                v["stop_hook_active"] = json!(false);
                if provider == "copilot" {
                    v["stop_reason"] = json!("end_turn");
                }
            }
            _ => {}
        }
        v
    }
    fn captured(provider: &str, event: &str) -> Value {
        capture(
            provider,
            event,
            payload(provider, event).to_string().as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn facts_selectors_are_closed_and_private_payloads_are_dropped() {
        for provider in ["codex", "copilot"] {
            for event in events(provider).unwrap() {
                let v = payload(provider, event);
                let p = captured(provider, event);
                params(&p).unwrap();
                for private in [
                    "PRIVATE-MODEL",
                    "PRIVATE-ERROR",
                    "/PRIVATE",
                    "PRIVATE-TOOL\"",
                ] {
                    assert!(!p.to_string().contains(private));
                }
                for key in ["sequence", "instance_id", "timestamp_ms", "promptText"] {
                    let mut bad = v.clone();
                    bad[key] = json!("PRIVATE");
                    assert!(capture(provider, event, bad.to_string().as_bytes()).is_err());
                }
                for key in ["agent_id", "agent_type"] {
                    let mut child = v.clone();
                    child[key] = json!("child");
                    assert!(capture(provider, event, child.to_string().as_bytes()).is_err());
                }
                let other = if provider == "codex" {
                    "copilot"
                } else {
                    "codex"
                };
                let mut bad = p.clone();
                bad["provider"] = json!(other);
                assert!(params(&bad).is_err());
                assert!(capture(provider, "sessionStart", v.to_string().as_bytes()).is_err());
            }
        }
    }
    #[test]
    fn facts_duplicate_depth_size_and_event_mismatch_reject() {
        assert!(
            capture(
                "codex",
                "SessionStart",
                br#"{"session_id":"a","session_id":"b"}"#
            )
            .is_err()
        );
        assert!(capture("codex", "SessionStart", &vec![b' '; MAX_PAYLOAD + 1]).is_err());
        let nested = format!("{}0{}", "[".repeat(33), "]".repeat(33));
        assert!(capture("codex", "SessionStart", nested.as_bytes()).is_err());
        assert!(
            capture(
                "codex",
                "Stop",
                payload("codex", "Interrupt").to_string().as_bytes()
            )
            .is_err()
        );
    }
    #[test]
    fn facts_dates_validate_calendar_utc_and_fraction() {
        assert_eq!(timestamp("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(timestamp("1970-01-01T00:00:00.1Z").unwrap(), 100);
        assert_eq!(timestamp("1970-01-01T00:00:00.12Z").unwrap(), 120);
        assert_eq!(
            timestamp("2000-03-01T00:00:00Z").unwrap() - timestamp("2000-02-29T00:00:00Z").unwrap(),
            86_400_000
        );
        for s in [
            "1900-02-29T00:00:00Z",
            "2023-02-29T00:00:00Z",
            "2024-04-31T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:00:60Z",
            "2024-01-01T00:00:00+00:00",
            "2024-01-01T00:00:00.Z",
            "2024-01-01T00:00:00.1234Z",
        ] {
            assert!(timestamp(s).is_err(), "{s}");
        }
    }
    #[test]
    fn facts_source_watermarks_equal_ambiguity_and_monotonic_expiry() {
        let now = Instant::now();
        let mut store = FactsStore::default();
        let mut p = captured("copilot", "SessionStart");
        let source = p["source_timestamp_ms"].as_u64().unwrap();
        store.report(&p, now, source + 29_000).unwrap();
        store
            .report(&p, now + Duration::from_millis(500), source + 29_500)
            .unwrap();
        assert_eq!(
            store.rows(now + Duration::from_secs(1))[0]["freshness"],
            "expired"
        );
        p["event"] = json!("UserPromptSubmit");
        p.as_object_mut().unwrap().remove("source");
        store
            .report(&p, now + Duration::from_secs(2), source - 5000)
            .unwrap();
        assert_eq!(
            store.rows(now + Duration::from_secs(2))[0]["last_received_event"],
            "ambiguous_same_timestamp"
        );
        p["source_timestamp_ms"] = json!(source - 1);
        store
            .report(&p, now + Duration::from_secs(3), source - 5000)
            .unwrap();
        assert_eq!(
            store.rows(now + Duration::from_secs(3))[0]["last_received_event"],
            "ambiguous_same_timestamp"
        );
        assert!(store.rows(now + Duration::from_secs(600)).is_empty());
    }
    #[test]
    fn facts_future_stale_and_late_codex_never_manufacture_freshness() {
        let now = Instant::now();
        let p = captured("copilot", "Stop");
        let s = p["source_timestamp_ms"].as_u64().unwrap();
        for wall in [s - 1001, s + 30_000] {
            assert!(FactsStore::default().report(&p, now, wall).is_err());
        }
        let mut store = FactsStore::default();
        store.report(&p, now, s - 1000).unwrap();
        assert_eq!(
            store.rows(now + Duration::from_secs(30))[0]["freshness"],
            "expired"
        );
        for event in ["Stop", "SessionEnd", "PreModelRequest"] {
            let p = captured("codex", event);
            store.report(&p, now, 0).unwrap();
            let rows = store.rows(now + Duration::from_secs(31));
            let r = rows.iter().find(|r| r["provider"] == "codex").unwrap();
            assert_eq!(r["freshness"], "unknown");
            assert_eq!(r["reported_state"], "unknown");
            validate_row(r).unwrap();
        }
    }
    #[test]
    fn facts_capacity_retention_repeats_and_privacy_substitution() {
        let now = Instant::now();
        let mut store = FactsStore::default();
        let mut p = captured("codex", "Stop");
        for _ in 0..1000 {
            store.report(&p, now, 0).unwrap();
        }
        assert_eq!(store.records.len(), 1);
        for i in 1..128 {
            p["session_id"] = json!(format!("private{i}"));
            store.report(&p, now, 0).unwrap();
        }
        p["session_id"] = json!("overflow");
        assert!(store.report(&p, now, 0).is_err());
        let row = store.rows(now).remove(0);
        assert!(!row.to_string().contains("PRIVATE"));
        for key in row.as_object().unwrap().keys() {
            let mut bad = row.clone();
            bad.as_object_mut().unwrap().remove(key);
            bad["private"] = json!("PRIVATE");
            assert!(validate_row(&bad).is_err(), "{key}");
        }
        store.report(&p, now + Duration::from_secs(600), 0).unwrap();
        assert_eq!(store.records.len(), 1);
    }
    #[test]
    fn facts_merge_preserves_attention_priority_and_shared_limit() {
        let now = Instant::now();
        let mut store = native::NativeStore::default();
        store.report_provider(&json!({"provider":"opencode","profile":"opencode-root.v1","instance_id":"12345678-1234-1234-1234-123456789abc","sequence":1,"session_id":"private","event":"permission.asked","request_id":"private"}),now).unwrap();
        let mut p = captured("codex", "SessionEnd");
        for i in 0..128 {
            p["session_id"] = json!(format!("private{i}"));
            store.facts.report(&p, now, 0).unwrap();
        }
        let view = store.projection(json!({"server_epoch":"a".repeat(32),"records":[]}), now);
        native::validate_control_result("observations", &view).unwrap();
        assert_eq!(view["observations"].as_array().unwrap().len(), 127);
        assert_eq!(view["observations"][0]["reported_state"], "blocked");
        assert_eq!(view["omitted_observations"], 2);
    }
    #[test]
    fn facts_generation_is_deterministic_inert_and_policy_neutral() {
        for provider in ["codex", "copilot"] {
            let make = || {
                plan(
                    provider,
                    "/inert/coven",
                    std::path::Path::new("/unread/root"),
                    &["--socket".into(), "/inert/s".into()],
                    true,
                )
                .unwrap()
                .stdout
            };
            let a = make();
            assert_eq!(a, make());
            assert!(a.contains("--best-effort"));
            assert!(!a.contains("--json"));
            assert!(!a.contains("async"));
            assert!(!a.contains("features.hooks"));
        }
    }
}
