//! Independent, native assertion evidence for the named legacy methods.
use super::*;
use crate::values::{self, Profile};
use std::{cell::Cell, fs};

fn parse(raw: &str) -> Value {
    values::parse(raw.as_bytes(), Profile::Authority).unwrap()
}
fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    get(v, key).unwrap()
}
fn same(a: &Value, b: &Value) {
    assert_eq!(a.canonical().unwrap(), b.canonical().unwrap());
}
fn at<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |v, k| field(v, k))
}
fn put(v: &mut Value, path: &[&str], value: Value) {
    let (key, parents) = path.split_last().unwrap();
    if parents.is_empty() {
        v.set_field(key, value).unwrap();
    } else {
        let mut child = field(v, parents[0]).clone();
        put(&mut child, &path[1..], value);
        v.set_field(parents[0], child).unwrap();
    }
}
fn endpoint() -> Value {
    parse(r#"{"path":"/not-a-real-kitsune.sock","device":1,"inode":2}"#)
}
fn binding(terminal: &str, native: Value) -> Value {
    let mut b = parse(
        r#"{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not-a-real-kitsune.sock","device":1,"inode":2},"terminal_id":"terminal-1","pane_id_at_binding":"pane-1"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2026-10-23T12:00:00Z","agent_kind_hint":"djinn","native_reference":null,"native_binding":"unestablished"}}"#,
    );
    put(&mut b, &["presentation", "terminal_id"], string(terminal));
    if !matches!(native, Value::Null) {
        put(
            &mut b,
            &["evidence", "native_binding"],
            string("observed_reference"),
        );
    }
    put(&mut b, &["evidence", "native_reference"], native);
    b
}
struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("state")).unwrap();
        fs::write(root.path().join("coven.json"), b"not mutable config JSON").unwrap();
        Self(root)
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn path(&self) -> std::path::PathBuf {
        self.root().join("state/groups.json")
    }
    fn raw(&self) -> Vec<u8> {
        fs::read(self.path()).unwrap()
    }
    fn read(&self) -> View {
        read_groups(self.root()).unwrap()
    }
    fn state(&self) -> Value {
        self.read().state().unwrap().clone()
    }
    fn write(&self, state: &Value) {
        fs::write(self.path(), state.storage().unwrap()).unwrap();
    }
    fn create(&self) -> Value {
        create_group(self.root(), "review", "Review", None).unwrap()
    }
    fn add(&self, member: &str, b: &Value, role: &str, rebind: bool) -> Result<Value> {
        self.add_to("review", member, b, role, rebind)
    }
    fn add_to(
        &self,
        group: &str,
        member: &str,
        b: &Value,
        role: &str,
        rebind: bool,
    ) -> Result<Value> {
        add_member(
            self.root(),
            AddMember {
                group,
                member,
                role,
                binding: b,
                rebind,
                expected_revision: None,
            },
        )
    }
    fn first(&self) -> Value {
        self.add(
            "reviewer",
            &binding("terminal-1", Value::Null),
            "Reviewer",
            false,
        )
        .unwrap()
    }
    fn remove(&self) -> Value {
        remove_member(self.root(), "review", "reviewer", None).unwrap()
    }
    fn reserve(
        &self,
        terminals: &[&str],
        source: &str,
        epoch: &str,
        reset: bool,
        ep: &Value,
    ) -> Result<Value> {
        let revision = self.read().revision().to_owned();
        let terminal_ids = terminals
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        reserve_cycle(
            self.root(),
            ReserveRequest {
                group: "review",
                expected_revision: &revision,
                endpoint: ep,
                source,
                epoch,
                terminal_ids: &terminal_ids,
                allow_new_epoch: reset,
            },
        )
    }
    fn cycle(&self, terminals: &[&str]) -> Result<Value> {
        self.reserve(terminals, "coven.groups", "epoch-1", false, &endpoint())
    }
}
fn reservations(result: &Value, expected: &str) {
    same(field(result, "reservations"), &parse(expected));
}
fn set_watermarks(state: &mut Value, watermarks: Value) {
    // Do not narrow authority integers through serde_json::Value.
    let mut counters = field(state, "publication_counters")
        .as_array()
        .unwrap()
        .to_vec();
    counters[0].set_field("watermarks", watermarks).unwrap();
    state
        .set_field("publication_counters", Value::Array(counters))
        .unwrap();
}

