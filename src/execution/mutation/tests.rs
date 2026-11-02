use super::*;
use std::cell::{Cell, RefCell};

const ATTEMPT: &str = "ffffffffffffffffffffffffffffffff";
const CANCEL: &str = "dddddddddddddddddddddddddddddddd";
const NOW: &str = "2026-11-02T01:02:03.123456+00:00";
const REASON: &str = "  withdraw \u{03bb}\n";

fn prepared() -> Value {
    let mut control = super::super::tests::enabled();
    super::super::tests::enroll(&mut control);
    super::super::tests::prepare(&mut control, ATTEMPT, Value::Null, 1);
    control
}
fn fixture(control: &Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("state/execution")).unwrap();
    let mut bytes = control.canonical().unwrap();
    bytes.push(b'\n');
    fs::write(dir.path().join("state/execution/control.json"), bytes).unwrap();
    // Cancellation must neither read these nor require a coven.json marker.
    for file in ["tasks.json", "team.json", "checkpoint.json"] {
        fs::write(dir.path().join("state").join(file), b"broken source").unwrap();
    }
    dir
}
fn bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("state/execution/control.json")).unwrap()
}
fn field<'a>(value: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter()
        .fold(value, |value, key| get(value, key).unwrap())
}
fn parsed(output: &TaskOutput) -> Value {
    assert!(output.stdout.is_ascii());
    assert!(!output.stdout.ends_with('\n'));
    assert!(output.warnings.is_empty());
    values::parse(output.stdout.as_bytes(), Profile::Authority).unwrap()
}

#[derive(Default)]
struct Script {
    calls: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
    code: Option<&'static str>,
    uncertain: bool,
    partial: bool,
    break_after_publish: bool,
    reads: Cell<usize>,
}
impl Script {
    fn step(&self, step: &'static str) -> Result<()> {
        self.calls.borrow_mut().push(step);
        require(
            self.fail != Some(step),
            self.code.unwrap_or("injected_failure"),
        )
    }
    fn run(
        &self,
        root: &Path,
        attempt: &str,
        reason: &str,
        expected: &str,
        key: &str,
        json: bool,
    ) -> Result<TaskOutput> {
        cancel(root, Path::new(attempt), reason, expected, key, json, self)
    }
}
impl Io for Script {
    fn read(&self, root: &Path) -> Result<Option<(Value, Validated)>> {
        self.reads.set(self.reads.get() + 1);
        self.step(if self.reads.get() == 1 {
            "initial"
        } else {
            "locked_read"
        })?;
        assert!(
            self.reads.get() <= 2,
            "mutation rediscovered a known effect"
        );
        read_typed(root)
    }
    fn lock(&self, root: &Path) -> Result<WorkspaceGuard> {
        self.step("lock")?;
        WorkspaceGuard::execution_maintenance(root, Duration::ZERO)
    }
    fn binding(&self, value: &Value, schema: usize) -> Result<()> {
        self.step("binding")?;
        SystemIo.binding(value, schema)
    }
    fn metadata(&self) -> Result<(String, String)> {
        self.step("metadata")?;
        Ok((CANCEL.to_owned(), NOW.to_owned()))
    }
    fn admit(&self, bytes: &[u8]) -> Result<Validated> {
        self.step("admit")?;
        SystemIo.admit(bytes)
    }
    fn output(
        &self,
        view: &Validated,
        result: &Value,
        replayed: bool,
        json: bool,
    ) -> Result<delivery::Prepared> {
        self.step("output")?;
        SystemIo.output(view, result, replayed, json)
    }
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        if self.partial {
            let mut file = tempfile::NamedTempFile::new_in(guard.root().join("state/execution"))?;
            std::io::Write::write_all(&mut file, &bytes[..bytes.len() / 2])?;
            file.as_file().sync_all()?;
            self.step("partial_write")?;
        }
        self.step("replace")?;
        let outcome = SystemIo.replace(guard, bytes)?;
        if self.break_after_publish {
            fs::write(guard.root().join("state/execution/control.json"), b"null")?;
        }
        Ok(if self.uncertain {
            Delivery::DurabilityUncertain
        } else {
            outcome
        })
    }
    fn confirm(&self, root: &Path, view: &Validated, guard: &WorkspaceGuard) -> Result<()> {
        self.step("confirm")?;
        SystemIo.confirm(root, view, guard)
    }
}

