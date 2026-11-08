use super::*;
use crate::{
    storage::{Delivery, WorkspaceGuard},
    values::{self, Profile},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

fn parse(s: &str) -> Value {
    values::parse(s.as_bytes(), Profile::Authority).unwrap()
}
fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        b"not JSON; independent of config",
    )
    .unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    for name in ["tasks.json", "agents.json", "execution.json"] {
        fs::write(root.path().join("state").join(name), b"corrupt").unwrap();
    }
    root
}
fn binding() -> Value {
    parse(
        r#"{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not/a/socket","device":184467440737095516160000,"inode":0},"terminal_id":"term","pane_id_at_binding":"w1:p1"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2024-W01T12Z","agent_kind_hint":null,"native_reference":null,"native_binding":"unestablished"}}"#,
    )
}
fn attachment() -> Value {
    parse(
        r#"{"adapter":"djinn_live","source_contract":"djinn.live-observation.v1","artifact_path":"/does/not/exist/00000000-0000-4000-8000-000000000001.json","runtime_id":"00000000-0000-4000-8000-000000000001","instance_id":"00000000-0000-4000-8000-000000000002","session_id":"session!","attached_at":"20240101T120000,123456789-000000,0000009"}"#,
    )
}
fn raw(root: &Path) -> Option<Vec<u8>> {
    fs::read(root.join("state/groups.json")).ok()
}
fn add(root: &Path, b: &Value, role: &str, rebind: bool, expected: Option<&str>) -> Result<Value> {
    add_member(
        root,
        AddMember {
            group: "g",
            member: "m",
            role,
            binding: b,
            rebind,
            expected_revision: expected,
        },
    )
}
struct Fixed(AtomicU64);
impl Fixed {
    fn new() -> Self {
        Self(AtomicU64::new(1))
    }
}
fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
impl store::Io for Fixed {
    fn uuid(&self) -> Result<String> {
        Ok(id(self.0.fetch_add(1, Ordering::SeqCst)))
    }
}
fn oracle(request: serde_json::Value) -> Value {
    let script = PathBuf::from(
        option_env!("COVEN_GROUPS_ORACLE_ROOT").unwrap_or(env!("CARGO_MANIFEST_DIR")),
    )
    .join("src/groups/oracle.py");
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    values::parse(&output.stdout, Profile::Authority).unwrap()
}
fn json(v: &Value) -> serde_json::Value {
    // Tests with huge endpoint integers pass bindings as raw JSON to Python.
    serde_json::from_slice(&v.compact().unwrap()).unwrap()
}

#[test]
fn pure_selector_display_source_and_live_pin_validators_match_python() {
    for (validator, values, validate) in [
        (
            "selector",
            vec![
                "g".to_owned(),
                "0".into(),
                "g_-9".into(),
                "".into(),
                "A".into(),
                "-g".into(),
                "g.x".into(),
                "a".repeat(64),
                "a".repeat(65),
                "é".into(),
            ],
            selector as fn(&str) -> Result<()>,
        ),
        (
            "display",
            vec![
                "Role".into(),
                " ".into(),
                "".into(),
                "é".repeat(32),
                "é".repeat(33),
                " x ".into(),
                "x\t".into(),
                "\u{a0}".into(),
                "\u{2028}".into(),
                "\u{200b}".into(),
                "\u{e000}".into(),
                "\u{378}".into(),
                "\u{1fae9}".into(),
            ],
            display as fn(&str) -> Result<()>,
        ),
        (
            "source",
            vec![
                "Coven.A_-9".into(),
                "a".repeat(64),
                "a".repeat(65),
                "".into(),
                "a b".into(),
                "é".into(),
                "-".into(),
            ],
            source as fn(&str) -> Result<()>,
        ),
    ] {
        let expected =
            oracle(serde_json::json!({"mode":"pure","validator":validator,"values":values}));
        for (s, e) in values.iter().zip(array(&expected).unwrap()) {
            assert_eq!(
                validate(s).is_ok(),
                matches!(e, Value::Bool(true)),
                "{validator}: {s:?}"
            );
        }
    }
    let a = attachment();
    let mut variants = vec![a.clone()];
    for (key, value) in [
        ("runtime_id", string("00000000-0000-1000-8000-000000000001")),
        (
            "instance_id",
            string("00000000-0000-4000-0000-000000000002"),
        ),
        ("session_id", string("with space")),
        (
            "artifact_path",
            string("/bad/../00000000-0000-4000-8000-000000000001.json"),
        ),
        ("artifact_path", string("/not-the-runtime.json")),
        ("attached_at", string("2024-01-01T00+01")),
        ("extra", Value::Null),
    ] {
        let mut altered = a.clone();
        altered.set_field(key, value).unwrap();
        variants.push(altered);
    }
    let expected = oracle(
        serde_json::json!({"mode":"pure","validator":"attachment","values":variants.iter().map(json).collect::<Vec<_>>()}),
    );
    for (a, e) in variants.iter().zip(array(&expected).unwrap()) {
        assert_eq!(
            validate_attachment(a).is_ok(),
            matches!(e, Value::Bool(true))
        );
    }
}

