//! Closed native decision wire types. Selectors and full details stay private.
use crate::observation_socket::{Delivery, Failure, WireResult};
use crate::session_prompts::{fresh_id, protocol::uuid};
use crate::shared_native::{Connection, Grant};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(crate) const CONTRACT: &str = "djinn.decision-control.v1";
fn invalid() -> Failure {
    Failure::new("invalid_decision")
}
pub(crate) fn capability() -> Value {
    json!({"contract":CONTRACT,"version":1,"max_response_bytes":65536,
        "actions":["question_answer","question_reject","permission_once","permission_deny"]})
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Question,
    Permission,
}
impl Kind {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Permission => "permission",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Meta {
    pub kind: Kind,
    pub request_id: String,
    pub occurrence_id: String,
    pub details_sha256: Option<String>,
    pub details_available: bool,
}
impl Meta {
    fn valid(&self) -> bool {
        text(&self.request_id, 512)
            && uuid(&self.occurrence_id)
            && self.details_sha256.as_ref().is_none_or(|s| hash(s))
            && self.details_available == self.details_sha256.is_some()
    }
    pub(crate) fn params(&self, connection: &Connection<'_>, grant: &Grant) -> Value {
        let mut p = connection.params(grant);
        p["kind"] = json!(self.kind);
        p["request_id"] = json!(self.request_id);
        p["occurrence_id"] = json!(self.occurrence_id);
        p["details_sha256"] = json!(self.details_sha256);
        p
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    r#type: String,
    contract: String,
    grant_id: String,
    session_id: String,
    pub requests: Vec<Meta>,
    pub complete: bool,
    pub omitted_count: Option<u64>,
    pub can_respond: bool,
}
pub(crate) fn summary(connection: &mut Connection<'_>, grant: &Grant) -> WireResult<Summary> {
    connection.require_decisions(grant)?;
    let v = connection.call("shared_attention", connection.params(grant), &fresh_id()?)?;
    let s = summary_result(v, grant)?;
    connection.require_grant(grant, false)?;
    Ok(s)
}
fn summary_result(v: Value, grant: &Grant) -> WireResult<Summary> {
    // Option fields are nullable but not optional on the wire.
    if v.get("omitted_count").is_none()
        || v["requests"]
            .as_array()
            .is_none_or(|a| a.iter().any(|r| r.get("details_sha256").is_none()))
    {
        return Err(invalid());
    }
    let s: Summary = serde_json::from_value(v).map_err(|_| invalid())?;
    let mut seen = HashSet::new();
    if s.r#type != "shared_attention"
        || s.contract != CONTRACT
        || s.grant_id != grant.grant_id
        || s.session_id != grant.session_id
        || s.requests.len() > 32
        || (s.complete && s.omitted_count.is_none())
        || s.requests
            .iter()
            .any(|r| !r.valid() || !seen.insert((r.kind.name(), &r.request_id, &r.occurrence_id)))
    {
        return Err(invalid());
    }
    Ok(s)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OptionRow {
    label: String,
    description: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
    header: String,
    options: Vec<OptionRow>,
    multiple: bool,
    custom: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Tool {
    message_id: String,
    call_id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Detail {
    Question {
        request_id: String,
        occurrence_id: String,
        details_sha256: String,
        questions: Vec<Question>,
        tool: Option<Tool>,
    },
    Permission {
        request_id: String,
        occurrence_id: String,
        details_sha256: String,
        permission: String,
        patterns: Vec<String>,
        always_patterns: Vec<String>,
        metadata_json: String,
        tool: Option<Tool>,
        deny_scope: String,
    },
}
pub(crate) struct Verified {
    detail: Detail,
    pub sanitized: Value,
}
fn bounded_tree(v: &Value, depth: usize, nodes: &mut usize) -> bool {
    *nodes += 1;
    if depth > 12 || *nodes > 2048 {
        return false;
    }
    match v {
        Value::String(s) => s.len() <= 8192,
        Value::Array(a) => a.len() <= 2048 && a.iter().all(|v| bounded_tree(v, depth + 1, nodes)),
        Value::Object(o) => {
            o.len() <= 2048
                && o.iter()
                    .all(|(k, v)| k.len() <= 8192 && bounded_tree(v, depth + 1, nodes))
        }
        _ => true,
    }
}
fn verify(v: Value, meta: &Meta) -> WireResult<Verified> {
    fn metadata_tree(v: &crate::values::Value, depth: usize, nodes: &mut usize) -> bool {
        use crate::values::{Number, Value as V};
        *nodes += 1;
        if depth > 12 || *nodes > 2048 {
            return false;
        }
        match v {
            V::String(s) => s.len() <= 8192,
            V::Array(a) => a.iter().all(|v| metadata_tree(v, depth + 1, nodes)),
            V::Object(o) => o
                .entries()
                .iter()
                .all(|(k, v)| k.len() <= 8192 && metadata_tree(v, depth + 1, nodes)),
            V::Number(Number::Integer(n)) => n.as_str().parse::<f64>().is_ok_and(f64::is_finite),
            V::Number(Number::Float(n)) => n.value().is_finite(),
            _ => true,
        }
    }
    let mut nodes = 0;
    if !meta.valid()
        || !meta.details_available
        || v.get("tool").is_none()
        || !bounded_tree(&v, 0, &mut nodes)
        || serde_json::to_vec(&v).map_err(|_| invalid())?.len() > 32768
    {
        return Err(invalid());
    }
    let detail: Detail = serde_json::from_value(v.clone()).map_err(|_| invalid())?;
    let tool_tuple = |t: &Option<Tool>| -> WireResult<Value> {
        match t {
            None => Ok(Value::Null),
            Some(t) if text(&t.message_id, 512) && text(&t.call_id, 512) => {
                Ok(json!([t.message_id, t.call_id]))
            }
            _ => Err(invalid()),
        }
    };
    let (request, occurrence, digest, tuple) = match &detail {
        Detail::Question {
            request_id,
            occurrence_id,
            details_sha256,
            questions,
            tool,
        } => {
            if meta.kind != Kind::Question
                || questions.is_empty()
                || questions.len() > 8
                || questions.iter().any(|q| {
                    q.question.len() > 4096
                        || q.header.len() > 256
                        || q.options.len() > 16
                        || q.options.iter().any(|o| {
                            o.label.is_empty() || o.label.len() > 256 || o.description.len() > 1024
                        })
                        || q.options
                            .iter()
                            .map(|o| &o.label)
                            .collect::<HashSet<_>>()
                            .len()
                            != q.options.len()
                })
            {
                return Err(invalid());
            }
            let qs: Vec<_> = questions
                .iter()
                .map(|q| {
                    json!([
                        q.question,
                        q.header,
                        q.options
                            .iter()
                            .map(|o| json!([o.label, o.description]))
                            .collect::<Vec<_>>(),
                        q.multiple,
                        q.custom
                    ])
                })
                .collect();
            (
                request_id,
                occurrence_id,
                details_sha256,
                json!([
                    CONTRACT,
                    "question",
                    request_id,
                    occurrence_id,
                    qs,
                    tool_tuple(tool)?
                ]),
            )
        }
        Detail::Permission {
            request_id,
            occurrence_id,
            details_sha256,
            permission,
            patterns,
            always_patterns,
            metadata_json,
            tool,
            deny_scope,
        } => {
            if meta.kind != Kind::Permission
                || permission.is_empty()
                || permission.len() > 1024
                || patterns.len() > 32
                || always_patterns.len() > 32
                || patterns
                    .iter()
                    .chain(always_patterns)
                    .any(|s| s.len() > 1024)
                || metadata_json.len() > 4096
                || deny_scope != "session_pending"
            {
                return Err(invalid());
            }
            let parsed =
                crate::values::parse(metadata_json.as_bytes(), crate::values::Profile::Authority)
                    .map_err(|_| invalid())?;
            // Hash the exact canonical native string, not Rust's number/key formatting.
            if !metadata_tree(&parsed, 0, &mut nodes) {
                return Err(invalid());
            }
            (
                request_id,
                occurrence_id,
                details_sha256,
                json!([
                    CONTRACT,
                    "permission",
                    request_id,
                    occurrence_id,
                    permission,
                    patterns,
                    always_patterns,
                    metadata_json,
                    tool_tuple(tool)?,
                    "session_pending"
                ]),
            )
        }
    };
    if request != &meta.request_id
        || occurrence != &meta.occurrence_id
        || Some(digest) != meta.details_sha256.as_ref()
        || format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&tuple).map_err(|_| invalid())?)
        ) != *digest
    {
        return Err(invalid());
    }
    let mut sanitized = v;
    for key in ["request_id", "occurrence_id", "details_sha256", "tool"] {
        sanitized.as_object_mut().ok_or_else(invalid)?.remove(key);
    }
    Ok(Verified { detail, sanitized })
}
pub(crate) fn details(c: &mut Connection<'_>, grant: &Grant, meta: &Meta) -> WireResult<Verified> {
    c.require_decisions(grant)?;
    let v = c.call("shared_decision", meta.params(c, grant), &fresh_id()?)?;
    if v.as_object().is_none_or(|o| o.len() != 5)
        || v["type"] != "shared_decision"
        || v["contract"] != CONTRACT
        || v["grant_id"] != grant.grant_id
        || v["session_id"] != grant.session_id
    {
        return Err(invalid());
    }
    let verified = verify(v["decision"].clone(), meta)?;
    c.require_grant(grant, false)?;
    Ok(verified)
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    QuestionAnswer { answers: Vec<Vec<String>> },
    QuestionReject,
    PermissionOnce,
    PermissionDeny { scope: String },
}
impl Action {
    pub(crate) fn parse(v: &Value) -> WireResult<Self> {
        let keys: &[&str] = match v["type"].as_str() {
            Some("question_answer") => &["type", "answers"],
            Some("question_reject" | "permission_once") => &["type"],
            Some("permission_deny") => &["type", "scope"],
            _ => return Err(invalid()),
        };
        // Serde unit variants otherwise accept extra fields on internally tagged enums.
        if v.as_object()
            .is_none_or(|o| o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)))
        {
            return Err(invalid());
        }
        if !bounded_tree(v, 0, &mut 0)
            || serde_json::to_vec(v).map_err(|_| invalid())?.len() > 65536
        {
            return Err(invalid());
        }
        let a: Self = serde_json::from_value(v.clone()).map_err(|_| invalid())?;
        if let Self::PermissionDeny { scope } = &a {
            if scope != "session_pending" {
                return Err(invalid());
            }
        }
        Ok(a)
    }
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::QuestionAnswer { .. } => "question_answer",
            Self::QuestionReject => "question_reject",
            Self::PermissionOnce => "permission_once",
            Self::PermissionDeny { .. } => "permission_deny",
        }
    }
    pub(crate) fn validate(&self, verified: &Verified) -> WireResult<()> {
        match (self, &verified.detail) {
            (Self::QuestionReject, Detail::Question { .. })
            | (Self::PermissionOnce | Self::PermissionDeny { .. }, Detail::Permission { .. }) => {
                Ok(())
            }
            (Self::QuestionAnswer { answers }, Detail::Question { questions, .. }) => {
                let mut total = 0;
                if answers.len() != questions.len() {
                    return Err(invalid());
                }
                for (answers, q) in answers.iter().zip(questions) {
                    if (!q.multiple && answers.len() != 1)
                        || answers.len() > q.options.len() + usize::from(q.custom)
                        || answers.iter().collect::<HashSet<_>>().len() != answers.len()
                    {
                        return Err(invalid());
                    }
                    let mut custom = 0;
                    for a in answers {
                        total += a.len();
                        if a.trim().is_empty()
                            || a.len() > 4096
                            || a.chars()
                                .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
                        {
                            return Err(invalid());
                        }
                        if !q.options.iter().any(|o| o.label == *a) {
                            custom += 1;
                        }
                    }
                    if custom > usize::from(q.custom) {
                        return Err(invalid());
                    }
                }
                if total > 8192 {
                    return Err(invalid());
                }
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
}
pub(crate) fn decide(
    c: &mut Connection<'_>,
    grant: &Grant,
    meta: &Meta,
    attempt: &str,
    action: &Action,
    before_send: impl FnOnce() -> WireResult<()>,
) -> WireResult<()> {
    let mut p = meta.params(c, grant);
    p["action"] = serde_json::to_value(action).map_err(|_| invalid())?;
    let v = c.call_checked("shared_decide", p, attempt, before_send)?;
    if v != json!({"type":"shared_decision_applied","contract":CONTRACT,"grant_id":grant.grant_id,
        "session_id":grant.session_id,"request_id":meta.request_id,"occurrence_id":meta.occurrence_id,"action":action.name()})
    {
        return Err(Failure {
            code: "decision_uncertain".into(),
            delivery: Delivery::Uncertain,
            interrupted: false,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const OCCURRENCE: &str = "11111111-2222-4333-8444-555555555555";
    fn question() -> (Value, Meta) {
        let digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!([
                    CONTRACT,
                    "question",
                    "ask",
                    OCCURRENCE,
                    [["Choose", "Header", [["one", "Description"]], false, false]],
                    null
                ]))
                .unwrap()
            )
        );
        (
            json!({"kind":"question","request_id":"ask","occurrence_id":OCCURRENCE,"details_sha256":digest,
            "questions":[{"question":"Choose","header":"Header","options":[{"label":"one","description":"Description"}],"multiple":false,"custom":false}],"tool":null}),
            Meta {
                kind: Kind::Question,
                request_id: "ask".into(),
                occurrence_id: OCCURRENCE.into(),
                details_sha256: Some(digest),
                details_available: true,
            },
        )
    }
    #[test]
    fn canonical_tuple_verification_and_exact_sanitization() {
        let (v, m) = question();
        let d = verify(v.clone(), &m).unwrap();
        assert_eq!(
            d.sanitized,
            json!({"kind":"question","questions":v["questions"]})
        );
        for (pointer, wrong) in [
            ("/request_id", json!("other")),
            (
                "/occurrence_id",
                json!("21111111-2222-4333-8444-555555555555"),
            ),
            ("/questions/0/question", json!("Changed")),
            ("/questions/0/custom", json!(true)),
            (
                "/tool",
                json!({"message_id":"msg_private","call_id":"tool_private"}),
            ),
        ] {
            let mut bad = v.clone();
            *bad.pointer_mut(pointer).unwrap() = wrong;
            assert!(verify(bad, &m).is_err(), "{pointer}");
        }
        for key in v.as_object().unwrap().keys() {
            let mut bad = v.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(verify(bad, &m).is_err(), "{key}");
        }
        let mut bad = v;
        bad["extra"] = json!(true);
        assert!(verify(bad, &m).is_err());
    }
    #[test]
    fn closed_actions_validate_exact_kind_choices_and_answer_bounds() {
        let (v, m) = question();
        let d = verify(v, &m).unwrap();
        for a in [
            json!({"type":"question_answer","answers":[["one"]]}),
            json!({"type":"question_reject"}),
        ] {
            Action::parse(&a).unwrap().validate(&d).unwrap();
        }
        for a in [
            json!({"type":"question_answer","answers":[["yes"]]}),
            json!({"type":"question_answer","answers":[]}),
            json!({"type":"question_answer","answers":[["one","one"]]}),
            json!({"type":"permission_once"}),
            json!({"type":"question_answer","answers":[[" "]]}),
            json!({"type":"question_answer","answers":[["a".repeat(4097)]]}),
        ] {
            assert!(Action::parse(&a).unwrap().validate(&d).is_err());
        }
        for a in [
            json!({"type":"permission_always"}),
            json!({"type":"permission_deny","scope":"always"}),
            json!({"type":"question_reject","answers":[]}),
            json!({"type":"question_answer","answers":"yes"}),
            json!({"type":"question_answer","answers":[[true]]}),
        ] {
            assert!(Action::parse(&a).is_err());
        }
    }
    #[test]
    fn permission_digest_covers_complete_patterns_scope_metadata_and_private_tool() {
        let tuple = json!([
            CONTRACT,
            "permission",
            "perm",
            OCCURRENCE,
            "shell",
            ["/full/**"],
            ["/remember/**"],
            "{\"a\":1,\"b\":2}",
            ["msg_secret", "tool_secret"],
            "session_pending"
        ]);
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&tuple).unwrap()));
        let m = Meta {
            kind: Kind::Permission,
            request_id: "perm".into(),
            occurrence_id: OCCURRENCE.into(),
            details_sha256: Some(digest.clone()),
            details_available: true,
        };
        let v = json!({"kind":"permission","request_id":"perm","occurrence_id":OCCURRENCE,"details_sha256":digest,"permission":"shell",
            "patterns":["/full/**"],"always_patterns":["/remember/**"],"metadata_json":"{\"a\":1,\"b\":2}","tool":{"message_id":"msg_secret","call_id":"tool_secret"},"deny_scope":"session_pending"});
        let d = verify(v.clone(), &m).unwrap();
        assert_eq!(d.sanitized.as_object().unwrap().len(), 6);
        assert_eq!(d.sanitized["patterns"], v["patterns"]);
        for a in [
            json!({"type":"permission_once"}),
            json!({"type":"permission_deny","scope":"session_pending"}),
        ] {
            Action::parse(&a).unwrap().validate(&d).unwrap();
        }
        for (key, bad) in [
            ("patterns", json!(["a".repeat(1025)])),
            ("patterns", json!(vec!["a"; 33])),
            ("metadata_json", json!("{\"b\":2,\"a\":1}")),
            ("deny_scope", json!("future")),
        ] {
            let mut bad_v = v.clone();
            bad_v[key] = bad;
            assert!(verify(bad_v, &m).is_err());
        }
    }
    #[test]
    fn strict_decoder_rejects_duplicates_large_leaves_depth_and_frames() {
        assert!(
            crate::session_conversations::decode(br#"{"kind":"question","kind":"permission"}"#)
                .is_err()
        );
        assert!(
            crate::session_conversations::decode(
                serde_json::to_string(&"a".repeat(8193)).unwrap().as_bytes()
            )
            .is_err()
        );
        assert!(crate::session_conversations::decode(&vec![b' '; 65536]).is_err());
        let mut nested = json!(null);
        for _ in 0..13 {
            nested = json!([nested]);
        }
        assert!(!bounded_tree(&nested, 0, &mut 0));
    }

    #[test]
    fn javascript_stringify_tuple_and_exact_metadata_number_text_interoperate() {
        let output=std::process::Command::new("node").args(["-e",r#"
            const crypto=require('node:crypto');
            const contract='djinn.decision-control.v1';
            const occurrence='11111111-2222-4333-8444-555555555555';
            const metadata=JSON.stringify({'2':2,'10':10,large:1e21,largeInteger:1e20,negativeInteger:-1e20,small:1e-7});
            const d={kind:'permission',request_id:'private',occurrence_id:occurrence,
                permission:'shell',patterns:['/complete/**','unicode \u00e9 \ud83d\ude80\u2028\u2029'],
                always_patterns:[],metadata_json:metadata,tool:{message_id:'msg_private',call_id:'call_private'},deny_scope:'session_pending'};
            const tuple=[contract,'permission',d.request_id,occurrence,d.permission,d.patterns,d.always_patterns,metadata,[d.tool.message_id,d.tool.call_id],'session_pending'];
            d.details_sha256=crypto.createHash('sha256').update(JSON.stringify(tuple),'utf8').digest('hex');
            process.stdout.write(JSON.stringify(d));
        "#]).output().unwrap();
        assert!(output.status.success());
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        let m = Meta {
            kind: Kind::Permission,
            request_id: "private".into(),
            occurrence_id: OCCURRENCE.into(),
            details_sha256: Some(v["details_sha256"].as_str().unwrap().into()),
            details_available: true,
        };
        assert_eq!(
            verify(v.clone(), &m).unwrap().sanitized["metadata_json"],
            v["metadata_json"]
        );
    }

    #[test]
    fn summaries_are_closed_bounded_correlated_and_preserve_nullable_unknown_counts() {
        let grant = Grant {
            grant_id: OCCURRENCE.into(),
            session_id: "ses_private".into(),
            can_submit: false,
            lease_ms: 5000,
        };
        let v = json!({"type":"shared_attention","contract":CONTRACT,"grant_id":grant.grant_id,"session_id":grant.session_id,
            "requests":[{"kind":"question","request_id":"ask","occurrence_id":OCCURRENCE,"details_sha256":null,"details_available":false}],
            "complete":false,"omitted_count":null,"can_respond":false});
        let s = summary_result(v.clone(), &grant).unwrap();
        assert!(!s.complete);
        assert_eq!(s.omitted_count, None);
        for pointer in [
            "/type",
            "/contract",
            "/grant_id",
            "/session_id",
            "/requests/0/kind",
            "/requests/0/occurrence_id",
        ] {
            let mut bad = v.clone();
            *bad.pointer_mut(pointer).unwrap() = json!("wrong");
            assert!(summary_result(bad, &grant).is_err(), "{pointer}");
        }
        for (pointer, value) in [
            ("/complete", json!(true)),
            ("/omitted_count", json!(-1)),
            ("/omitted_count", json!(0.0)),
            ("/requests/0/request_id", json!("")),
            ("/requests/0/request_id", json!("x".repeat(513))),
            ("/requests/0/request_id", json!("ask\n")),
            ("/requests/0/details_available", json!(true)),
            ("/requests/0/details_sha256", json!("not-a-hash")),
        ] {
            let mut bad = v.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(summary_result(bad, &grant).is_err(), "{pointer}");
        }
        for pointer in ["", "/requests/0"] {
            for key in v.pointer(pointer).unwrap().as_object().unwrap().keys() {
                let mut bad = v.clone();
                bad.pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(summary_result(bad, &grant).is_err(), "{pointer}/{key}");
            }
            let mut bad = v.clone();
            bad.pointer_mut(pointer).unwrap()["extra"] = json!(true);
            assert!(summary_result(bad, &grant).is_err());
        }
        let mut bad = v.clone();
        bad["requests"] = json!(vec![v["requests"][0].clone(); 33]);
        assert!(summary_result(bad, &grant).is_err());
        let mut known = v;
        known["complete"] = json!(true);
        known["omitted_count"] = json!(0);
        assert_eq!(
            summary_result(known, &grant).unwrap().omitted_count,
            Some(0)
        );
    }
}