#[test]
fn v1_exact_effect_bytes_ascii_modes_and_cli_source_independence() {
    for json in [false, true] {
        let control = prepared();
        let root = fixture(&control);
        let rev = validated(&control).unwrap().replayed.revision;
        let io = Script::default();
        let output = io
            .run(root.path(), ATTEMPT, REASON, &rev, "cancel", json)
            .unwrap();
        assert_eq!(output.exit_code, 0);
        let value = parsed(&output);
        let result = get(&value, "result").unwrap();
        let expected = object(vec![
            ("operation", string("cancel")),
            ("generation", integer(4).unwrap()),
            (
                "execution_authority_id",
                string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ),
            ("attempt_id", string(ATTEMPT)),
            ("state", string("cancelled_before_submission")),
            (
                "transition",
                object(vec![
                    ("cancel_id", string(CANCEL)),
                    ("attempt_id", string(ATTEMPT)),
                    ("reason", string(REASON)),
                    ("cancelled_at", string(NOW)),
                ])
                .unwrap(),
            ),
        ])
        .unwrap();
        assert_eq!(result.canonical().unwrap(), expected.canonical().unwrap());
        assert_eq!(
            &*io.calls.borrow(),
            &[
                "initial",
                "binding",
                "lock",
                "locked_read",
                "binding",
                "metadata",
                "admit",
                "output",
                "replace",
                "confirm"
            ]
        );
        let saved = values::parse(&bytes(root.path()), Profile::Authority).unwrap();
        let mut canonical = saved.canonical().unwrap();
        canonical.push(b'\n');
        assert_eq!(bytes(root.path()), canonical);
        assert_eq!(
            get(&saved, "extensions").unwrap().canonical().unwrap(),
            get(&control, "extensions").unwrap().canonical().unwrap()
        );
        // Real public CLI adapter, including initial read and retained-key retry.
        let output = cancel_arguments(
            &[root.path().to_owned(), ATTEMPT.into()],
            REASON,
            &rev,
            "cancel",
            json,
        )
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(matches!(
            field(&parsed(&output), &["delivery", "replayed"]),
            Value::Bool(true)
        ));
    }
}

#[test]
fn all_prepublication_faults_leave_authority_unchanged() {
    for step in [
        "initial",
        "binding",
        "lock",
        "locked_read",
        "metadata",
        "admit",
        "output",
        "replace",
        "partial_write",
    ] {
        let control = prepared();
        let root = fixture(&control);
        let before = bytes(root.path());
        let io = Script {
            fail: Some(step),
            partial: step == "partial_write",
            ..Script::default()
        };
        let rev = validated(&control).unwrap().replayed.revision;
        assert!(
            io.run(root.path(), ATTEMPT, REASON, &rev, "cancel", false)
                .is_err(),
            "{step}"
        );
        assert_eq!(bytes(root.path()), before, "{step}");
        assert!(!io.calls.borrow().contains(&"confirm"));
    }
}

#[test]
fn every_postknown_confirmation_error_retains_exact_frozen_receipt() {
    for code in [
        "input_or_storage_unavailable",
        "unsupported_json_size_profile",
        "unsupported_execution_work_profile",
        "unsafe_managed_path",
        "unsupported_json_output_profile",
        "unsafe_archive_path",
        "sync_failure",
        "invalid_execution_fields",
    ] {
        for replay in [false, true] {
            let control = prepared();
            let root = fixture(&control);
            let rev = validated(&control).unwrap().replayed.revision;
            let first = Script::default()
                .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
                .unwrap();
            if !replay {
                fs::write(
                    root.path().join("state/execution/control.json"),
                    control.canonical().unwrap(),
                )
                .unwrap();
            }
            let io = Script {
                fail: Some("confirm"),
                code: Some(code),
                ..Script::default()
            };
            let output = io
                .run(root.path(), ATTEMPT, REASON, &rev, "cancel", false)
                .unwrap();
            assert_eq!(output.exit_code, 2, "{code}");
            let value = parsed(&output);
            assert_eq!(
                get(&value, "result").unwrap().canonical().unwrap(),
                get(&parsed(&first), "result").unwrap().canonical().unwrap()
            );
            assert!(matches!(
                field(&value, &["delivery", "current_revision"]),
                Value::Null
            ));
            assert!(matches!(
                field(&value, &["delivery", "attempt_state"]),
                Value::Null
            ));
            assert_eq!(
                field(&value, &["delivery", "replayed"]).compact().unwrap(),
                if replay {
                    b"true".to_vec()
                } else {
                    b"false".to_vec()
                }
            );
            assert!(output.stdout.contains(code));
            assert_eq!(io.reads.get(), 2);
        }
    }
}