#[cfg(unix)]
#[test]
fn unsafe_workspace_entries_are_rejected_without_creating_coordination_files() {
    use std::os::unix::fs::symlink;
    for entry in ["coven.json", "state", "state/groups.json"] {
        let root = workspace();
        let outside = workspace();
        let target = root.path().join(entry);
        if target.is_dir() {
            fs::remove_dir_all(&target).unwrap();
        } else if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        let source = if entry == "state" {
            outside.path().join("state")
        } else {
            outside.path().join("coven.json")
        };
        symlink(source, &target).unwrap();
        assert!(read_groups(root.path()).is_err());
        assert!(create_group(root.path(), "g", "Group", None).is_err());
        assert!(!root.path().join("state/.workspace.lock").exists());
        assert!(!outside.path().join("state/.workspace.lock").exists());
    }
    let root = workspace();
    fs::create_dir(root.path().join("state/groups.json")).unwrap();
    assert!(read_groups(root.path()).is_err());
    assert!(!root.path().join("state/.workspace.lock").exists());
}

fn with_terminal(b: &Value, terminal: &str) -> Value {
    let mut b = b.clone();
    let mut p = get(&b, "presentation").unwrap().clone();
    p.set_field("terminal_id", string(terminal)).unwrap();
    b.set_field("presentation", p).unwrap();
    b
}

#[test]
fn closed_schema_variants_process_order_and_identity_uniqueness_match_python() {
    let root = workspace();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &binding(), "Role", false, None).unwrap();
    let state = read_groups(root.path()).unwrap().state.unwrap();
    let original = array(get(&state, "groups").unwrap()).unwrap()[0].clone();
    let member = array(get(&original, "members").unwrap()).unwrap()[0].clone();
    let replace = |member: Value, version: u64| {
        let mut state = state.clone();
        let mut g = original.clone();
        g.set_field("members", Value::Array(vec![member])).unwrap();
        state.set_field("groups", Value::Array(vec![g])).unwrap();
        state
            .set_field("schema_version", num(version).unwrap())
            .unwrap();
        state
    };
    let mut cases = vec![state.clone()];
    let mut m3 = member.clone();
    m3.set_field("live_attachment", Value::Null).unwrap();
    cases.push(replace(m3.clone(), 3));
    cases.push(replace(m3.clone(), 2));
    cases.push(replace(member.clone(), 3));
    m3.set_field("live_attachment", attachment()).unwrap();
    cases.push(replace(m3, 3));
    for raw in ["true", "1.0", "0", "4294967296", "2"] {
        let mut b = binding();
        let e = parse(&format!(
            r#"{{"provenance":"kitsune.pane.process_info","observed_at":"2024-01-01T00Z","agent_kind_hint":"djinn","native_reference":null,"native_binding":"unestablished","process_evidence":{{"interpreter":"coven.kitsune.djinn-process.v1","matches":[{{"pid":{raw},"executable_family":"djinn","rule":"exact_posix_argv0_basename"}},{{"pid":1,"executable_family":"djinn-ui","rule":"exact_posix_argv0_basename"}}]}}}}"#
        ));
        b.set_field("evidence", e).unwrap();
        let mut m = member.clone();
        m.set_field("binding", b).unwrap();
        cases.push(replace(m, 2));
    }
    let mut duplicate = original.clone();
    let mut other = member.clone();
    other.set_field("key", string("other")).unwrap();
    other.set_field("participant_id", string(id(999))).unwrap();
    duplicate
        .set_field("members", Value::Array(vec![member.clone(), other]))
        .unwrap();
    let mut s = state.clone();
    s.set_field("groups", Value::Array(vec![duplicate]))
        .unwrap();
    cases.push(s);
    let mut same_id = member.clone();
    same_id
        .set_field(
            "participant_id",
            get(&original, "group_id").unwrap().clone(),
        )
        .unwrap();
    cases.push(replace(same_id, 1));
    let raws: Vec<_> = cases
        .iter()
        .map(|s| String::from_utf8(s.storage().unwrap()).unwrap())
        .collect();
    let expected = oracle(serde_json::json!({"mode":"validate","values":raws}));
    for (raw, e) in raws.iter().zip(array(&expected).unwrap()) {
        assert_eq!(
            store::decode(raw.as_bytes()).is_ok(),
            matches!(get(e, "accepted").unwrap(), Value::Bool(true)),
            "{raw}"
        );
    }
}