#[test]
fn create_retry_confirms_without_rewrite_and_name_conflicts() {
    struct Poison(Cell<usize>);
    impl store::Io for Poison {
        fn replace(
            &self,
            _: &crate::storage::WorkspaceGuard,
            _: &[u8],
        ) -> Result<crate::storage::Delivery> {
            self.0.set(self.0.get() + 1);
            panic!("no-op attempted replacement");
        }
    }
    let f = Fixture::new();
    let first = f.create();
    let raw = f.raw();
    let modified = fs::metadata(f.path()).unwrap().modified().unwrap();
    let poison = Poison(Cell::new(0));
    let retry = mutate(
        f.root(),
        "review",
        None,
        Operation::Create("Review"),
        &poison,
    )
    .unwrap();
    same(field(&retry, "outcome"), &string("unchanged"));
    same(field(&retry, "group"), field(&first, "group"));
    same(field(&retry, "revision"), field(&first, "revision"));
    assert_eq!(f.raw(), raw);
    assert_eq!(
        fs::metadata(f.path()).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(poison.0.get(), 0);
    assert!(create_group(f.root(), "review", "Other", None).is_err());
    assert!(create_group(f.root(), "review", "Review", Some(UNINITIALIZED)).is_err());
    assert_eq!(f.raw(), raw);
}

#[test]
fn binding_retry_move_role_and_explicit_rebind() {
    let f = Fixture::new();
    f.create();
    let first = f.first();
    let mut moved = binding("terminal-1", Value::Null);
    put(
        &mut moved,
        &["presentation", "pane_id_at_binding"],
        string("pane-2"),
    );
    put(
        &mut moved,
        &["evidence", "observed_at"],
        string("2026-10-23T13:00:00+00:00"),
    );
    let raw = f.raw();
    let retry = f.add("reviewer", &moved, "Reviewer", false).unwrap();
    same(field(&retry, "outcome"), &string("unchanged"));
    assert_eq!(f.raw(), raw);
    let role = f.add("reviewer", &moved, "Coordinator", false).unwrap();
    assert!(!equal(field(&role, "revision"), field(&first, "revision")).unwrap());
    same(at(&role, &["member", "binding"]), &moved);
    let resolved = binding(
        "terminal-1",
        parse(r#"{"source":"snapshot","agent":"djinn","kind":"id","value":"session-1"}"#),
    );
    assert!(f.add("reviewer", &resolved, "Reviewer", false).is_err());
    let rebound = f.add("reviewer", &resolved, "Reviewer", true).unwrap();
    same(
        at(&rebound, &["member", "participant_id"]),
        at(&first, &["member", "participant_id"]),
    );
    same(
        at(
            &rebound,
            &["member", "binding", "evidence", "native_binding"],
        ),
        &string("observed_reference"),
    );
    for (path, value) in [
        (vec!["presentation", "terminal_id"], string("terminal-2")),
        (vec!["presentation", "endpoint", "inode"], num(3).unwrap()),
        (vec!["evidence", "agent_kind_hint"], string("other")),
        (
            vec!["evidence", "native_reference", "value"],
            string("session-2"),
        ),
    ] {
        let mut changed = resolved.clone();
        put(&mut changed, &path, value);
        let raw = f.raw();
        assert!(
            f.add("reviewer", &changed, "Reviewer", false).is_err(),
            "{path:?}"
        );
        assert_eq!(f.raw(), raw);
    }
}

#[test]
fn duplicate_terminal_rejected_native_duplicates_allowed_keys_scoped() {
    let f = Fixture::new();
    f.create();
    create_group(f.root(), "other", "Other", None).unwrap();
    let native = parse(r#"{"source":"snapshot","agent":"djinn","kind":"path","value":"/session"}"#);
    let first = f
        .add(
            "reviewer",
            &binding("terminal-1", native.clone()),
            "Reviewer",
            false,
        )
        .unwrap();
    assert!(
        f.add(
            "another",
            &binding("terminal-1", Value::Null),
            "Reviewer",
            false
        )
        .is_err()
    );
    assert!(
        f.add_to(
            "other",
            "reviewer",
            &binding("terminal-1", Value::Null),
            "Role",
            false
        )
        .is_err()
    );
    let other = f
        .add_to(
            "other",
            "reviewer",
            &binding("terminal-2", native),
            "Role",
            false,
        )
        .unwrap();
    assert!(
        !equal(
            at(&first, &["member", "participant_id"]),
            at(&other, &["member", "participant_id"])
        )
        .unwrap()
    );
}

#[test]
fn remove_is_idempotent_and_retains_counter_watermarks() {
    let f = Fixture::new();
    f.create();
    f.first();
    f.cycle(&["terminal-1"]).unwrap();
    same(field(&f.remove(), "outcome"), &string("committed"));
    let raw = f.raw();
    let absent = f.remove();
    assert!(matches!(field(&absent, "member"), Value::Null));
    same(field(&absent, "outcome"), &string("unchanged"));
    assert_eq!(f.raw(), raw);
    f.first();
    reservations(&f.cycle(&["terminal-1"]).unwrap(), r#"{"terminal-1":2}"#);
}

#[test]
fn counter_cycles_preserve_content_token_and_full_target_set() {
    let f = Fixture::new();
    f.create();
    f.first();
    f.add(
        "second",
        &binding("terminal-2", Value::Null),
        "Reviewer",
        false,
    )
    .unwrap();
    let revision = f.read().revision().to_owned();
    for targets in [
        vec![],
        vec!["terminal-1"],
        vec!["terminal-1", "terminal-1"],
        vec!["terminal-1", "other"],
    ] {
        assert!(f.cycle(&targets).is_err(), "{targets:?}");
    }
    let first = f.cycle(&["terminal-2", "terminal-1"]).unwrap();
    reservations(&first, r#"{"terminal-1":1,"terminal-2":1}"#);
    let second = f.cycle(&["terminal-1", "terminal-2"]).unwrap();
    reservations(&second, r#"{"terminal-1":2,"terminal-2":2}"#);
    same(field(&first, "revision"), &string(&revision));
    same(field(&second, "revision"), &string(&revision));
    assert_eq!(f.read().revision(), revision);
    let mut ep = endpoint();
    ep.set_field("inode", num(5).unwrap()).unwrap();
    assert!(
        f.reserve(
            &["terminal-1", "terminal-2"],
            "coven.groups",
            "epoch-1",
            false,
            &ep
        )
        .is_err()
    );
    let mut b = binding("terminal-2", Value::Null);
    put(&mut b, &["presentation", "endpoint"], ep);
    f.add("second", &b, "Reviewer", true).unwrap();
    assert!(f.cycle(&["terminal-1", "terminal-2"]).is_err());
}

#[test]
fn epochs_explicit_reset_whole_map_and_no_entry_eviction() {
    let f = Fixture::new();
    f.create();
    f.first();
    f.cycle(&["terminal-1"]).unwrap();
    f.remove();
    f.add(
        "reviewer",
        &binding("terminal-2", Value::Null),
        "Reviewer",
        false,
    )
    .unwrap();
    f.cycle(&["terminal-2"]).unwrap();
    assert!(
        f.reserve(&["terminal-2"], "coven.groups", "new", false, &endpoint())
            .is_err()
    );
    reservations(
        &f.reserve(&["terminal-2"], "coven.groups", "new", true, &endpoint())
            .unwrap(),
        r#"{"terminal-2":1}"#,
    );
    same(
        field(
            &field(&f.state(), "publication_counters")
                .as_array()
                .unwrap()[0],
            "watermarks",
        ),
        &parse(r#"{"terminal-2":1}"#),
    );
    for i in 0..7 {
        f.reserve(
            &["terminal-2"],
            &format!("source-{i}"),
            "epoch-1",
            false,
            &endpoint(),
        )
        .unwrap();
    }
    let raw = f.raw();
    assert!(
        f.reserve(&["terminal-2"], "overflow", "epoch-1", false, &endpoint())
            .is_err()
    );
    assert_eq!(f.raw(), raw);
    reservations(
        &f.reserve(&["terminal-2"], "coven.groups", "new", false, &endpoint())
            .unwrap(),
        r#"{"terminal-2":2}"#,
    );
}

#[test]
fn watermark_cap_and_overflow_do_not_publish_partial_cycles() {
    let f = Fixture::new();
    f.create();
    f.first();
    f.cycle(&["terminal-1"]).unwrap();
    let mut state = f.state();
    let mut watermarks = obj(&[]).unwrap();
    for i in 0..63 {
        watermarks
            .set_field(&format!("old-{i}"), num(1).unwrap())
            .unwrap();
    }
    watermarks.set_field("terminal-1", num(1).unwrap()).unwrap();
    set_watermarks(&mut state, watermarks);
    f.write(&state);
    reservations(&f.cycle(&["terminal-1"]).unwrap(), r#"{"terminal-1":2}"#);
    f.add(
        "second",
        &binding("terminal-2", Value::Null),
        "Reviewer",
        false,
    )
    .unwrap();
    let raw = f.raw();
    assert!(f.cycle(&["terminal-1", "terminal-2"]).is_err());
    assert_eq!(f.raw(), raw);
    f.reserve(
        &["terminal-1", "terminal-2"],
        "coven.groups",
        "new",
        true,
        &endpoint(),
    )
    .unwrap();
    let mut state = f.state();
    set_watermarks(
        &mut state,
        parse(r#"{"terminal-1":1,"terminal-2":18446744073709551615}"#),
    );
    f.write(&state);
    let raw = f.raw();
    assert!(
        f.reserve(
            &["terminal-1", "terminal-2"],
            "coven.groups",
            "new",
            false,
            &endpoint()
        )
        .is_err()
    );
    assert_eq!(f.raw(), raw);
}

#[test]
fn multitarget_cycle_overflow_does_not_burn_any_target() {
    let f = Fixture::new();
    f.create();
    f.first();
    f.add(
        "second",
        &binding("terminal-2", Value::Null),
        "Reviewer",
        false,
    )
    .unwrap();
    f.cycle(&["terminal-1", "terminal-2"]).unwrap();
    let mut state = f.state();
    set_watermarks(
        &mut state,
        parse(r#"{"terminal-1":1,"terminal-2":18446744073709551615}"#),
    );
    f.write(&state);
    let raw = f.raw();
    assert!(f.cycle(&["terminal-1", "terminal-2"]).is_err());
    same(&f.state(), &state);
    assert_eq!(f.raw(), raw);
}

#[test]
fn canonical_hash_ignores_json_format_and_counters_only() {
    let f = Fixture::new();
    f.create();
    f.first();
    let first = f.read();
    let state = first.state().unwrap();
    // Rewrite actual authority bytes, not an in-memory hash-only comparison.
    let mut spaced = b" \n\t".to_vec();
    spaced.extend(state.sorted_spaced().unwrap());
    spaced.extend(b"\n \t");
    assert_ne!(spaced, f.raw());
    fs::write(f.path(), spaced).unwrap();
    assert_eq!(f.read().revision(), first.revision());
    f.cycle(&["terminal-1"]).unwrap();
    assert_eq!(f.read().revision(), first.revision());
    let mut state = f.state();
    let mut groups = field(&state, "groups").as_array().unwrap().to_vec();
    groups[0]
        .set_field("name", string("Changed outside API"))
        .unwrap();
    state.set_field("groups", Value::Array(groups)).unwrap();
    f.write(&state);
    assert_ne!(f.read().revision(), first.revision());
}