#[test]
fn uncertain_rename_never_confirms_or_rereads_even_with_broken_authority() {
    for broken in [false, true] {
        let control = prepared();
        let root = fixture(&control);
        let rev = validated(&control).unwrap().replayed.revision;
        let io = Script {
            uncertain: true,
            break_after_publish: broken,
            ..Script::default()
        };
        let output = io
            .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
            .unwrap();
        assert_eq!(output.exit_code, 2);
        parsed(&output);
        assert!(!io.calls.borrow().contains(&"confirm"));
        assert_eq!(io.reads.get(), 2);
    }
}

#[test]
fn committed_then_corrupt_authority_is_known_uncertainty_not_hard_failure() {
    let control = prepared();
    let root = fixture(&control);
    let rev = validated(&control).unwrap().replayed.revision;
    let io = Script {
        break_after_publish: true,
        ..Script::default()
    };
    let output = io
        .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
        .unwrap();
    assert_eq!(output.exit_code, 2);
    assert_eq!(
        field(&parsed(&output), &["result", "transition", "cancel_id"]).as_str(),
        Some(CANCEL)
    );
}

#[test]
fn invalid_and_disabled_admission_precedes_first_lock_but_not_initial_read() {
    let control = prepared();
    let root = fixture(&control);
    let rev = validated(&control).unwrap().replayed.revision;
    let long_reason = "x".repeat(16385);
    let long_key = "k".repeat(129);
    let long_expected = "e".repeat(values::MAX_STRING + 1);
    for (attempt, reason, expected, key) in [
        ("bad", REASON, rev.as_str(), "key"),
        (ATTEMPT, " \n", &rev, "key"),
        (ATTEMPT, &long_reason, &rev, "key"),
        (ATTEMPT, REASON, &rev, &long_key),
        (ATTEMPT, REASON, &long_expected, "key"),
        (ATTEMPT, REASON, "invalid", "key"),
        (ATTEMPT, REASON, &rev, "!key"),
    ] {
        let io = Script::default();
        assert!(
            io.run(root.path(), attempt, reason, expected, key, true)
                .is_err()
        );
        assert_eq!(io.reads.get(), 1);
        assert!(!io.calls.borrow().contains(&"lock"));
    }
    fs::remove_dir_all(root.path().join("state/execution")).unwrap();
    let io = Script::default();
    assert_eq!(
        io.run(root.path(), ATTEMPT, REASON, DISABLED, "key", true)
            .err()
            .unwrap()
            .code(),
        "execution_disabled"
    );
    assert!(!io.calls.borrow().contains(&"lock"));
    fs::create_dir(root.path().join("state/execution")).unwrap();
    fs::write(root.path().join("state/execution/control.json"), b"null").unwrap();
    assert_eq!(
        Script::default()
            .run(root.path(), "bad", "", "bad", "!", true)
            .err()
            .unwrap()
            .code(),
        "invalid_execution_fields"
    );
}

#[test]
fn initial_binding_syntax_precedes_key_validation_and_lock() {
    let root = fixture(&prepared());
    let before = bytes(root.path());
    for expected in ["invalid", "execution-control-v2:invalid"] {
        for key in ["!key", ""] {
            let io = Script::default();
            assert_eq!(
                io.run(root.path(), ATTEMPT, REASON, expected, key, true)
                    .err()
                    .unwrap()
                    .code(),
                "invalid_execution_revision"
            );
            assert_eq!(io.reads.get(), 1);
            assert_eq!(&*io.calls.borrow(), &["initial", "binding"]);
            assert!(!root.path().join("state/.workspace.lock").exists());
            assert_eq!(bytes(root.path()), before);
        }
    }
    fs::write(root.path().join("state/execution/control.json"), b"null").unwrap();
    let io = Script::default();
    assert_eq!(
        io.run(root.path(), "bad", "", "invalid", "!key", true)
            .err()
            .unwrap()
            .code(),
        "invalid_execution_fields"
    );
    assert_eq!(io.reads.get(), 1);
    assert_eq!(&*io.calls.borrow(), &["initial"]);
}