#[test]
fn attachment_promotion_normalizes_all_members_and_noop_keeps_timestamp_and_binding() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    create_group(root.path(), "other", "Other", None).unwrap();
    add_member(
        root.path(),
        AddMember {
            group: "other",
            member: "second",
            role: "Other",
            binding: &with_terminal(&b, "second"),
            rebind: false,
            expected_revision: None,
        },
    )
    .unwrap();
    let view = read_groups(root.path()).unwrap();
    let token = view.revision().to_owned();
    let participant = text(get(view.member("g", "m").unwrap(), "participant_id").unwrap())
        .unwrap()
        .to_owned();
    detach_live(root.path(), "g", "m", &token, &participant).unwrap();
    assert_eq!(
        uint(
            get(
                read_groups(root.path()).unwrap().state().unwrap(),
                "schema_version"
            )
            .unwrap()
        )
        .unwrap(),
        1
    );
    let a = attachment();
    attach_live(
        root.path(),
        AttachMember {
            group: "g",
            member: "m",
            attachment: &a,
            expected_revision: &token,
            expected_participant_id: &participant,
            reattach: false,
        },
    )
    .unwrap();
    let view = read_groups(root.path()).unwrap();
    assert_eq!(
        uint(get(view.state().unwrap(), "schema_version").unwrap()).unwrap(),
        3
    );
    assert!(matches!(
        get(view.member("other", "second").unwrap(), "live_attachment").unwrap(),
        Value::Null
    ));
    let before = raw(root.path()).unwrap();
    let mut fresh = a.clone();
    fresh
        .set_field("attached_at", string("2025-01-01T00Z"))
        .unwrap();
    let same = attach_live(
        root.path(),
        AttachMember {
            group: "g",
            member: "m",
            attachment: &fresh,
            expected_revision: view.revision(),
            expected_participant_id: &participant,
            reattach: false,
        },
    )
    .unwrap();
    assert_eq!(text(get(&same, "outcome").unwrap()).unwrap(), "unchanged");
    assert_eq!(raw(root.path()).unwrap(), before);
    let mut changed = fresh.clone();
    changed.set_field("session_id", string("another")).unwrap();
    assert!(
        attach_live(
            root.path(),
            AttachMember {
                group: "g",
                member: "m",
                attachment: &changed,
                expected_revision: view.revision(),
                expected_participant_id: &participant,
                reattach: false
            }
        )
        .is_err()
    );
    assert_eq!(raw(root.path()).unwrap(), before);
    add(root.path(), &b, "Renamed role", false, None).unwrap();
    let view = read_groups(root.path()).unwrap();
    assert!(
        equal(
            get(view.member("g", "m").unwrap(), "live_attachment").unwrap(),
            &a
        )
        .unwrap()
    );
    let rebound = add(
        root.path(),
        &with_terminal(&b, "replacement"),
        "Renamed role",
        true,
        None,
    )
    .unwrap();
    assert!(matches!(
        get(&rebound, "live_attachment_cleared").unwrap(),
        Value::Bool(true)
    ));
    let view = read_groups(root.path()).unwrap();
    assert!(matches!(
        get(view.member("g", "m").unwrap(), "live_attachment").unwrap(),
        Value::Null
    ));
    assert_eq!(
        text(get(view.member("g", "m").unwrap(), "participant_id").unwrap()).unwrap(),
        participant
    );
}
#[test]
fn new_reservation_copies_request_endpoint_order_and_exact_integers_only() {
    for raw_endpoint in [
        r#"{"path":"/not/a/socket","device":184467440737095516160000,"inode":9007199254740993}"#,
        r#"{"inode":9007199254740993,"path":"/not/a/socket","device":184467440737095516160000}"#,
        r#"{"device":184467440737095516160000,"inode":9007199254740993,"path":"/not/a/socket"}"#,
    ] {
        let root = workspace();
        let endpoint = parse(raw_endpoint);
        let projected = values::parse(&endpoint.canonical().unwrap(), Profile::Authority).unwrap();
        let mut b = binding();
        let mut presentation = get(&b, "presentation").unwrap().clone();
        presentation
            .set_field("endpoint", endpoint.clone())
            .unwrap();
        b.set_field("presentation", presentation).unwrap();
        create_group(root.path(), "g", "Group", None).unwrap();
        add(root.path(), &b, "Role", false, None).unwrap();
        let view = read_groups(root.path()).unwrap();
        let mut expected = view.state().unwrap().clone();
        // An unrelated retained counter must preserve its own order and max u64.
        let retained = obj(&[
            ("endpoint", projected.clone()),
            ("source", string("retained")),
            ("epoch", string("old")),
            (
                "watermarks",
                obj(&[("removed", num(u64::MAX).unwrap())]).unwrap(),
            ),
        ])
        .unwrap();
        expected
            .set_field("publication_counters", Value::Array(vec![retained.clone()]))
            .unwrap();
        fs::write(
            root.path().join("state/groups.json"),
            expected.storage().unwrap(),
        )
        .unwrap();
        let result = reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: view.revision(),
                endpoint: &projected,
                source: "new",
                epoch: "one",
                terminal_ids: &["term".into()],
                allow_new_epoch: false,
            },
        )
        .unwrap();
        let counter = obj(&[
            ("endpoint", projected),
            ("source", string("new")),
            ("epoch", string("one")),
            ("watermarks", obj(&[("term", num(1).unwrap())]).unwrap()),
        ])
        .unwrap();
        expected
            .set_field(
                "publication_counters",
                Value::Array(vec![retained, counter]),
            )
            .unwrap();
        assert_eq!(raw(root.path()).unwrap(), expected.storage().unwrap());
        assert_eq!(
            text(get(&result, "revision").unwrap()).unwrap(),
            view.revision()
        );
        assert_eq!(
            uint(get(get(&result, "reservations").unwrap(), "term").unwrap()).unwrap(),
            1
        );
    }
}

