use super::*;
use crate::execution::mutation::delivery;
use std::{process::Command, sync::OnceLock};

const NOW: &str = "9999-12-31T23:59:59.123456-23:59";

fn fixtures() -> &'static [Value] {
    static FIXTURES: OnceLock<Vec<Value>> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        let output = Command::new("python3")
            .args([
                "-B",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/execution/mutation/capacity/oracle.py"
                ),
            ])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        values::parse(&output.stdout, Profile::Authority)
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec()
    })
}

fn measured(control: &Value) -> (Validated, usize, usize) {
    let mut bytes = control.canonical().unwrap();
    bytes.push(b'\n');
    let parsed = values::parse(&bytes, Profile::Authority).unwrap();
    let mut work = Work::default();
    let view = execution::validated_with_work(&parsed, &mut work).unwrap();
    (view, bytes.len(), work.0)
}

// None revokes review, Some cancels. IDs/key/timestamp are deterministic and
// maximal width; only reducer-derived results are ever appended.
fn escape(control: &mut Value, attempt: Option<&str>, reason: &str) -> Value {
    let (mut view, _, _) = measured(control);
    let generation = view.replayed.receipts.len() + 1;
    let (operation, parameters, effect) = if let Some(attempt) = attempt {
        (
            "cancel",
            object(vec![
                ("attempt_id", string(attempt)),
                ("reason", string(reason)),
            ])
            .unwrap(),
            object(vec![
                ("cancel_id", string(format!("{generation:032x}"))),
                ("attempt_id", string(attempt)),
                ("reason", string(reason)),
                ("cancelled_at", string(NOW)),
            ])
            .unwrap(),
        )
    } else {
        let graph = view.graph.as_ref().unwrap();
        let revision = get(graph, "current_graph_revision").unwrap();
        let current = get(get(graph, "graphs").unwrap(), text(revision).unwrap()).unwrap();
        let params = object(vec![
            ("graph_revision", revision.clone()),
            ("decision", string("changes-requested")),
            ("reason", string(reason)),
            (
                "expected_graph_digest",
                get(current, "digest").unwrap().clone(),
            ),
        ])
        .unwrap();
        let decision = object(vec![
            ("decision_id", string(format!("{generation:032x}"))),
            ("graph_revision", revision.clone()),
            ("decision", string("changes-requested")),
            ("reason", string(reason)),
            ("state", string("changes_requested")),
            ("decided_at", string(NOW)),
        ])
        .unwrap();
        (
            "graph_decide",
            params,
            object(vec![("decision", decision)]).unwrap(),
        )
    };
    let binding = object(vec![
        ("schema_version", integer(view.schema).unwrap()),
        ("operation", string(operation)),
        ("parameters", parameters),
        (
            "expected_execution_revision",
            string(&view.replayed.revision),
        ),
        ("expected_tasks_revision", Value::Null),
    ])
    .unwrap();
    let result = execution::apply_successor(
        control,
        Some(&mut view),
        &binding,
        &effect,
        generation,
        &mut Work::default(),
    )
    .unwrap();
    let entry = object(vec![
        ("generation", integer(generation).unwrap()),
        ("request_id", string(format!("{generation:0128}"))),
        ("binding", binding),
        ("effect", effect),
        ("result", result),
    ])
    .unwrap();
    let mut history = get(control, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    history.push(entry.clone());
    control.set_field("history", Value::Array(history)).unwrap();
    control
        .set_field("generation", integer(generation).unwrap())
        .unwrap();
    entry
}

fn check_sequence(initial: &Value, order: &[Option<String>], reason: &str) {
    let (view, length, work) = measured(initial);
    admit(initial, &view, length, work).unwrap();
    let e = escape_count(&view).unwrap();
    assert_eq!(e, order.len());
    let enlarged = length + e * BOUND;
    let work_bound = work + e * BOUND + e * (enlarged + 32 + 8 * BOUND);
    let nodes = node_count(initial, 0).unwrap();
    let mut control = initial.clone();
    let mut previous_length = length;
    let mut previous_work = work;
    for (index, attempt) in order.iter().enumerate() {
        let entry = escape(&mut control, attempt.as_deref(), reason);
        let (view, length, work) = measured(&control);
        assert_eq!(escape_count(&view).unwrap(), e - index - 1);
        assert!(length - previous_length <= BOUND);
        previous_length = length;
        assert!(length <= enlarged && work <= work_bound && work <= MAX_WORK);
        assert!(work - previous_work <= BOUND + enlarged + 32 + 8 * BOUND);
        previous_work = work;
        admit(&control, &view, length, work).unwrap();
        assert!(node_count(&entry, 0).unwrap() <= ESCAPE_NODES);
        assert!(node_count(&control, 0).unwrap() <= nodes + (index + 1) * ESCAPE_NODES);
        // Three 16 KiB reasons at <=6 JSON bytes per input byte plus <8 KiB
        // closed-schema punctuation/metadata, comma and generation growth.
        let schema_bound = 3 * 16384 * 6 + 8192;
        assert!(entry.canonical().unwrap().len() + 8 <= schema_bound);
        assert!(schema_bound < BOUND);
        let result = get(&entry, "result").unwrap();
        assert!(
            !String::from_utf8(result.canonical().unwrap())
                .unwrap()
                .contains("complete_task_snapshot")
        );
        // One reason copy plus <8 KiB metadata/pretty spacing in the result;
        // delivery adds <8 KiB, so both ASCII modes are comfortably <1 MiB.
        for json in [false, true] {
            let prepared = delivery::Prepared::new(&view, result, false, json).unwrap();
            let committed = prepared.confirm(Ok(()));
            let uncertain = delivery::Prepared::new(&view, result, false, json)
                .unwrap()
                .uncertain(Error("execution_control_durability_uncertain"));
            for output in [committed, uncertain] {
                assert!(output.stdout.is_ascii());
                assert!(output.stdout.len() <= 16384 * 6 + 16384);
                assert!(output.stdout.len() < 1_048_576);
            }
        }
    }
    // Retained legacy bytes and all old receipts are unchanged by every order.
    if view.schema == 2 {
        assert_eq!(
            get(&control, "legacy_v1").unwrap().canonical().unwrap(),
            get(initial, "legacy_v1").unwrap().canonical().unwrap()
        );
    }
    let old = get(initial, "history").unwrap().as_array().unwrap();
    let new = get(&control, "history").unwrap().as_array().unwrap();
    assert_eq!(
        Value::Array(old.to_vec()).canonical().unwrap(),
        Value::Array(new[..old.len()].to_vec()).canonical().unwrap()
    );
}

#[test]
fn all_cancel_and_revoke_orders_with_maximum_actual_entries() {
    let control = &fixtures()[0];
    let (view, _, _) = measured(control);
    let mut order: Vec<_> = view
        .replayed
        .state
        .attempts
        .iter()
        .map(|a| Some(a.id.clone()))
        .collect();
    order.push(None);
    assert_eq!(order.len(), 4);
    fn permute(control: &Value, order: &mut [Option<String>], at: usize) {
        if at == order.len() {
            check_sequence(control, order, &"\u{0001}".repeat(16384));
        } else {
            for next in at..order.len() {
                order.swap(at, next);
                permute(control, order, at + 1);
                order.swap(at, next);
            }
        }
    }
    permute(control, &mut order, 0); // All 24 orders, not just two endpoint cases.
    for reason in [
        "\"".repeat(16384),
        "\u{03bb}".repeat(8192),
        "\u{1f980}".repeat(4096),
    ] {
        check_sequence(control, &order, &reason);
        order.reverse();
        check_sequence(control, &order, &reason);
    }
}

#[test]
fn closed_schema_maxima_have_bounded_nodes_depth_bytes_and_reason_copies() {
    let mut v1 = execution::tests::enabled();
    execution::tests::enroll(&mut v1);
    execution::tests::prepare(&mut v1, "ffffffffffffffffffffffffffffffff", Value::Null, 1);
    let v2 = fixtures()[0].clone();
    for (control, revoke) in [(&v1, false), (&v2, false), (&v2, true)] {
        let (mut view, _, _) = measured(control);
        let attempt = (!revoke).then(|| view.replayed.state.attempts[0].id.clone());
        let reason = "\u{0001}".repeat(16384);
        let mut entry = escape(&mut control.clone(), attempt.as_deref(), &reason);
        let mut binding = get(&entry, "binding").unwrap().clone();
        // Maximal *schema* generation/revision widths. This is a checked reducer
        // entry, not a claim that the small fixture has 1023 preceding receipts.
        binding
            .set_field(
                "expected_execution_revision",
                string(format!(
                    "execution-control-v{}:{}:1023:{}",
                    view.schema,
                    view.authority,
                    "f".repeat(64),
                )),
            )
            .unwrap();
        if view.schema == 2 {
            execution::v2::binding(&binding, &mut Work::default()).unwrap();
        } else {
            execution::binding(&binding, &mut Work::default()).unwrap();
        }
        let result = execution::apply_successor(
            control,
            Some(&mut view),
            &binding,
            get(&entry, "effect").unwrap(),
            MAX_HISTORY,
            &mut Work::default(),
        )
        .unwrap();
        entry.set_field("binding", binding).unwrap();
        entry
            .set_field("generation", integer(MAX_HISTORY).unwrap())
            .unwrap();
        entry.set_field("result", result).unwrap();
        fn profile(value: &Value, depth: usize, key: &str) -> usize {
            assert!(depth <= 5);
            match value {
                Value::Object(object) => {
                    assert!(object.entries().len() <= 7);
                    object
                        .entries()
                        .iter()
                        .map(|(key, value)| profile(value, depth + 1, key))
                        .sum()
                }
                Value::String(s) if key == "reason" => {
                    assert_eq!(s.len(), 16384);
                    1
                }
                Value::String(s) => {
                    assert!(s.len() <= 128);
                    0
                }
                Value::Array(_) => panic!("escape entry must not embed arrays/snapshots"),
                _ => 0,
            }
        }
        assert_eq!(profile(&entry, 2, ""), 3);
        assert!(node_count(&entry, 2).unwrap() < ESCAPE_NODES);
        let bytes = entry.canonical().unwrap();
        let ascii = crate::output::ascii_json(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert!(ascii.len() + 8 < BOUND);
        assert!(bytes.len() + 8 < BOUND);
    }
}

#[test]
fn six_task_fresh_graph_fits_and_already_revoked_is_growth() {
    let mut control = fixtures()[1].clone();
    let (view, length, work) = measured(&control);
    assert_eq!(escape_count(&view).unwrap(), 7);
    admit(&control, &view, length, work).unwrap();
    let mut order = vec![None];
    order.extend(
        view.replayed
            .state
            .attempts
            .iter()
            .rev()
            .map(|a| Some(a.id.clone())),
    );
    check_sequence(&control, &order, &"\u{0001}".repeat(16384));
    escape(&mut control, None, "revoke");
    let (before, _, _) = measured(&control);
    assert_eq!(escape_count(&before).unwrap(), 6);
    escape(&mut control, None, "already revoked fresh decision");
    let (after, length, work) = measured(&control);
    assert_eq!(escape_count(&after).unwrap(), 6);
    assert_eq!(
        after.replayed.receipts.len(),
        before.replayed.receipts.len() + 1
    );
    admit(&control, &after, length, work).unwrap();
}

#[test]
fn coordinator_spends_review_reserve_once_but_protects_active_cancellations() {
    let output = Command::new("python3")
        .args([
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/execution/mutation/capacity/oracle.py"
            ),
            "--near-nodes",
        ])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let control = values::parse(&output.stdout, Profile::Authority).unwrap();
    let (view, length, work) = measured(&control);
    assert_eq!(escape_count(&view).unwrap(), 4);
    assert_eq!(
        admit(&control, &view, length, work).unwrap_err().code(),
        "unsupported_json_node_profile"
    );

    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("state/execution")).unwrap();
    let path = root.path().join("state/execution/control.json");
    let mut bytes = control.canonical().unwrap();
    bytes.push(b'\n');
    fs::write(&path, &bytes).unwrap();
    let lock_path = root.path().join("state/.workspace.lock");
    fs::write(&lock_path, b"").unwrap();
    let lock_metadata = fs::metadata(&lock_path).unwrap();
    // No coven.json, tasks, roster or backup exists: both decisions are source-free.
    let (_, old) = execution::read_typed(root.path()).unwrap().unwrap();
    assert_eq!(old.replayed.revision, view.replayed.revision);
    let graph = old.graph.as_ref().unwrap();
    let revision = text(get(graph, "current_graph_revision").unwrap()).unwrap();
    let digest = text(
        get(
            get(get(graph, "graphs").unwrap(), revision).unwrap(),
            "digest",
        )
        .unwrap(),
    )
    .unwrap();
    let items = [root.path().to_owned(), revision.into()];
    let first = execution::decide_arguments(
        &items,
        "changes-requested",
        "withdraw approval",
        digest,
        execution::WriteOptions {
            expected_execution_revision: &old.replayed.revision,
            expected_tasks_revision: None,
            request_id: "capacity-first-revoke",
            json: true,
        },
    )
    .unwrap();
    assert_eq!(first.exit_code, 0);
    let retained = fs::read(&path).unwrap();
    assert_ne!(retained, bytes);
    let (revoked_control, revoked) = execution::read_typed(root.path()).unwrap().unwrap();
    assert_eq!(escape_count(&revoked).unwrap(), 3);
    assert_eq!(
        revoked.replayed.receipts.len(),
        old.replayed.receipts.len() + 1
    );
    assert_eq!(
        revoked
            .replayed
            .state
            .attempts
            .iter()
            .filter(|a| a.active())
            .count(),
        3
    );
    let remaining_nodes = values::MAX_NODES - node_count(&revoked_control, 0).unwrap();
    assert!(remaining_nodes < 100);
    let last = get(&revoked_control, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        get(last, "request_id").unwrap().as_str(),
        Some("capacity-first-revoke")
    );

    // A second decision really fits the reader's finite parser and replay caps.
    // Its refusal must therefore be growth admission, not a stricter old reader.
    let mut second_candidate = revoked_control.clone();
    escape(&mut second_candidate, None, "already revoked");
    let (candidate_view, candidate_length, candidate_work) = measured(&second_candidate);
    assert_eq!(escape_count(&candidate_view).unwrap(), 3);
    assert_eq!(
        admit(
            &second_candidate,
            &candidate_view,
            candidate_length,
            candidate_work
        )
        .unwrap_err()
        .code(),
        "unsupported_json_node_profile"
    );
    let error = execution::decide_arguments(
        &items,
        "changes-requested",
        "already revoked",
        digest,
        execution::WriteOptions {
            expected_execution_revision: &revoked.replayed.revision,
            expected_tasks_revision: None,
            request_id: "capacity-second-revoke",
            json: true,
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), "unsupported_json_node_profile");
    assert_eq!(fs::read(&path).unwrap(), retained);
    let (_, final_view) = execution::read_typed(root.path()).unwrap().unwrap();
    assert_eq!(final_view.replayed.revision, revoked.replayed.revision);
    assert_eq!(escape_count(&final_view).unwrap(), 3);
    assert!(
        !final_view
            .replayed
            .requests
            .contains("capacity-second-revoke")
    );

    // The only files are the control and the pre-existing persistent lock.
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.path().join("state")).unwrap().count(), 2);
    assert_eq!(
        fs::read_dir(root.path().join("state/execution"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(fs::read(&lock_path).unwrap(), b"");
    let after_lock = fs::metadata(&lock_path).unwrap();
    assert_eq!(after_lock.created().ok(), lock_metadata.created().ok());
    assert_eq!(
        after_lock.modified().unwrap(),
        lock_metadata.modified().unwrap()
    );
}

#[test]
fn v1_maximum_cancel_entry_and_native_bytes_not_python_reserve() {
    let mut control = execution::tests::enabled();
    execution::tests::enroll(&mut control);
    let attempt = "ffffffffffffffffffffffffffffffff";
    execution::tests::prepare(&mut control, attempt, Value::Null, 1);
    check_sequence(
        &control,
        &[Some(attempt.to_owned())],
        &"\u{0001}".repeat(16384),
    );
    let (view, _, work) = measured(&control);
    for delta in [-1isize, 0, 1] {
        let length = (values::MAX_INPUT - BOUND)
            .checked_add_signed(delta)
            .unwrap();
        let outcome = admit(&control, &view, length, work);
        assert_eq!(outcome.is_ok(), delta <= 0);
        if delta > 0 {
            assert_eq!(outcome.unwrap_err().code(), CAPACITY);
        }
    }
    assert!(admit(&control, &view, usize::MAX, work).is_err());
}

#[test]
fn exact_history_work_and_prefix_boundaries() {
    let control = execution::tests::enabled();
    let (mut view, length, work) = measured(&control);
    // Arithmetic boundary tests isolate each gate using a real replayed view;
    // synthetic counters are not presented as valid execution histories.
    let receipt = view.replayed.receipts[0].clone();
    for count in [MAX_HISTORY - 1, MAX_HISTORY, MAX_HISTORY + 1] {
        view.replayed.receipts = vec![receipt.clone(); count];
        assert_eq!(
            admit(&control, &view, length, work).is_ok(),
            count <= MAX_HISTORY
        );
    }
    view.replayed.receipts = vec![receipt];
    for w in [MAX_WORK - 1, MAX_WORK, MAX_WORK + 1, usize::MAX] {
        assert_eq!(admit(&control, &view, length, w).is_ok(), w <= MAX_WORK);
    }
    let (mut view, _, work) = measured(&fixtures()[0]);
    let e = escape_count(&view).unwrap();
    let receipt = view.replayed.receipts[0].clone();
    for count in [MAX_HISTORY - e - 1, MAX_HISTORY - e, MAX_HISTORY - e + 1] {
        view.replayed.receipts = vec![receipt.clone(); count];
        // Keep enlarged bytes low to isolate history from prefix preflight.
        let outcome = admit(&control, &view, 1, work);
        if count + e > MAX_HISTORY {
            assert_eq!(outcome.unwrap_err().code(), CAPACITY);
        } else {
            assert_eq!(
                outcome.unwrap_err().code(),
                "unsupported_execution_work_profile"
            );
        }
    }
    let (view, length, _) = measured(&fixtures()[0]);
    let e = escape_count(&view).unwrap();
    let growth = e * BOUND;
    let increment = growth + e * (length + growth + 32 + 8 * BOUND);
    for delta in [-1isize, 0, 1] {
        let w = (MAX_WORK - increment).checked_add_signed(delta).unwrap();
        assert_eq!(admit(&fixtures()[0], &view, length, w).is_ok(), delta <= 0);
    }
    let (mut view, _, _) = measured(&control);
    let receipt = view.replayed.receipts[0].clone();
    view.replayed.receipts = vec![receipt; MAX_HISTORY];
    for delta in [-1isize, 0, 1] {
        let length = (MAX_WORK / MAX_HISTORY - 32)
            .checked_add_signed(delta)
            .unwrap();
        assert_eq!(admit(&control, &view, length, 0).is_ok(), delta <= 0);
    }
}

#[test]
fn exact_parser_node_headroom() {
    let mut base = execution::tests::enabled();
    execution::tests::enroll(&mut base);
    execution::tests::prepare(
        &mut base,
        "ffffffffffffffffffffffffffffffff",
        Value::Null,
        1,
    );
    let (view, _, _) = measured(&base);
    let reserved = escape_count(&view).unwrap() * ESCAPE_NODES;
    // Real valid candidates near the finite-node boundary, without large text.
    for delta in [-1isize, 0, 1] {
        let target = (values::MAX_NODES - reserved)
            .checked_add_signed(delta)
            .unwrap();
        let mut control = base.clone();
        control
            .set_field(
                "extensions",
                object(vec![("nodes", Value::Array(vec![Value::Array(vec![]); 4]))]).unwrap(),
            )
            .unwrap();
        let mut remaining = target - node_count(&control, 0).unwrap();
        let mut arrays = Vec::new();
        for _ in 0..4 {
            let n = remaining.min(values::MAX_ITEMS);
            arrays.push(Value::Array(vec![Value::Null; n]));
            remaining -= n;
        }
        control
            .set_field(
                "extensions",
                object(vec![("nodes", Value::Array(arrays))]).unwrap(),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        // V1 prefixes deliberately include final extensions. Rebind every
        // successor to that exact prefix; effects/results are unchanged.
        let mut history = get(&control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        for index in 1..history.len() {
            let mut prefix = control.clone();
            prefix
                .set_field("history", Value::Array(history[..index].to_vec()))
                .unwrap();
            prefix
                .set_field("generation", integer(index).unwrap())
                .unwrap();
            let revision = format!(
                "execution-control-v1:{}:{index}:{}",
                text(get(&control, "execution_authority_id").unwrap()).unwrap(),
                canonical::digest(&prefix.canonical().unwrap())
            );
            let mut binding = get(&history[index], "binding").unwrap().clone();
            binding
                .set_field("expected_execution_revision", string(revision))
                .unwrap();
            history[index].set_field("binding", binding).unwrap();
        }
        control.set_field("history", Value::Array(history)).unwrap();
        assert_eq!(node_count(&control, 0).unwrap(), target);
        let (view, length, work) = measured(&control);
        let outcome = admit(&control, &view, length, work);
        assert_eq!(outcome.is_ok(), delta <= 0);
        if delta > 0 {
            // Historical validation still succeeds; only new growth is refused.
            assert_eq!(outcome.unwrap_err().code(), "unsupported_json_node_profile");
        }
    }
}