#[test]
fn retry_precedes_cas_and_live_state_conflicts_and_corrupt_tail_refuse() {
    let control = prepared();
    let root = fixture(&control);
    let rev = validated(&control).unwrap().replayed.revision;
    let first = Script::default()
        .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
        .unwrap();
    assert_eq!(first.exit_code, 0);
    let saved = read_typed(root.path()).unwrap().unwrap().0;
    let receipt = get(&saved, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        field(receipt, &["binding", "expected_execution_revision"]).as_str(),
        Some(rev.as_str())
    );
    let frozen = get(&parsed(&first), "result").unwrap().canonical().unwrap();
    for (reason, token, id) in [
        ("changed", rev.as_str(), ATTEMPT),
        (REASON, DISABLED, ATTEMPT),
        (REASON, rev.as_str(), CANCEL),
    ] {
        assert_eq!(
            Script::default()
                .run(root.path(), id, reason, token, "cancel", true)
                .err()
                .unwrap()
                .code(),
            "execution_request_key_binding_conflict"
        );
    }
    let (mut saved, state) = read_typed(root.path()).unwrap().unwrap();
    assert_eq!(
        Script::default()
            .run(
                root.path(),
                ATTEMPT,
                REASON,
                &state.replayed.revision,
                "new",
                true
            )
            .err()
            .unwrap()
            .code(),
        "execution_attempt_not_prepared"
    );
    super::super::tests::prepare(
        &mut saved,
        "11111111111111111111111111111111",
        string(ATTEMPT),
        2,
    );
    fs::write(
        root.path().join("state/execution/control.json"),
        saved.canonical().unwrap(),
    )
    .unwrap();
    let output = Script::default()
        .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
        .unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(matches!(
        field(&parsed(&output), &["delivery", "replayed"]),
        Value::Bool(true)
    ));
    assert_eq!(
        get(&parsed(&output), "result")
            .unwrap()
            .canonical()
            .unwrap(),
        frozen
    );
    let mut history = get(&saved, "history").unwrap().as_array().unwrap().to_vec();
    history
        .last_mut()
        .unwrap()
        .set_field("result", Value::Null)
        .unwrap();
    saved.set_field("history", Value::Array(history)).unwrap();
    fs::write(
        root.path().join("state/execution/control.json"),
        saved.canonical().unwrap(),
    )
    .unwrap();
    assert_eq!(
        Script::default()
            .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
            .err()
            .unwrap()
            .code(),
        "execution_receipt_effect_mismatch"
    );
}

#[test]
fn legacy_v1_cancel_key_survives_upgrade_with_original_binding_schema() {
    let control = prepared();
    let root = fixture(&control);
    let rev = validated(&control).unwrap().replayed.revision;
    let first = Script::default()
        .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
        .unwrap();
    let saved = read_typed(root.path()).unwrap().unwrap().0;
    let upgraded = super::super::tests::upgraded(saved);
    fs::write(
        root.path().join("state/execution/control.json"),
        upgraded.canonical().unwrap(),
    )
    .unwrap();
    let io = Script::default();
    let output = io
        .run(root.path(), ATTEMPT, REASON, &rev, "cancel", true)
        .unwrap();
    assert_eq!(
        get(&parsed(&output), "result")
            .unwrap()
            .canonical()
            .unwrap(),
        get(&parsed(&first), "result").unwrap().canonical().unwrap()
    );
    assert!(matches!(
        field(&parsed(&output), &["delivery", "replayed"]),
        Value::Bool(true)
    ));
    assert!(!io.calls.borrow().contains(&"metadata"));
    assert!(!io.calls.borrow().contains(&"replace"));
}