#[test]
fn whole_cycle_precomputes_all_sixteen_counters_and_retains_removed_watermarks() {
    let root = workspace();
    create_group(root.path(), "g", "Group", None).unwrap();
    let b = binding();
    let mut targets = Vec::new();
    for n in 0..16 {
        let key = format!("m{n}");
        let terminal = format!("term{n}");
        let binding = with_terminal(&b, &terminal);
        add_member(
            root.path(),
            AddMember {
                group: "g",
                member: &key,
                role: "Role",
                binding: &binding,
                rebind: false,
                expected_revision: None,
            },
        )
        .unwrap();
        targets.push(terminal);
    }
    let ep = get(get(&b, "presentation").unwrap(), "endpoint").unwrap();
    let token = read_groups(root.path()).unwrap().revision().to_owned();
    let request = ReserveRequest {
        group: "g",
        expected_revision: &token,
        endpoint: ep,
        source: "coven",
        epoch: "one",
        terminal_ids: &targets,
        allow_new_epoch: false,
    };
    let first = reserve_cycle(root.path(), request).unwrap();
    assert_eq!(
        get(&first, "reservations")
            .unwrap()
            .as_object()
            .unwrap()
            .entries()
            .len(),
        16
    );
    let mut state = read_groups(root.path()).unwrap().state.unwrap();
    let mut counters = array(get(&state, "publication_counters").unwrap())
        .unwrap()
        .to_vec();
    let mut wm = get(&counters[0], "watermarks").unwrap().clone();
    wm.set_field(&targets[15], num(u64::MAX).unwrap()).unwrap();
    counters[0].set_field("watermarks", wm).unwrap();
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
    fs::write(
        root.path().join("state/groups.json"),
        state.storage().unwrap(),
    )
    .unwrap();
    let before = raw(root.path()).unwrap();
    assert_eq!(
        reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: &token,
                endpoint: ep,
                source: "coven",
                epoch: "one",
                terminal_ids: &targets,
                allow_new_epoch: false
            }
        )
        .unwrap_err()
        .code(),
        "groups_publication_revision_overflow"
    );
    assert_eq!(raw(root.path()).unwrap(), before);
    remove_member(root.path(), "g", "m15", None).unwrap();
    targets.pop();
    let view = read_groups(root.path()).unwrap();
    let next = reserve_cycle(
        root.path(),
        ReserveRequest {
            group: "g",
            expected_revision: view.revision(),
            endpoint: ep,
            source: "coven",
            epoch: "one",
            terminal_ids: &targets,
            allow_new_epoch: false,
        },
    )
    .unwrap();
    assert_eq!(
        uint(get(get(&next, "reservations").unwrap(), "term0").unwrap()).unwrap(),
        2
    );
    let state = read_groups(root.path()).unwrap().state.unwrap();
    let counter = &array(get(&state, "publication_counters").unwrap()).unwrap()[0];
    assert_eq!(
        uint(get(get(counter, "watermarks").unwrap(), "term15").unwrap()).unwrap(),
        u64::MAX
    );
}

#[test]
fn counter_capacity_never_evicts_and_epoch_reset_is_explicit() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    let token = read_groups(root.path()).unwrap().revision().to_owned();
    let ep = get(get(&b, "presentation").unwrap(), "endpoint").unwrap();
    let targets = vec!["term".to_owned()];
    for n in 0..8 {
        reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: &token,
                endpoint: ep,
                source: &format!("s{n}"),
                epoch: "one",
                terminal_ids: &targets,
                allow_new_epoch: false,
            },
        )
        .unwrap();
    }
    let before = raw(root.path()).unwrap();
    assert!(
        reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: &token,
                endpoint: ep,
                source: "ninth",
                epoch: "one",
                terminal_ids: &targets,
                allow_new_epoch: false
            }
        )
        .is_err()
    );
    assert_eq!(raw(root.path()).unwrap(), before);
    let mut state = read_groups(root.path()).unwrap().state.unwrap();
    let mut counters = array(get(&state, "publication_counters").unwrap())
        .unwrap()
        .to_vec();
    let mut wm = obj(&[]).unwrap();
    for n in 0..64 {
        wm.set_field(&format!("removed{n}"), num(1).unwrap())
            .unwrap();
    }
    counters[0].set_field("watermarks", wm).unwrap();
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
    fs::write(
        root.path().join("state/groups.json"),
        state.storage().unwrap(),
    )
    .unwrap();
    let before = raw(root.path()).unwrap();
    assert!(
        reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: &token,
                endpoint: ep,
                source: "s0",
                epoch: "one",
                terminal_ids: &targets,
                allow_new_epoch: true
            }
        )
        .is_err()
    );
    assert_eq!(raw(root.path()).unwrap(), before);
    assert!(
        reserve_cycle(
            root.path(),
            ReserveRequest {
                group: "g",
                expected_revision: &token,
                endpoint: ep,
                source: "s0",
                epoch: "two",
                terminal_ids: &targets,
                allow_new_epoch: false
            }
        )
        .is_err()
    );
    let reset = reserve_cycle(
        root.path(),
        ReserveRequest {
            group: "g",
            expected_revision: &token,
            endpoint: ep,
            source: "s0",
            epoch: "two",
            terminal_ids: &targets,
            allow_new_epoch: true,
        },
    )
    .unwrap();
    assert_eq!(
        uint(get(get(&reset, "reservations").unwrap(), "term").unwrap()).unwrap(),
        1
    );
    let state = read_groups(root.path()).unwrap().state.unwrap();
    assert_eq!(
        array(get(&state, "publication_counters").unwrap())
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn native_writer_admits_exact_serialized_profile_without_a_smaller_headroom_cap() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    let mut state = read_groups(root.path()).unwrap().state.unwrap();
    // Fill valid retained watermarks with bounded strings, independently of active
    // membership. This is valid authority, not a fixture-sized writer restriction.
    let ep = get(get(&b, "presentation").unwrap(), "endpoint").unwrap();
    let mut counters = Vec::new();
    for n in 0..8 {
        let mut wm = obj(&[]).unwrap();
        for t in 0..64 {
            wm.set_field(&format!("{t}-{}", "x".repeat(1900)), num(1).unwrap())
                .unwrap();
        }
        counters.push(
            obj(&[
                ("endpoint", ep.clone()),
                ("source", string(format!("s{n}"))),
                ("epoch", string("one")),
                ("watermarks", wm),
            ])
            .unwrap(),
        );
    }
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
    validate(&state).unwrap();
    let bytes = state.storage().unwrap();
    assert!(bytes.len() > 128 * 1024 && bytes.len() < store::LIMIT);
    let guard = WorkspaceGuard::acquire(root.path(), std::time::Duration::ZERO).unwrap();
    store::finish(&guard, &state, None, true, &store::SystemIo).unwrap();
    assert_eq!(raw(root.path()).unwrap(), bytes);
    let before = raw(root.path()).unwrap();
    let mut counters = array(get(&state, "publication_counters").unwrap())
        .unwrap()
        .to_vec();
    // Enough legal strings to cross 1 MiB in the actual serialization.
    let mut oversized = obj(&[]).unwrap();
    for t in 0..64 {
        oversized
            .set_field(&format!("{t}-{}", "y".repeat(4090)), num(1).unwrap())
            .unwrap();
    }
    counters[0].set_field("watermarks", oversized).unwrap();
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
    validate(&state).unwrap();
    assert!(state.storage().unwrap().len() > store::LIMIT);
    assert_eq!(
        store::finish(&guard, &state, None, true, &store::SystemIo)
            .unwrap_err()
            .code(),
        "groups_authority_size_limit"
    );
    assert_eq!(raw(root.path()).unwrap(), before);
}

