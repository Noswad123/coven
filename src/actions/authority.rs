//! Pure historical authority validation, not launch admission or durable replay.
use super::plans;
pub(crate) use super::plans::{build_plan, validate_plan};
use crate::{
    Error, Result,
    error::require,
    platform::WorkspaceLocation,
    values::{Number, Value},
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MAX_WORK: usize = 256 * 1_048_576;
const MAX_AUTHORITY: usize = 16 * 1_048_576;

#[derive(Default)]
pub(super) struct Budget(usize);
impl Budget {
    pub(super) fn charge(&mut self, bytes: usize) -> Result<()> {
        let next = self
            .0
            .checked_add(bytes)
            .ok_or(Error("unsupported_action_work_profile"))?;
        require(next <= MAX_WORK, "unsupported_action_work_profile")?;
        self.0 = next;
        Ok(())
    }
    pub(super) fn canonical(&mut self, value: &Value) -> Result<Vec<u8>> {
        let bytes = value.canonical()?;
        self.charge(bytes.len())?;
        Ok(bytes)
    }
    pub(super) fn spaced(&mut self, value: &Value) -> Result<Vec<u8>> {
        let bytes = value.sorted_spaced()?;
        self.charge(bytes.len())?;
        Ok(bytes)
    }
    fn authority(&mut self, value: &Value) -> Result<Vec<u8>> {
        let bytes = self.canonical(value)?;
        require(
            bytes.len() <= MAX_AUTHORITY,
            "unsupported_action_input_profile",
        )?;
        Ok(bytes)
    }
}

pub(super) fn fatal(error: &Error) -> bool {
    let code = error.code();
    (code.starts_with("unsupported_json_") && code.ends_with("_profile"))
        || matches!(
            code,
            "unsupported_integer_profile"
                | "unsupported_number_token_profile"
                | "unsupported_action_input_profile"
                | "unsupported_action_encoding_profile"
                | "unsupported_checkpoint_input_profile"
                | "unsupported_action_work_profile"
                | "unsupported_platform"
        )
}

pub(super) fn get<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|o| o.get(name))
        .ok_or(Error("invalid_action_fields"))
}
fn fields(value: &Value, names: &str) -> Result<()> {
    let object = value.as_object().ok_or(Error("invalid_action_fields"))?;
    require(
        object.entries().len() == names.split_whitespace().count()
            && names
                .split_whitespace()
                .all(|name| object.get(name).is_some()),
        "invalid_action_fields",
    )
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or(Error("invalid_action_text"))
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn action_id(value: &str) -> Result<()> {
    require(hex(value, 32), "invalid_action_id")
}
fn uuid(value: &Value) -> Result<&str> {
    let value = text(value)?;
    action_id(value)?;
    Ok(value)
}
fn revision(value: &str, prefix: &str) -> Result<()> {
    require(
        value.strip_prefix(prefix).is_some_and(|tail| hex(tail, 64)),
        "invalid_action_reference",
    )
}
fn nonnegative(value: &Value) -> Result<&str> {
    match value {
        Value::Number(Number::Integer(n)) if !n.as_str().starts_with('-') => Ok(n.as_str()),
        _ => Err(Error("invalid_action_integer")),
    }
}
fn location(value: &Value) -> Result<()> {
    fields(value, "root device inode")?;
    let root = text(get(value, "root")?)?;
    // pathlib.Path collapses dot, repeated separators, and trailing separators.
    require(
        root.starts_with('/')
            && !root.starts_with("//")
            && !root.contains('\0')
            && (root == "/"
                || root[1..]
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")),
        "invalid_action_location",
    )?;
    nonnegative(get(value, "device")?)?;
    nonnegative(get(value, "inode")?)?;
    Ok(())
}
fn plan(
    value: &Value,
    loc: &Value,
    workspace: &Value,
    id: &str,
    budget: &mut Budget,
) -> Result<()> {
    plans::validate(value, budget).map_err(|error| {
        if error.code() == "runtime_launch_plan_too_large" {
            Error("unsupported_action_input_profile")
        } else {
            error
        }
    })?;
    require(
        text(get(value, "root")?)? == text(get(loc, "root")?)?
            && text(get(value, "namespace")?)? == format!("coven-{}-{id}", text(workspace)?),
        "invalid_action_plan_identity",
    )
}
fn handle(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub(super) fn report(value: &Value, budget: &mut Budget) -> Result<()> {
    budget.canonical(value)?;
    fields(
        value,
        "operation status completed_steps handles diagnostics outcome_unknown",
    )?;
    require(
        text(get(value, "operation")?)? == "runtime_launch"
            && matches!(
                text(get(value, "status")?)?,
                "transport_completed" | "transport_failed_or_partial"
            ),
        "invalid_action_report",
    )?;
    nonnegative(get(value, "completed_steps")?)?;
    require(
        matches!(get(value, "outcome_unknown")?, Value::Bool(_)),
        "invalid_action_report",
    )?;
    let handles = get(value, "handles")?
        .as_object()
        .ok_or(Error("invalid_action_report"))?;
    for (key, value) in handles.entries() {
        require(handle(key) && handle(text(value)?), "invalid_action_report")?;
    }
    for item in get(value, "diagnostics")?
        .as_array()
        .ok_or(Error("invalid_action_report"))?
    {
        fields(item, "step error message")?;
        if !matches!(get(item, "step")?, Value::Null) {
            nonnegative(get(item, "step")?)?;
        }
        text(get(item, "error")?)?;
        text(get(item, "message")?)?;
    }
    Ok(())
}

/// Only the three consumed references participate in historical lookup.
pub(crate) struct ExpectedRefs<'a> {
    manifest: &'a str,
    checkpoint: &'a str,
    decision: &'a str,
}
impl<'a> ExpectedRefs<'a> {
    pub(crate) fn new(manifest: &'a str, checkpoint: &'a str, decision: &'a str) -> Result<Self> {
        revision(manifest, "runtime-launch-manifest-v1:")?;
        revision(checkpoint, "checkpoint-snapshot-v1:")?;
        action_id(decision)?;
        Ok(Self {
            manifest,
            checkpoint,
            decision,
        })
    }
}
pub(crate) enum Lookup<'a> {
    Absent,
    Existing(&'a Value),
    Match(&'a Value),
    Conflict(&'a Value),
}
pub(crate) struct ValidatedControl(Value);
impl ValidatedControl {
    pub(super) fn validate(
        value: Value,
        actual: &WorkspaceLocation,
        budget: &mut Budget,
    ) -> Result<Self> {
        budget.authority(&value)?;
        fields(
            &value,
            "version action workspace_id epoch_id location head claims",
        )?;
        require(
            get(&value, "version")?.is_integer_one(),
            "invalid_action_version",
        )?;
        require(
            text(get(&value, "action")?)? == "runtime_launch",
            "invalid_action_control",
        )?;
        uuid(get(&value, "workspace_id")?)?;
        uuid(get(&value, "epoch_id")?)?;
        let loc = get(&value, "location")?;
        location(loc)?;
        require(
            text(get(loc, "root")?)? == actual.root
                && nonnegative(get(loc, "device")?)? == actual.device.to_string()
                && nonnegative(get(loc, "inode")?)? == actual.inode.to_string(),
            "action_workspace_location_changed",
        )?;
        let claims = get(&value, "claims")?
            .as_object()
            .ok_or(Error("invalid_action_claims"))?;
        let head = get(&value, "head")?;
        if matches!(head, Value::Null) {
            require(claims.entries().is_empty(), "invalid_action_head")?;
        } else {
            require(claims.get(uuid(head)?).is_some(), "invalid_action_head")?;
        }
        let mut ids = HashSet::new();
        for (id, claim) in claims.entries() {
            action_id(id)?;
            fields(
                claim,
                "claim_id action_id manifest_digest checkpoint_revision decision_id scope_revision plan status report operator_acknowledged",
            )?;
            require(
                ids.insert(uuid(get(claim, "claim_id")?)?) && text(get(claim, "action_id")?)? == id,
                "invalid_action_claim_identity",
            )?;
            ExpectedRefs::new(
                text(get(claim, "manifest_digest")?)?,
                text(get(claim, "checkpoint_revision")?)?,
                text(get(claim, "decision_id")?)?,
            )?;
            revision(text(get(claim, "scope_revision")?)?, "checkpoint-scope-v1:")?;
            plan(
                get(claim, "plan")?,
                loc,
                get(&value, "workspace_id")?,
                id,
                budget,
            )?;
            let status = text(get(claim, "status")?)?;
            let Value::Bool(ack) = get(claim, "operator_acknowledged")? else {
                return Err(Error("invalid_action_claim_status"));
            };
            match status {
                "transport_completed" | "transport_failed_or_partial" => {
                    report(get(claim, "report")?, budget)?;
                    require(
                        text(get(get(claim, "report")?, "status")?)? == status,
                        "invalid_action_claim_report",
                    )?;
                }
                "claimed" | "outcome_unknown" => {
                    require(
                        matches!(get(claim, "report")?, Value::Null),
                        "invalid_action_claim_report",
                    )?;
                    require(
                        if status == "claimed" {
                            !ack && head.as_str() == Some(id)
                        } else {
                            *ack
                        },
                        "invalid_action_claim_status",
                    )?;
                }
                _ => return Err(Error("invalid_action_claim_status")),
            }
        }
        Ok(Self(value))
    }
    pub(crate) fn lookup(
        &self,
        id: &str,
        expected: Option<&ExpectedRefs<'_>>,
    ) -> Result<Lookup<'_>> {
        action_id(id)?;
        let Some(claim) = get(&self.0, "claims")?.as_object().and_then(|o| o.get(id)) else {
            return Ok(Lookup::Absent);
        };
        let Some(expected) = expected else {
            return Ok(Lookup::Existing(claim));
        };
        Ok(
            if text(get(claim, "manifest_digest")?)? == expected.manifest
                && text(get(claim, "checkpoint_revision")?)? == expected.checkpoint
                && text(get(claim, "decision_id")?)? == expected.decision
            {
                Lookup::Match(claim)
            } else {
                Lookup::Conflict(claim)
            },
        )
    }
    pub(super) fn value(&self) -> &Value {
        &self.0
    }
}

pub(super) fn bundle(value: &Value, id: &str, budget: &mut Budget) -> Result<String> {
    let encoded = budget.authority(value)?;
    fields(
        value,
        "version action action_id workspace_id epoch_id location control_revision inputs plan",
    )?;
    require(
        get(value, "version")?.is_integer_one(),
        "invalid_action_version",
    )?;
    require(
        text(get(value, "action")?)? == "runtime_launch",
        "invalid_action_bundle",
    )?;
    require(
        uuid(get(value, "action_id")?)? == id,
        "invalid_action_bundle_identity",
    )?;
    uuid(get(value, "workspace_id")?)?;
    uuid(get(value, "epoch_id")?)?;
    location(get(value, "location")?)?;
    revision(
        text(get(value, "control_revision")?)?,
        "runtime-launch-control-v1:",
    )?;
    require(
        get(value, "inputs")?.as_object().is_some(),
        "invalid_action_inputs",
    )?;
    plan(
        get(value, "plan")?,
        get(value, "location")?,
        get(value, "workspace_id")?,
        id,
        budget,
    )?;
    Ok(label("runtime-launch-manifest-v1:", &encoded))
}
fn label(prefix: &str, bytes: &[u8]) -> String {
    // The label is not a hash preimage domain in the Python contract.
    format!("{prefix}{:x}", Sha256::digest(bytes))
}
pub(super) fn control_revision(value: &Value, budget: &mut Budget) -> Result<String> {
    Ok(label(
        "runtime-launch-control-v1:",
        &budget.authority(value)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{self, Profile};
    use serde_json::json;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const WORKSPACE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    fn parse(value: serde_json::Value) -> Value {
        values::parse(&serde_json::to_vec(&value).unwrap(), Profile::Authority).unwrap()
    }
    fn actual() -> WorkspaceLocation {
        WorkspaceLocation {
            root: "/".to_owned(),
            device: 1,
            inode: 2,
        }
    }
    fn fixture() -> Value {
        let namespace = format!("coven-{WORKSPACE}-{ID}");
        let target = format!("={namespace}");
        let pane = format!("={namespace}:=agent-a");
        parse(json!({
            "version": 1, "action": "runtime_launch", "workspace_id": WORKSPACE,
            "epoch_id": "cccccccccccccccccccccccccccccccc", "location": {"root":"/","device":1,"inode":2},
            "head": ID, "claims": { (ID): {
                "claim_id": "dddddddddddddddddddddddddddddddd", "action_id": ID,
                "manifest_digest": format!("runtime-launch-manifest-v1:{}", "1".repeat(64)),
                "checkpoint_revision": format!("checkpoint-snapshot-v1:{}", "2".repeat(64)),
                "decision_id": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "scope_revision": format!("checkpoint-scope-v1:{}", "3".repeat(64)),
                "status":"claimed", "report":null, "operator_acknowledged":false,
                "plan": {"version":1,"operation":"runtime_launch","root":"/","cwd":"/",
                    "namespace": namespace, "multiplexer":"tmux", "env":{},
                    "agents":[{"id":"a","command":"sh","starter":"go"}],
                    "steps":[
                        {"operation":"ensure_absent","argv":["tmux","has-session","-t",target]},
                        {"operation":"run","argv":["tmux","new-session","-d","-s",namespace,"-n","orchestrator","-c","/","/bin/zsh"]},
                        {"operation":"run","argv":["tmux","new-window","-d","-t",target,"-n","agent-a","-c","/","sh"]},
                        {"operation":"sleep","seconds":5},
                        {"operation":"run","argv":["tmux","send-keys","-t",pane,"-l","--","go"]},
                        {"operation":"run","argv":["tmux","send-keys","-t",pane,"Enter"]}
                    ]}
            }}
        }))
    }
    fn set_claim(control: &mut Value, key: &str, value: Value) {
        let mut claims = get(control, "claims").unwrap().clone();
        let mut claim = get(&claims, ID).unwrap().clone();
        claim.set_field(key, value).unwrap();
        claims.set_field(ID, claim).unwrap();
        control.set_field("claims", claims).unwrap();
    }
    #[test]
    fn lookup_uses_only_validated_control_and_all_three_references() {
        let control =
            ValidatedControl::validate(fixture(), &actual(), &mut Budget::default()).unwrap();
        assert!(matches!(
            control.lookup(ID, None).unwrap(),
            Lookup::Existing(_)
        ));
        assert!(matches!(
            control.lookup(WORKSPACE, None).unwrap(),
            Lookup::Absent
        ));
        let manifest = format!("runtime-launch-manifest-v1:{}", "1".repeat(64));
        let checkpoint = format!("checkpoint-snapshot-v1:{}", "2".repeat(64));
        let decision = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let expected = ExpectedRefs::new(&manifest, &checkpoint, decision).unwrap();
        assert!(matches!(
            control.lookup(ID, Some(&expected)).unwrap(),
            Lookup::Match(_)
        ));
        for (m, c, d) in [
            (
                format!("runtime-launch-manifest-v1:{}", "9".repeat(64)),
                checkpoint.clone(),
                decision.to_owned(),
            ),
            (
                manifest.clone(),
                format!("checkpoint-snapshot-v1:{}", "9".repeat(64)),
                decision.to_owned(),
            ),
            (manifest.clone(), checkpoint.clone(), WORKSPACE.to_owned()),
        ] {
            let refs = ExpectedRefs::new(&m, &c, &d).unwrap();
            assert!(matches!(
                control.lookup(ID, Some(&refs)).unwrap(),
                Lookup::Conflict(_)
            ));
        }
        // No bundle, checkpoint, current inputs, or filesystem is involved.
    }
    #[test]
    fn all_claims_and_head_are_validated_before_lookup() {
        let mut value = fixture();
        let mut claims = get(&value, "claims").unwrap().clone();
        claims.set_field(WORKSPACE, Value::Null).unwrap();
        value.set_field("claims", claims).unwrap();
        assert!(ValidatedControl::validate(value, &actual(), &mut Budget::default()).is_err());
        let mut value = fixture();
        let mut claims = get(&value, "claims").unwrap().clone();
        let mut duplicate = get(&claims, ID).unwrap().clone();
        duplicate
            .set_field("action_id", Value::String(WORKSPACE.into()))
            .unwrap();
        claims.set_field(WORKSPACE, duplicate).unwrap();
        value.set_field("claims", claims).unwrap();
        assert!(matches!(
            ValidatedControl::validate(value, &actual(), &mut Budget::default()),
            Err(Error("invalid_action_claim_identity"))
        ));
        for (field, child) in [
            ("operator_acknowledged", Value::Bool(true)),
            ("status", Value::String("outcome_unknown".into())),
        ] {
            let mut value = fixture();
            set_claim(&mut value, field, child);
            assert!(ValidatedControl::validate(value, &actual(), &mut Budget::default()).is_err());
        }
        let mut value = fixture();
        value.set_field("head", Value::Null).unwrap();
        assert!(ValidatedControl::validate(value, &actual(), &mut Budget::default()).is_err());
    }
    #[test]
    fn terminal_reports_do_not_invent_plan_relationships() {
        let mut value = fixture();
        set_claim(
            &mut value,
            "status",
            Value::String("transport_completed".into()),
        );
        set_claim(&mut value, "operator_acknowledged", Value::Bool(true));
        let report = parse(
            json!({"operation":"runtime_launch","status":"transport_completed",
            "completed_steps":999999,"handles":{"unrelated":"handle"},"diagnostics":[
                {"step":999999,"error":"","message":""}],"outcome_unknown":true}),
        );
        set_claim(&mut value, "report", report);
        ValidatedControl::validate(value, &actual(), &mut Budget::default()).unwrap();
    }
    #[test]
    fn location_is_lexical_and_identity_never_downcasts() {
        for root in ["/", "/a", "/a/...", "/a/ b"] {
            location(&parse(json!({"root":root,"device":0,"inode":1}))).unwrap();
        }
        for root in [
            "", "a", "//", "//a", "/a/", "/a//b", "/./a", "/a/../b", "/a\0b",
        ] {
            assert!(location(&parse(json!({"root":root,"device":0,"inode":1}))).is_err());
        }
        let mut value = fixture();
        let mut loc = get(&value, "location").unwrap().clone();
        loc.set_field("device", Value::integer("18446744073709551617").unwrap())
            .unwrap();
        location(&loc).unwrap();
        value.set_field("location", loc).unwrap();
        assert!(matches!(
            ValidatedControl::validate(value, &actual(), &mut Budget::default()),
            Err(Error("action_workspace_location_changed"))
        ));
        for number in [
            Value::Bool(true),
            Value::float(1.0).unwrap(),
            Value::integer("-1").unwrap(),
        ] {
            assert!(nonnegative(&number).is_err());
        }
    }
    #[test]
    fn accepted_work_boundary_is_inclusive_and_shared() {
        let mut budget = Budget::default();
        budget.charge(MAX_WORK - 4).unwrap();
        budget.canonical(&Value::Null).unwrap();
        assert_eq!(
            budget.charge(1).unwrap_err().code(),
            "unsupported_action_work_profile"
        );
        assert_eq!(
            Budget::default().charge(usize::MAX).unwrap_err().code(),
            "unsupported_action_work_profile"
        );
        let mut budget = Budget::default();
        budget.spaced(&parse(json!({"a":1}))).unwrap();
        assert_eq!(budget.0, 8);
        assert!(!fatal(&Error("unsupported_nonfinite_json")));
        assert!(!fatal(&Error("unsupported_or_invalid_json_scalar")));
        assert!(fatal(&Error("unsupported_json_node_profile")));
    }

    #[test]
    fn stale_bundle_is_valid_and_inputs_are_opaque() {
        let control = fixture();
        let claim = get(get(&control, "claims").unwrap(), ID).unwrap();
        let value = Value::object(vec![
            ("version".into(), Value::integer("1").unwrap()),
            ("action".into(), Value::String("runtime_launch".into())),
            ("action_id".into(), Value::String(ID.into())),
            (
                "workspace_id".into(),
                get(&control, "workspace_id").unwrap().clone(),
            ),
            (
                "epoch_id".into(),
                Value::String("ffffffffffffffffffffffffffffffff".into()),
            ),
            (
                "location".into(),
                get(&control, "location").unwrap().clone(),
            ),
            (
                "control_revision".into(),
                Value::String(format!("runtime-launch-control-v1:{}", "9".repeat(64))),
            ),
            (
                "inputs".into(),
                parse(json!({"arbitrary":{"path":"/never/read","ref":[true,null,1.5]}})),
            ),
            ("plan".into(), get(claim, "plan").unwrap().clone()),
        ])
        .unwrap();
        let digest = bundle(&value, ID, &mut Budget::default()).unwrap();
        assert_eq!(
            digest,
            format!(
                "runtime-launch-manifest-v1:{:x}",
                Sha256::digest(value.canonical().unwrap())
            )
        );
        assert_ne!(
            get(&value, "epoch_id").unwrap().as_str(),
            get(&control, "epoch_id").unwrap().as_str()
        );
        assert!(bundle(&value, WORKSPACE, &mut Budget::default()).is_err());
    }

    #[test]
    fn canonical_authority_bound_is_not_a_report_bound() {
        let payload = Value::String("x".repeat(crate::values::MAX_STRING));
        let oversized = Value::Array(vec![payload.clone(); 16]);
        assert_eq!(
            Budget::default().authority(&oversized).unwrap_err().code(),
            "unsupported_action_input_profile"
        );
        let item = Value::object(vec![
            ("step".into(), Value::Null),
            ("error".into(), Value::String(String::new())),
            ("message".into(), payload),
        ])
        .unwrap();
        let value = Value::object(vec![
            ("operation".into(), Value::String("runtime_launch".into())),
            ("status".into(), Value::String("transport_completed".into())),
            ("completed_steps".into(), Value::integer("0").unwrap()),
            ("handles".into(), Value::object(vec![]).unwrap()),
            ("diagnostics".into(), Value::Array(vec![item; 16])),
            ("outcome_unknown".into(), Value::Bool(false)),
        ])
        .unwrap();
        report(&value, &mut Budget::default()).unwrap();
    }

    #[test]
    fn inspect_retains_claim_and_inspected_outcome_for_every_historical_status() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let actual = crate::platform::workspace_location(&root).unwrap();
        let raw = String::from_utf8(fixture().canonical().unwrap()).unwrap();
        let raw = raw.replace("\"/\"", &serde_json::to_string(&actual.root).unwrap());
        let mut base = values::parse(raw.as_bytes(), Profile::Authority).unwrap();
        let loc = parse(json!({"root":actual.root,"device":actual.device,"inode":actual.inode}));
        base.set_field("location", loc).unwrap();
        std::fs::create_dir(root.join("state")).unwrap();
        for status in [
            "claimed",
            "outcome_unknown",
            "transport_completed",
            "transport_failed_or_partial",
        ] {
            let mut value = base.clone();
            set_claim(&mut value, "status", Value::String(status.into()));
            set_claim(
                &mut value,
                "operator_acknowledged",
                Value::Bool(status != "claimed"),
            );
            if status.starts_with("transport_") {
                set_claim(
                    &mut value,
                    "report",
                    parse(json!({"operation":"runtime_launch","status":status,
                    "completed_steps":0,"handles":{},"diagnostics":[],"outcome_unknown":false})),
                );
            }
            std::fs::write(
                root.join("state/runtime-launch-control.json"),
                value.canonical().unwrap(),
            )
            .unwrap();
            let result = crate::actions::inspect(&root, ID).unwrap();
            assert_eq!(get(&result, "outcome").unwrap().as_str(), Some("inspected"));
            assert_eq!(
                get(get(&result, "claim").unwrap(), "status")
                    .unwrap()
                    .as_str(),
                Some(status)
            );
            assert_eq!(
                get(&result, "bundle_error").unwrap().as_str(),
                Some("action_bundle_missing")
            );
        }
    }

    #[test]
    fn command_work_counts_and_optional_compact_ascii_boundaries() {
        use crate::actions::{inspect_json_with_budget, inspect_with_budget};

        // Independent charge ledger for this one-agent tmux fixture. Do not
        // obtain expected work from Budget or a baseline command's meter.
        fn plan_charges(plan: &Value) -> Vec<usize> {
            let root = get(plan, "root").unwrap().as_str().unwrap().len();
            let namespace = get(plan, "namespace").unwrap().as_str().unwrap().len();
            let agents = get(plan, "agents").unwrap();
            let agent = &agents.as_array().unwrap()[0];
            let command = get(agent, "command").unwrap().as_str().unwrap().len();
            let starter = get(agent, "starter").unwrap().as_str().unwrap().len();
            let id = get(agent, "id").unwrap().as_str().unwrap().len();
            let spaced = plan.sorted_spaced().unwrap().len();
            vec![
                spaced,
                agents.canonical().unwrap().len(),
                2,                                                 // empty env
                2 * root + namespace + 4, // retained root, namespace, "tmux"
                root + namespace,         // session argv payload
                root + command + starter + 4 * id + 4 * namespace, // agent argv payload
                spaced,
            ]
        }

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace-\u{96ea}");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let actual = crate::platform::workspace_location(&root).unwrap();
        let raw = String::from_utf8(fixture().canonical().unwrap()).unwrap();
        let raw = raw.replace("\"/\"", &serde_json::to_string(&actual.root).unwrap());
        let mut control = values::parse(raw.as_bytes(), Profile::Authority).unwrap();
        control
            .set_field(
                "location",
                parse(json!({"root":actual.root,"device":actual.device,"inode":actual.inode})),
            )
            .unwrap();
        let mut claims = get(&control, "claims").unwrap().clone();
        let selected = get(&claims, ID).unwrap().clone();
        let other_id = "11111111111111111111111111111111";
        let raw = String::from_utf8(selected.canonical().unwrap())
            .unwrap()
            .replace(ID, other_id)
            .replace(
                "dddddddddddddddddddddddddddddddd",
                "ffffffffffffffffffffffffffffffff",
            );
        let mut other = values::parse(raw.as_bytes(), Profile::Authority).unwrap();
        other
            .set_field("status", Value::String("transport_completed".into()))
            .unwrap();
        let report = parse(
            json!({"operation":"runtime_launch","status":"transport_completed",
            "completed_steps":0,"handles":{},"diagnostics":[{"step":null,"error":"","message":"\u{96ea}"}],"outcome_unknown":false}),
        );
        other.set_field("report", report.clone()).unwrap();
        claims.set_field(other_id, other.clone()).unwrap();
        control.set_field("claims", claims).unwrap();
        let mut bundle = parse(json!({"version":1,"action":"runtime_launch","action_id":ID,
            "workspace_id":WORKSPACE,"epoch_id":"cccccccccccccccccccccccccccccccc",
            "control_revision":format!("runtime-launch-control-v1:{}", "9".repeat(64)),"inputs":{}}));
        bundle
            .set_field("location", get(&control, "location").unwrap().clone())
            .unwrap();
        bundle
            .set_field("plan", get(&selected, "plan").unwrap().clone())
            .unwrap();
        std::fs::create_dir_all(root.join("state/actions")).unwrap();
        std::fs::write(
            root.join("state/runtime-launch-control.json"),
            control.canonical().unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join(format!("state/actions/{ID}.json")),
            bundle.canonical().unwrap(),
        )
        .unwrap();

        let control_size = control.canonical().unwrap().len();
        let mut ledger = vec![control_size]; // full control validation
        ledger.extend(plan_charges(get(&selected, "plan").unwrap()));
        ledger.extend(plan_charges(get(&other, "plan").unwrap()));
        ledger.push(report.canonical().unwrap().len());
        ledger.push(control_size); // control revision
        let before_bundle: usize = ledger.iter().sum();
        let bundle_size = bundle.canonical().unwrap().len();
        ledger.push(bundle_size);
        ledger.extend(plan_charges(get(&bundle, "plan").unwrap()));
        let before_compact: usize = ledger.iter().sum();

        let mut meter = Budget::default();
        let (result, compact) = inspect_with_budget(&root, ID, &mut meter).unwrap();
        assert_eq!(get(&result, "outcome").unwrap().as_str(), Some("inspected"));
        assert_eq!(
            get(get(&result, "claim").unwrap(), "claim_id")
                .unwrap()
                .as_str(),
            get(&selected, "claim_id").unwrap().as_str()
        );
        assert_eq!(
            get(get(&result, "control").unwrap(), "claims")
                .unwrap()
                .as_object()
                .unwrap()
                .entries()
                .len(),
            2
        );
        let compact_size = compact.len();
        let library_work = before_compact + compact_size;
        assert_eq!(meter.0, library_work);
        // Calculate ASCII expansion without the command's output helper;
        // BMP snow characters expand from three to six bytes.
        let text = std::str::from_utf8(&compact).unwrap();
        let escaped_size: usize = text
            .chars()
            .map(|c| if c < '\u{7f}' { 1 } else { 6 * c.len_utf16() })
            .sum();
        assert!(escaped_size > compact_size);
        let cli_work = library_work + escaped_size;
        assert!(cli_work < 100_000); // precharging avoids large allocations

        // Exhaust within the optional selected bundle, not control validation.
        for cli in [false, true] {
            let precharge = MAX_WORK - (before_bundle + bundle_size - 1);
            let mut meter = Budget(precharge);
            let error = if cli {
                inspect_json_with_budget(&root, ID, &mut meter).unwrap_err()
            } else {
                inspect_with_budget(&root, ID, &mut meter).unwrap_err()
            };
            assert_eq!(error.code(), "unsupported_action_work_profile");
            assert_eq!(meter.0, precharge + before_bundle);
        }
        // Both entry points must charge the final compact report on that meter.
        for cli in [false, true] {
            let precharge = MAX_WORK - (library_work - 1);
            let mut meter = Budget(precharge);
            let error = if cli {
                inspect_json_with_budget(&root, ID, &mut meter).unwrap_err()
            } else {
                inspect_with_budget(&root, ID, &mut meter).unwrap_err()
            };
            assert_eq!(error.code(), "unsupported_action_work_profile");
            assert_eq!(meter.0, precharge + before_compact);
        }
        // Library accepts exactly its compact budget, without an ASCII charge.
        let mut meter = Budget(MAX_WORK - library_work);
        inspect_with_budget(&root, ID, &mut meter).unwrap();
        assert_eq!(meter.0, MAX_WORK);
        // CLI needs the *sum*, not max(compact, escaped), and accepts equality.
        let mut meter = Budget(MAX_WORK - cli_work);
        assert_eq!(
            inspect_json_with_budget(&root, ID, &mut meter)
                .unwrap()
                .len(),
            escaped_size
        );
        assert_eq!(meter.0, MAX_WORK);
        let precharge = MAX_WORK - cli_work + 1;
        let mut meter = Budget(precharge);
        assert_eq!(
            inspect_json_with_budget(&root, ID, &mut meter)
                .unwrap_err()
                .code(),
            "unsupported_action_work_profile"
        );
        assert_eq!(meter.0, precharge + library_work);
    }
}