#[test]
fn v2_graph_bound_attempts_use_shared_cancel_reducer() {
    let vectors = values::parse(
        include_bytes!("../inspection/projection/vectors.json"),
        Profile::Authority,
    )
    .unwrap();
    let mut graph_bound = 0;
    for vector in vectors.as_array().unwrap() {
        let control = get(vector, "control").unwrap();
        let view = validated(control).unwrap();
        if view.schema != 2 {
            continue;
        }
        for attempt in &view.replayed.state.attempts {
            if !attempt.active() {
                continue;
            }
            if matches!(
                get(&attempt.prepared, "graph_binding").unwrap(),
                Value::Null
            ) {
                continue;
            } else {
                graph_bound += 1;
            }
            let root = fixture(control);
            let io = Script::default();
            let output = io
                .run(
                    root.path(),
                    &attempt.id,
                    REASON,
                    &view.replayed.revision,
                    "native-cancel",
                    true,
                )
                .unwrap();
            assert_eq!(output.exit_code, 0);
            let value = parsed(&output);
            assert_eq!(
                field(&value, &["delivery", "attempt_state"]).as_str(),
                Some("cancelled_before_submission")
            );
            let retry = Script::default()
                .run(
                    root.path(),
                    &attempt.id,
                    REASON,
                    &view.replayed.revision,
                    "native-cancel",
                    false,
                )
                .unwrap();
            assert_eq!(
                get(&parsed(&retry), "result").unwrap().canonical().unwrap(),
                get(&value, "result").unwrap().canonical().unwrap()
            );
        }
    }
    assert!(graph_bound > 0);
}

#[test]
fn exact_native_persisted_input_limit_includes_terminal_lf() {
    let control = prepared();
    let root = fixture(&control);
    let mut raw = bytes(root.path());
    raw.resize(values::MAX_INPUT, b' ');
    fs::write(root.path().join("state/execution/control.json"), &raw).unwrap();
    assert!(read_typed(root.path()).is_ok());
    raw.push(b' ');
    fs::write(root.path().join("state/execution/control.json"), &raw).unwrap();
    let io = Script::default();
    assert_eq!(
        io.run(root.path(), ATTEMPT, REASON, DISABLED, "cancel", true)
            .err()
            .unwrap()
            .code(),
        "handoff_size_limit"
    );
    assert_eq!(bytes(root.path()), raw);
    assert!(!io.calls.borrow().contains(&"lock"));
}