#[test]
fn structural_preflight_is_pure_and_read_does_not_lock_or_initialize() {
    let root = workspace();
    let view = read_groups(root.path()).unwrap();
    assert_eq!(view.revision(), UNINITIALIZED);
    assert!(view.state().is_none());
    assert!(!root.path().join("state/.workspace.lock").exists());
    fs::remove_dir_all(root.path().join("state")).unwrap();
    assert!(create_group(root.path(), "g", "Group", None).is_err());
    assert!(!root.path().join("state").exists());
    assert_eq!(
        create_group(root.path(), "INVALID", "Group", None)
            .unwrap_err()
            .code(),
        "groups_invalid_selector"
    );
}

#[test]
fn noops_keep_original_bytes_timestamp_mode_and_inode_and_still_check_user_cas() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    let result = add(root.path(), &b, "Role", false, None).unwrap();
    let revision = text(get(&result, "revision").unwrap()).unwrap();
    let mut fresh = b.clone();
    let mut e = get(&fresh, "evidence").unwrap().clone();
    e.set_field("observed_at", string("2025-01-01T00Z"))
        .unwrap();
    fresh.set_field("evidence", e).unwrap();
    let mut p = get(&fresh, "presentation").unwrap().clone();
    p.set_field("pane_id_at_binding", string("w2:p2")).unwrap();
    fresh.set_field("presentation", p).unwrap();
    let path = root.path().join("state/groups.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let original = fs::read(&path).unwrap();
    let before = fs::metadata(&path).unwrap();
    let retry = add(root.path(), &fresh, "Role", true, Some(revision)).unwrap();
    assert_eq!(text(get(&retry, "outcome").unwrap()).unwrap(), "unchanged");
    assert_eq!(fs::read(&path).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let after = fs::metadata(&path).unwrap();
        assert_eq!((before.ino(), before.mode()), (after.ino(), after.mode()));
    }
    assert_eq!(
        add(root.path(), &fresh, "Role", false, Some(UNINITIALIZED))
            .unwrap_err()
            .code(),
        "groups_revision_conflict"
    );
    assert!(equal(get(get(&retry, "member").unwrap(), "binding").unwrap(), &b).unwrap());
}

#[test]
fn python_timestamp_oracle_quirks_and_invalid_calendar_values() {
    let mut cases: Vec<String> = vec![
        "2024-01-01T12Z",
        "20240101T1200+00",
        "2024W011T120000Z",
        "2024-W01T12-00:00",
        "2020-W53-7T23:59:59.123456789+000000,0000009",
        "2021-W53-1T00Z",
        "0000-01-01T00Z",
        "9999-W52-7T00Z",
        "2024-02-30T00Z",
        "2024-01-01T23:59:60Z",
        "2024-01-01T24Z",
        "2024-01-01t12Z",
        "2024-01-01T12z",
        "2024-01-01T12.1Z",
        "2024-01-01T12:00,1Z",
        "2024-01-01T120000+00.1",
        "2024-01-01T120000+00:00:00.1",
        "2024-01-01T120000-00:00:00.0000001",
        "2024-01-01T12:00:00+00:60",
        "2024-01-01T00:00:00+24",
        "2024-01-01T00:00:00.Z",
        "2024-01-01T00:00:00Zextra",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for y in [1, 4, 100, 400, 1900, 2000, 2024, 9999] {
        for week in [0, 1, 52, 53, 54] {
            for day in [1, 7] {
                cases.push(format!("{y:04}-W{week:02}-{day}T00Z"));
            }
        }
    }
    let expected = oracle(serde_json::json!({"mode":"timestamp","values":cases}));
    for (case, expected) in cases.iter().zip(array(&expected).unwrap()) {
        assert_eq!(
            super::timestamp::validate(case).is_ok(),
            matches!(expected, Value::Bool(true)),
            "{case}"
        );
    }
}

#[test]
fn authority_schema_and_canonical_hash_match_python_including_arbitrary_integers() {
    let root = workspace();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &binding(), "Role", false, None).unwrap();
    let state = read_groups(root.path()).unwrap().state.unwrap();
    let base = state.storage().unwrap();
    let mut cases = vec![String::from_utf8(base.clone()).unwrap()];
    let mut historic = state.clone();
    historic
        .set_field(
            "workspace_id",
            string("ffffffff-ffff-ffff-ffff-ffffffffffff"),
        )
        .unwrap();
    cases.push(String::from_utf8(historic.storage().unwrap()).unwrap());
    for value in ["true", "-1", "1.0", "18446744073709551616"] {
        let mut altered = state.clone();
        altered.set_field("content_revision", parse(value)).unwrap();
        cases.push(String::from_utf8(altered.storage().unwrap()).unwrap());
    }
    let mut unknown = state.clone();
    unknown.set_field("unknown", Value::Null).unwrap();
    cases.push(String::from_utf8(unknown.storage().unwrap()).unwrap());
    cases.push(String::from_utf8(base.clone()).unwrap().replacen(
        "\"groups\":",
        "\"groups\": [], \"groups\":",
        1,
    ));
    cases.push(format!(
        "{}{}",
        " ".repeat(store::LIMIT),
        String::from_utf8(base).unwrap()
    ));
    let bounded_cases = cases[..cases.len() - 1].to_vec();
    let expected = oracle(serde_json::json!({"mode":"validate","values":bounded_cases}));
    for (raw, expected) in bounded_cases.iter().zip(array(&expected).unwrap()) {
        let actual = store::decode(raw.as_bytes());
        assert_eq!(
            actual.is_ok(),
            matches!(get(expected, "accepted").unwrap(), Value::Bool(true))
        );
        if let Ok(actual) = actual {
            assert_eq!(
                revision(Some(&actual)).unwrap(),
                text(get(expected, "revision").unwrap()).unwrap()
            );
        }
    }
    assert!(store::decode(cases.last().unwrap().as_bytes()).is_err());
    let nested = format!("{}0{}", "[".repeat(17), "]".repeat(17));
    assert!(store::decode(nested.as_bytes()).is_err());
    let mut counters = state.clone();
    counters
        .set_field("publication_counters", Value::Array(vec![]))
        .unwrap();
    counters
        .set_field("schema_version", num(2).unwrap())
        .unwrap();
    assert_eq!(
        revision(Some(&state)).unwrap(),
        revision(Some(&counters)).unwrap()
    );
}

#[test]
fn exact_python_mutation_results_bytes_promotions_and_retry_transitions() {
    let root = workspace();
    let io = Fixed::new();
    // Oracle requests parse the arbitrary endpoint integer themselves, avoiding
    // serde_json's intentionally narrower numeric representation in test setup.
    let mut b = binding();
    let mut ep = get(get(&b, "presentation").unwrap(), "endpoint")
        .unwrap()
        .clone();
    ep.set_field("device", num(7).unwrap()).unwrap();
    let ep = values::parse(&ep.canonical().unwrap(), Profile::Authority).unwrap();
    let mut p = get(&b, "presentation").unwrap().clone();
    p.set_field("endpoint", ep.clone()).unwrap();
    b.set_field("presentation", p).unwrap();
    let b = values::parse(&b.canonical().unwrap(), Profile::Authority).unwrap();
    let mut process = b.clone();
    let e = parse(
        r#"{"provenance":"kitsune.pane.process_info","observed_at":"2024-01-01T00Z","agent_kind_hint":"djinn","native_reference":null,"native_binding":"unestablished","process_evidence":{"interpreter":"coven.kitsune.djinn-process.v1","matches":[{"pid":1,"executable_family":"djinn","rule":"exact_posix_argv0_basename"}]}}"#,
    );
    process.set_field("evidence", e).unwrap();
    let process = values::parse(&process.canonical().unwrap(), Profile::Authority).unwrap();
    let a = values::parse(&attachment().canonical().unwrap(), Profile::Authority).unwrap();
    let terminals = vec!["term".to_owned()];
    for step in 0..18 {
        let before = raw(root.path());
        let view = read_groups(root.path()).unwrap();
        let token = view.revision().to_owned();
        let participant = view
            .member("g", "m")
            .ok()
            .map(|m| text(get(m, "participant_id").unwrap()).unwrap().to_owned())
            .unwrap_or_else(|| id(3));
        let start = io.0.load(Ordering::SeqCst);
        let (operation, args, result) = match step {
            0 | 1 => (
                "create_group",
                serde_json::json!({"key":"g","name":"Group"}),
                mutate(root.path(), "g", None, Operation::Create("Group"), &io),
            ),
            2 | 3 | 13 => (
                "add_member",
                serde_json::json!({"group_key":"g","member_key":"m","role":"Role","binding":json(&b)}),
                mutate(
                    root.path(),
                    "g",
                    None,
                    Operation::Add(AddMember {
                        group: "g",
                        member: "m",
                        role: "Role",
                        binding: &b,
                        rebind: false,
                        expected_revision: None,
                    }),
                    &io,
                ),
            ),
            4 | 5 | 14 => (
                "reserve_cycle",
                serde_json::json!({"group_key":"g","expected_revision":token,"endpoint":json(&ep),"source":"coven","epoch":"one","terminal_ids":terminals,"allow_new_epoch":false}),
                reserve_with(
                    root.path(),
                    ReserveRequest {
                        group: "g",
                        expected_revision: &token,
                        endpoint: &ep,
                        source: "coven",
                        epoch: "one",
                        terminal_ids: &terminals,
                        allow_new_epoch: false,
                    },
                    &io,
                ),
            ),
            6 | 7 => (
                "add_member",
                serde_json::json!({"group_key":"g","member_key":"m","role":"Role","binding":json(&process),"rebind":true}),
                mutate(
                    root.path(),
                    "g",
                    None,
                    Operation::Add(AddMember {
                        group: "g",
                        member: "m",
                        role: "Role",
                        binding: &process,
                        rebind: true,
                        expected_revision: None,
                    }),
                    &io,
                ),
            ),
            8 | 9 => (
                "attach_live",
                serde_json::json!({"group_key":"g","member_key":"m","attachment":json(&a),"expected_revision":token,"expected_participant_id":participant}),
                mutate(
                    root.path(),
                    "g",
                    Some(&token),
                    Operation::Attach(AttachMember {
                        group: "g",
                        member: "m",
                        attachment: &a,
                        expected_revision: &token,
                        expected_participant_id: &participant,
                        reattach: false,
                    }),
                    &io,
                ),
            ),
            10 => (
                "add_member",
                serde_json::json!({"group_key":"g","member_key":"m","role":"New role","binding":json(&b),"rebind":true}),
                mutate(
                    root.path(),
                    "g",
                    None,
                    Operation::Add(AddMember {
                        group: "g",
                        member: "m",
                        role: "New role",
                        binding: &b,
                        rebind: true,
                        expected_revision: None,
                    }),
                    &io,
                ),
            ),
            11 | 12 => (
                "remove_member",
                serde_json::json!({"group_key":"g","member_key":"m"}),
                mutate(root.path(), "g", None, Operation::Remove("m"), &io),
            ),
            15 => (
                "reserve_cycle",
                serde_json::json!({"group_key":"g","expected_revision":token,"endpoint":json(&ep),"source":"coven","epoch":"two","terminal_ids":terminals,"allow_new_epoch":true}),
                reserve_with(
                    root.path(),
                    ReserveRequest {
                        group: "g",
                        expected_revision: &token,
                        endpoint: &ep,
                        source: "coven",
                        epoch: "two",
                        terminal_ids: &terminals,
                        allow_new_epoch: true,
                    },
                    &io,
                ),
            ),
            _ => (
                "detach_live",
                serde_json::json!({"group_key":"g","member_key":"m","expected_revision":token,"expected_participant_id":participant}),
                mutate(
                    root.path(),
                    "g",
                    Some(&token),
                    Operation::Detach("m", &participant),
                    &io,
                ),
            ),
        };
        let expected = oracle(
            serde_json::json!({"mode":"mutation","raw":before.map(|b|String::from_utf8(b).unwrap()),"ids":(start..start+8).map(id).collect::<Vec<_>>(),"action":{"operation":operation,"args":args}}),
        );
        assert!(
            matches!(get(&expected, "accepted").unwrap(), Value::Bool(true)),
            "oracle rejected step {step}"
        );
        let actual = result.unwrap();
        assert!(
            equal(&actual, get(&expected, "result").unwrap()).unwrap(),
            "result step {step}: {}",
            String::from_utf8_lossy(&actual.storage().unwrap())
        );
        assert_eq!(
            raw(root.path()).unwrap(),
            text(get(&expected, "raw").unwrap()).unwrap().as_bytes(),
            "bytes step {step}"
        );
    }
}