#[test]
fn admitted_diagnostic_slot_is_bounded_json_safe_and_does_not_match_reason() {
    const LONG_CODE: &str = concat!(
        "quote\"\n\u{03bb}",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    assert!(LONG_CODE.len() > 128);
    let long_sanitized = format!("quote____{}", &"0123456789abcdef".repeat(8)[..119]);
    for (pattern, code, sanitized) in [
        ("\u{03bb}\0\n\"\\", "quote\"\n\u{03bb}", "quote____"),
        ("\u{1f98a}\t\r\u{0008}", LONG_CODE, long_sanitized.as_str()),
    ] {
        let mut reason = format!("{}{}", delivery::PREFIX, delivery::SLOT);
        while reason.len() + pattern.len() <= 16384 {
            reason.push_str(pattern);
        }
        reason.push_str(&"x".repeat(16384 - reason.len()));
        assert_eq!(reason.len(), 16384);
        for json in [false, true] {
            let control = prepared();
            let root = fixture(&control);
            let rev = validated(&control).unwrap().replayed.revision;
            let mut frozen = None;
            for replayed in [false, true] {
                let before = bytes(root.path());
                let io = Script {
                    fail: Some("confirm"),
                    code: Some(code),
                    ..Script::default()
                };
                let output = io
                    .run(root.path(), ATTEMPT, &reason, &rev, "cancel", json)
                    .unwrap();
                assert_eq!(output.exit_code, 2);
                let mut value = parsed(&output);
                assert_eq!(
                    field(&value, &["result", "transition", "reason"]).as_str(),
                    Some(reason.as_str())
                );
                let result = get(&value, "result").unwrap().canonical().unwrap();
                if let Some(frozen) = &frozen {
                    assert_eq!(&result, frozen);
                } else {
                    frozen = Some(result.clone());
                }
                let (saved, view) = read_typed(root.path()).unwrap().unwrap();
                let receipt = get(&saved, "history")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap();
                assert_eq!(get(receipt, "result").unwrap().canonical().unwrap(), result);
                let mut delivery = get(&value, "delivery").unwrap().clone();
                assert_eq!(
                    get(&delivery, "outcome").unwrap().as_str(),
                    Some("durability_uncertain")
                );
                assert_eq!(
                    get(&delivery, "replayed").unwrap().compact().unwrap(),
                    if replayed {
                        b"true".as_slice()
                    } else {
                        b"false".as_slice()
                    }
                );
                assert!(matches!(
                    get(&delivery, "current_revision").unwrap(),
                    Value::Null
                ));
                assert!(matches!(
                    get(&delivery, "attempt_state").unwrap(),
                    Value::Null
                ));
                assert_eq!(
                    get(&delivery, "known_control_revision").unwrap().as_str(),
                    Some(view.replayed.revision.as_str())
                );
                let warnings = get(&delivery, "warnings").unwrap().as_array().unwrap();
                assert_eq!(warnings.len(), 1);
                let warning = text(&warnings[0]).unwrap();
                assert_eq!(warning, format!("{}{sanitized}", delivery::PREFIX));
                assert!(warning.len() <= delivery::PREFIX.len() + 128);
                assert!(
                    warning
                        .strip_prefix(delivery::PREFIX)
                        .unwrap()
                        .bytes()
                        .all(
                            |b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
                        )
                );

                // Restore the admitted 128-byte slot to prove substitution cannot grow output.
                delivery
                    .set_field(
                        "warnings",
                        Value::Array(vec![string(format!(
                            "{}{}",
                            delivery::PREFIX,
                            delivery::SLOT
                        ))]),
                    )
                    .unwrap();
                value.set_field("delivery", delivery).unwrap();
                let admitted = if json {
                    value.compact().unwrap()
                } else {
                    value.storage().unwrap()
                };
                let admitted = std::str::from_utf8(&admitted).unwrap();
                let admitted =
                    crate::output::ascii_json(admitted.strip_suffix('\n').unwrap_or(admitted))
                        .unwrap();
                assert!(output.stdout.len() <= admitted.len());
                assert!(admitted.len() <= values::MAX_OUTPUT - 8192);
                assert_eq!(admitted.len() - output.stdout.len(), 128 - sanitized.len());
                assert_eq!(io.reads.get(), 2);
                for step in ["metadata", "replace"] {
                    assert_eq!(
                        io.calls
                            .borrow()
                            .iter()
                            .filter(|&&call| call == step)
                            .count(),
                        usize::from(!replayed)
                    );
                }
                if replayed {
                    assert_eq!(bytes(root.path()), before);
                    assert!(!io.calls.borrow().contains(&"admit"));
                } else {
                    assert_ne!(bytes(root.path()), before);
                }
            }
        }
    }
}

#[test]
fn python_v1_v2_standalone_byte_goldens_include_uuid_time_and_envelope() {
    let output = std::process::Command::new("python3")
        .args(["-B", "src/execution/mutation/oracle.py"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let vectors = values::parse(&output.stdout, Profile::Authority).unwrap();
    for vector in vectors.as_array().unwrap() {
        for json in [false, true] {
            let root = fixture(get(vector, "control").unwrap());
            let io = Script::default();
            let output = io
                .run(
                    root.path(),
                    text(get(vector, "attempt").unwrap()).unwrap(),
                    REASON,
                    text(get(vector, "revision").unwrap()).unwrap(),
                    "cancel",
                    json,
                )
                .unwrap();
            assert_eq!(output.exit_code, 0);
            assert_eq!(
                parsed(&output).canonical().unwrap(),
                get(vector, "expected").unwrap().canonical().unwrap()
            );
            assert_eq!(
                bytes(root.path()),
                text(get(vector, "persisted").unwrap()).unwrap().as_bytes()
            );
            let retry = Script::default()
                .run(
                    root.path(),
                    text(get(vector, "attempt").unwrap()).unwrap(),
                    REASON,
                    text(get(vector, "revision").unwrap()).unwrap(),
                    "cancel",
                    json,
                )
                .unwrap();
            assert_eq!(retry.exit_code, 0);
            assert!(matches!(
                field(&parsed(&retry), &["delivery", "replayed"]),
                Value::Bool(true)
            ));
        }
    }
}

// Extensions are intentionally part of every v1 prefix. Rebind their hashes,
// retaining every effect/result byte and without weakening the production reader.
fn padded(mut control: Value, size: usize) -> Value {
    let mut extensions = get(&control, "extensions").unwrap().clone();
    let mut remaining = size;
    let mut index = 0;
    while remaining > 0 {
        let count = remaining.min(values::MAX_STRING);
        extensions
            .set_field(&format!("padding-{index}"), string("p".repeat(count)))
            .unwrap();
        index += 1;
        remaining -= count;
    }
    control.set_field("extensions", extensions).unwrap();
    let history = get(&control, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    control.set_field("history", Value::Array(vec![])).unwrap();
    for (index, mut entry) in history.into_iter().enumerate() {
        if index > 0 {
            let revision = format!(
                "execution-control-v1:{}:{index}:{}",
                text(get(&control, "execution_authority_id").unwrap()).unwrap(),
                canonical::digest(&control.canonical().unwrap())
            );
            let mut binding = get(&entry, "binding").unwrap().clone();
            binding
                .set_field("expected_execution_revision", string(revision))
                .unwrap();
            entry.set_field("binding", binding).unwrap();
        }
        let mut history = get(&control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        history.push(entry);
        control.set_field("history", Value::Array(history)).unwrap();
        control
            .set_field("generation", integer(index + 1).unwrap())
            .unwrap();
    }
    control
}

#[test]
fn candidate_native_eight_mib_exact_and_one_byte_over_are_prepublication() {
    let control = prepared();
    let small = fixture(&control);
    let revision = validated(&control).unwrap().replayed.revision;
    Script::default()
        .run(small.path(), ATTEMPT, REASON, &revision, "cancel", true)
        .unwrap();
    let growth = bytes(small.path()).len() - (control.canonical().unwrap().len() + 1);
    let prototype = padded(control.clone(), 8 * 1_048_576 - 16384);
    let overhead = prototype.canonical().unwrap().len() + 1 - (8 * 1_048_576 - 16384);
    for extra in [0, 1] {
        let control = padded(
            control.clone(),
            values::MAX_INPUT - overhead - growth + extra,
        );
        let root = fixture(&control);
        let before = bytes(root.path());
        let revision = validated(&control).unwrap().replayed.revision;
        let io = Script::default();
        let output = io.run(root.path(), ATTEMPT, REASON, &revision, "cancel", true);
        if extra == 0 {
            assert_eq!(output.unwrap().exit_code, 0);
            assert_eq!(bytes(root.path()).len(), values::MAX_INPUT);
            assert!(read_typed(root.path()).is_ok());
        } else {
            assert_eq!(
                output.err().unwrap().code(),
                "unsupported_json_size_profile"
            );
            assert_eq!(bytes(root.path()), before);
            assert!(!io.calls.borrow().contains(&"replace"));
        }
    }
}

#[test]
fn native_combined_history_admission_and_work_boundaries_are_not_raised() {
    for schema in [1, 2] {
        for count in [1024, 1025] {
            let mut control = if schema == 1 {
                super::super::tests::enabled()
            } else {
                super::super::tests::upgraded(super::super::tests::enabled())
            };
            control
                .set_field(
                    "history",
                    Value::Array(vec![Value::Null; count - usize::from(schema == 2)]),
                )
                .unwrap();
            control
                .set_field("generation", integer(count).unwrap())
                .unwrap();
            let error = SystemIo.admit(&control.canonical().unwrap()).err().unwrap();
            assert_eq!(
                error.code(),
                if count == 1025 {
                    "unsupported_execution_history_profile"
                } else {
                    "invalid_execution_fields"
                }
            );
        }
    }
    assert!(preflight(MAX_WORK / 1024 - 32, 1024).is_ok());
    assert_eq!(
        preflight(MAX_WORK / 1024 - 31, 1024).err().unwrap().code(),
        "unsupported_execution_work_profile"
    );
    assert!(capacity(MAX_CONTROL - 1 - CANCELLATION_RESERVE, 1).is_ok());
    assert!(capacity(MAX_CONTROL - CANCELLATION_RESERVE, 1).is_err());
}

#[test]
fn candidate_prefix_work_profile_can_refuse_a_valid_predecessor_without_changes() {
    let mut control = prepared();
    for index in 0..16 {
        let attempt = if index == 0 {
            ATTEMPT.to_owned()
        } else {
            format!("{:032x}", index)
        };
        super::super::tests::cancel(&mut control, &attempt, &format!("{:032x}", index + 100));
        super::super::tests::prepare(
            &mut control,
            &format!("{:032x}", index + 1),
            string(attempt),
            index + 2,
        );
    }
    let length = get(&control, "history").unwrap().as_array().unwrap().len();
    assert_eq!(length, 35);
    let budget = MAX_WORK / length - 4096;
    let baseline = padded(control.clone(), budget);
    let overhead = baseline.canonical().unwrap().len() - budget;
    let control = padded(control, budget - overhead);
    let root = fixture(&control);
    let before = bytes(root.path());
    let revision = validated(&control).unwrap().replayed.revision;
    let io = Script::default();
    let error = io
        .run(
            root.path(),
            &format!("{:032x}", 16),
            REASON,
            &revision,
            "cancel",
            true,
        )
        .err()
        .unwrap();
    assert_eq!(error.code(), "unsupported_execution_work_profile");
    assert_eq!(bytes(root.path()), before);
    assert!(io.calls.borrow().contains(&"admit"));
    assert!(!io.calls.borrow().contains(&"replace"));
}

#[test]
#[cfg(unix)]
fn replacement_preserves_control_mode_and_all_source_bytes_without_secondaries() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let control = prepared();
    let root = fixture(&control);
    let path = root.path().join("state/execution/control.json");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let before_inode = fs::metadata(&path).unwrap().ino();
    fs::write(
        root.path().join("state/.workspace.lock"),
        b"retained lock bytes",
    )
    .unwrap();
    let lock_inode = fs::metadata(root.path().join("state/.workspace.lock"))
        .unwrap()
        .ino();
    let revision = validated(&control).unwrap().replayed.revision;
    let output = Script::default()
        .run(root.path(), ATTEMPT, REASON, &revision, "cancel", false)
        .unwrap();
    assert_eq!(output.exit_code, 0);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o640);
    assert_ne!(fs::metadata(&path).unwrap().ino(), before_inode);
    assert_eq!(
        fs::metadata(root.path().join("state/.workspace.lock"))
            .unwrap()
            .ino(),
        lock_inode
    );
    assert_eq!(
        fs::read(root.path().join("state/.workspace.lock")).unwrap(),
        b"retained lock bytes"
    );
    for file in ["tasks.json", "team.json", "checkpoint.json"] {
        assert_eq!(
            fs::read(root.path().join("state").join(file)).unwrap(),
            b"broken source"
        );
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 5);
    assert_eq!(
        fs::read_dir(root.path().join("state/execution"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn metadata_invalid_before_publish_and_selector_arity_is_closed() {
    struct InvalidMetadata;
    impl Io for InvalidMetadata {
        fn metadata(&self) -> Result<(String, String)> {
            Ok(("invalid".into(), NOW.into()))
        }
    }
    let control = prepared();
    let root = fixture(&control);
    let before = bytes(root.path());
    let revision = validated(&control).unwrap().replayed.revision;
    assert_eq!(
        cancel(
            root.path(),
            Path::new(ATTEMPT),
            REASON,
            &revision,
            "cancel",
            true,
            &InvalidMetadata
        )
        .err()
        .unwrap()
        .code(),
        "invalid_execution_uuid"
    );
    assert_eq!(bytes(root.path()), before);
    for items in [vec![], vec!["one".into(), "two".into(), "three".into()]] {
        assert_eq!(
            cancel_arguments(&items, REASON, &revision, "cancel", true)
                .err()
                .unwrap()
                .code(),
            "invalid_execution_selector_arguments"
        );
    }
}