struct Fault {
    stage: &'static str,
}
impl store::Io for Fault {
    fn replace(&self, g: &WorkspaceGuard, b: &[u8]) -> Result<Delivery> {
        if self.stage == "rename" {
            return Err(Error("test_rename_failed"));
        }
        let d = store::Io::replace(&store::SystemIo, g, b)?;
        Ok(if self.stage == "publication_sync" {
            Delivery::DurabilityUncertain
        } else {
            d
        })
    }
    fn reread(&self, root: &Path) -> Result<(Option<Value>, Option<Vec<u8>>)> {
        if self.stage == "read" {
            return Err(Error("test_lost_read"));
        }
        let (v, mut b) = store::read(root)?;
        if self.stage == "bytes" {
            b.as_mut().unwrap().push(b' ');
        }
        if self.stage == "state" {
            return Ok((None, b));
        }
        Ok((v, b))
    }
    fn open(&self, p: &Path) -> Result<fs::File> {
        if self.stage == "open" {
            Err(Error("test_open"))
        } else {
            store::Io::open(&store::SystemIo, p)
        }
    }
    fn sync_file(&self, f: &fs::File) -> Result<()> {
        if self.stage == "file_sync" {
            Err(Error("test_sync"))
        } else {
            store::Io::sync_file(&store::SystemIo, f)
        }
    }
    fn sync_dir(&self, p: &Path) -> Result<()> {
        if self.stage == "dir_sync" {
            Err(Error("test_sync"))
        } else {
            store::Io::sync_dir(&store::SystemIo, p)
        }
    }
}
#[test]
fn known_result_is_never_lost_or_rolled_back_on_confirmation_fault_including_noop() {
    for changed in [false, true] {
        for stage in [
            "rename",
            "publication_sync",
            "read",
            "bytes",
            "state",
            "open",
            "file_sync",
            "dir_sync",
            "success",
        ] {
            let root = workspace();
            create_group(root.path(), "g", "Group", None).unwrap();
            let before = raw(root.path()).unwrap();
            let binding = binding();
            let result = mutate(
                root.path(),
                "g",
                None,
                if changed {
                    Operation::Add(AddMember {
                        group: "g",
                        member: "m",
                        role: "Role",
                        binding: &binding,
                        rebind: false,
                        expected_revision: None,
                    })
                } else {
                    Operation::Create("Group")
                },
                &Fault { stage },
            );
            if changed && stage == "rename" {
                assert!(result.is_err());
                assert_eq!(raw(root.path()).unwrap(), before);
                continue;
            }
            let result = result.unwrap();
            let outcome = text(get(get(&result, "delivery").unwrap(), "outcome").unwrap()).unwrap();
            let uncertain =
                stage != "success" && (changed || !matches!(stage, "rename" | "publication_sync"));
            assert_eq!(
                outcome,
                if uncertain {
                    "durability_uncertain"
                } else {
                    "durably_committed"
                },
                "{stage} changed={changed}"
            );
            assert_eq!(
                text(get(&result, "outcome").unwrap()).unwrap(),
                if changed { "committed" } else { "unchanged" }
            );
            if changed {
                assert!(get(&result, "member").is_ok());
                assert_ne!(raw(root.path()).unwrap(), before);
            } else {
                assert_eq!(raw(root.path()).unwrap(), before);
            }
        }
    }
}

#[test]
fn reservations_whole_targets_overflow_epoch_fences_and_uncertainty_burn_without_rollback() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    let view = read_groups(root.path()).unwrap();
    let token = view.revision().to_owned();
    let ep = get(get(&b, "presentation").unwrap(), "endpoint").unwrap();
    let targets = vec!["term".to_owned()];
    let request = |epoch, allow| ReserveRequest {
        group: "g",
        expected_revision: &token,
        endpoint: ep,
        source: "coven",
        epoch,
        terminal_ids: &targets,
        allow_new_epoch: allow,
    };
    let uncertain =
        reserve_with(root.path(), request("one", false), &Fault { stage: "read" }).unwrap();
    assert_eq!(
        uint(get(get(&uncertain, "reservations").unwrap(), "term").unwrap()).unwrap(),
        1
    );
    assert_eq!(
        text(get(get(&uncertain, "delivery").unwrap(), "outcome").unwrap()).unwrap(),
        "durability_uncertain"
    );
    let next = reserve_cycle(root.path(), request("one", false)).unwrap();
    assert_eq!(
        uint(get(get(&next, "reservations").unwrap(), "term").unwrap()).unwrap(),
        2
    );
    assert_eq!(read_groups(root.path()).unwrap().revision(), token);
    let before = raw(root.path()).unwrap();
    assert!(reserve_cycle(root.path(), request("two", false)).is_err());
    assert_eq!(raw(root.path()).unwrap(), before);
    let reset = reserve_cycle(root.path(), request("two", true)).unwrap();
    assert_eq!(
        uint(get(get(&reset, "reservations").unwrap(), "term").unwrap()).unwrap(),
        1
    );
    let mut state = read_groups(root.path()).unwrap().state.unwrap();
    let mut counters = array(get(&state, "publication_counters").unwrap())
        .unwrap()
        .to_vec();
    let mut marks = get(&counters[0], "watermarks").unwrap().clone();
    marks.set_field("term", num(u64::MAX).unwrap()).unwrap();
    counters[0].set_field("watermarks", marks).unwrap();
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
    fs::write(
        root.path().join("state/groups.json"),
        state.storage().unwrap(),
    )
    .unwrap();
    let before = raw(root.path()).unwrap();
    assert!(reserve_cycle(root.path(), request("two", false)).is_err());
    assert_eq!(raw(root.path()).unwrap(), before);
    let wrong = vec!["other".into()];
    assert!(
        reserve_cycle(
            root.path(),
            ReserveRequest {
                terminal_ids: &wrong,
                ..request("two", false)
            }
        )
        .is_err()
    );
}

#[test]
fn cas_and_counter_races_are_serialized_and_attachment_readd_is_fenced() {
    let root = workspace();
    let b = binding();
    create_group(root.path(), "g", "Group", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    let view = read_groups(root.path()).unwrap();
    let token = view.revision().to_owned();
    let participant = text(get(view.member("g", "m").unwrap(), "participant_id").unwrap())
        .unwrap()
        .to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let p = root.path().to_owned();
            let t = token.clone();
            let b = b.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                add(
                    &p,
                    &b,
                    if i == 0 { "First" } else { "Second" },
                    false,
                    Some(&t),
                )
            })
        })
        .collect();
    let successes = handles
        .into_iter()
        .filter(|h| h.thread().id() != std::thread::current().id())
        .map(|h| h.join().unwrap())
        .filter(Result::is_ok)
        .count();
    assert_eq!(successes, 1);
    let view = read_groups(root.path()).unwrap();
    let token = view.revision().to_owned();
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let p = root.path().to_owned();
            let t = token.clone();
            let ep = get(get(&b, "presentation").unwrap(), "endpoint")
                .unwrap()
                .clone();
            std::thread::spawn(move || {
                reserve_cycle(
                    &p,
                    ReserveRequest {
                        group: "g",
                        expected_revision: &t,
                        endpoint: &ep,
                        source: "coven",
                        epoch: "one",
                        terminal_ids: &["term".into()],
                        allow_new_epoch: false,
                    },
                )
                .unwrap()
            })
        })
        .collect();
    let mut numbers: Vec<_> = handles
        .into_iter()
        .map(|h| {
            uint(get(get(&h.join().unwrap(), "reservations").unwrap(), "term").unwrap()).unwrap()
        })
        .collect();
    numbers.sort();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6]);
    remove_member(root.path(), "g", "m", None).unwrap();
    add(root.path(), &b, "Role", false, None).unwrap();
    let current = read_groups(root.path()).unwrap();
    assert!(
        attach_live(
            root.path(),
            AttachMember {
                group: "g",
                member: "m",
                attachment: &attachment(),
                expected_revision: &token,
                expected_participant_id: &participant,
                reattach: false
            }
        )
        .is_err()
    );
    assert_eq!(
        attach_live(
            root.path(),
            AttachMember {
                group: "g",
                member: "m",
                attachment: &attachment(),
                expected_revision: current.revision(),
                expected_participant_id: &participant,
                reattach: false
            }
        )
        .unwrap_err()
        .code(),
        "groups_participant_conflict"
    );
}
