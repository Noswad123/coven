use super::*;
use std::fs;

#[cfg(unix)]
#[path = "native_first.rs"]
mod native_first;

fn fixture(active: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for directory in ["state", "logs", "checkpoints"] {
        fs::create_dir(root.path().join(directory)).unwrap();
    }
    fs::write(root.path().join("coven.json"), br#"{"name":"test","agents":["Worker_ID","other","___","---"],"display_names":{"other":"Display Name"}}"#).unwrap();
    fs::write(root.path().join("state/tasks.json"), b"[]\n").unwrap();
    if active {
        activate(root.path());
    }
    root
}
fn activate(root: &Path) {
    let (revision, _) = snapshot(root).unwrap();
    let guard = WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
    let _report = ledger::import_tasks(root, &revision, &guard).unwrap();
}
fn io() -> TaskIo {
    TaskIo {
        fixed_clock: true,
        ..TaskIo::default()
    }
}
fn fault(name: &'static str, fault: Fault) -> TaskIo {
    TaskIo {
        faults: [(name, fault)].into(),
        ..io()
    }
}
fn run_add(root: &Path, key: Option<&str>, io: &TaskIo) -> Result<Value> {
    mutate(
        root,
        Change::Add {
            title: " Test Title ",
            owner: Some(" worker_id "),
            notes: "note",
        },
        None,
        key,
        io,
    )
}
fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    field(value, key).unwrap()
}
fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    string(get(value, key)).unwrap()
}
fn status(value: &Value) -> &Value {
    value.as_object().unwrap().get("delivery").unwrap_or(value)
}
fn warnings(value: &Value) -> &[Value] {
    get(status(value), "warnings").as_array().unwrap()
}
fn count(io: &TaskIo, name: &str) -> usize {
    io.trace.borrow().iter().filter(|v| **v == name).count()
}
fn absent(io: &TaskIo, names: &[&str]) {
    for name in names {
        assert_eq!(count(io, name), 0, "{name}: {:?}", io.trace.borrow());
    }
}
fn raw(root: &Path) -> Vec<u8> {
    fs::read(root.join("state/tasks.json")).unwrap()
}

#[test]
fn snapshot_add_order_time_id_and_assign_preserve_extension_values() {
    let root = fixture(false);
    let io = io();
    let result = run_add(root.path(), None, &io).unwrap();
    assert_eq!(count(&io, "now"), 4); // two discarded, two fresh
    assert_eq!(count(&io, "uuid"), 1);
    assert_eq!(s(&result, "outcome"), "committed");
    let task = get(&result, "task");
    assert_eq!(s(task, "id"), "test-title");
    assert_eq!(s(task, "title"), " Test Title ");
    assert_eq!(s(task, "owner"), "Worker_ID");
    assert_eq!(s(task, "created_at"), "2026-01-02T03:04:05+00:00");
    assert_eq!(
        task.as_object()
            .unwrap()
            .entries()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>(),
        [
            "id",
            "title",
            "status",
            "owner",
            "notes",
            "created_at",
            "updated_at"
        ]
    );
    assert_eq!(s(&result, "mutation_id").len(), 36);
    assert_eq!(
        s(&result, "after_revision"),
        ledger::snapshot_revision(Some(&raw(root.path())))
    );
    let mut tasks = snapshot(root.path()).unwrap().1;
    tasks.array_mut().unwrap()[0]
        .set_field(
            "extension",
            values::parse(
                br#"[123456789012345678901234567890,-0.0,0.0,{"z":1,"a":2}]"#,
                Profile::LegacySnapshot,
            )
            .unwrap(),
        )
        .unwrap();
    fs::write(
        root.path().join("state/tasks.json"),
        tasks.storage().unwrap(),
    )
    .unwrap();
    let before = tasks.as_array().unwrap()[0].clone();
    let assigned = assign(root.path(), "test-title", "OTHER", None, None).unwrap();
    let mut expected = before;
    expected.set_field("owner", text("other")).unwrap();
    expected.set_field("status", text("assigned")).unwrap();
    expected
        .set_field(
            "updated_at",
            get(get(&assigned, "task"), "updated_at").clone(),
        )
        .unwrap();
    assert_eq!(
        expected.storage().unwrap(),
        get(&assigned, "task").storage().unwrap()
    );
    let second = run_add(root.path(), None, &io).unwrap();
    assert_eq!(s(get(&second, "task"), "id"), "test-title-2");
}

#[test]
fn agent_resolution_is_roster_alias_not_display_name_and_legacy_is_exact() {
    let root = fixture(false);
    let team = workspace::load(root.path()).unwrap().team;
    assert_eq!(resolve_agent(&team, "WORKER_ID").unwrap(), "Worker_ID");
    assert_eq!(resolve_agent(&team, "___").unwrap(), "___");
    assert_eq!(resolve_agent(&team, "---").unwrap(), "---");
    for name in ["Display Name", " ___ ", " --- ", "missing"] {
        assert!(resolve_agent(&team, name).is_err());
    }
    let error = mutate(
        root.path(),
        Change::Add {
            title: " ",
            owner: Some("missing"),
            notes: "",
        },
        None,
        None,
        &io(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "unknown_agent");
    assert_eq!(
        assign(root.path(), "missing-task", "missing-agent", None, None)
            .unwrap_err()
            .code(),
        "unknown_agent"
    );
}

#[test]
fn arguments_preserve_raw_title_ignore_assign_extras_and_reject_non_utf8() {
    let root = fixture(false);
    let result = add_arguments(
        &[root.path().into(), " Hello ".into(), " world ".into()],
        None,
        "",
        None,
        None,
    )
    .unwrap();
    assert_eq!(s(get(&result, "task"), "title"), " Hello   world ");
    assert!(add_arguments(&[root.path().into()], None, "", None, None).is_err());
    let assigned = assign_arguments(
        &[
            root.path().into(),
            "hello-world".into(),
            "other".into(),
            "ignored".into(),
        ],
        None,
        None,
    )
    .unwrap();
    assert_eq!(s(get(&assigned, "task"), "owner"), "other");
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let bad = PathBuf::from(OsString::from_vec(vec![0xff]));
        assert!(add_arguments(&[root.path().into(), bad.clone()], None, "", None, None).is_err());
        assert!(
            assign_arguments(
                &[root.path().into(), "hello-world".into(), bad.clone()],
                None,
                None
            )
            .is_err()
        );
        assert!(
            assign_arguments(
                &[
                    root.path().into(),
                    "hello-world".into(),
                    "other".into(),
                    bad
                ],
                None,
                None
            )
            .is_ok()
        );
    }
}

#[test]
fn deterministic_snapshot_failures_do_not_create_lock() {
    for case in [
        "title", "owner", "slug", "notes", "revision", "events", "messages", "pending", "request",
    ] {
        let root = fixture(false);
        let mut title = "Task";
        let mut owner = None;
        let notes = if case == "notes" {
            "x".repeat(values::MAX_STRING + 1)
        } else {
            String::new()
        };
        if case == "title" {
            title = " ";
        }
        if case == "slug" {
            title = "___";
        }
        if case == "owner" {
            owner = Some("missing");
        }
        if matches!(case, "events" | "messages") {
            fs::write(root.path().join(format!("logs/{case}.jsonl")), b"bad\n").unwrap();
        }
        if case == "pending" || case == "request" {
            fs::create_dir(root.path().join(ledger::DIRECTORY)).unwrap();
        }
        let expected = (case == "revision").then_some("invalid");
        let key = (case == "request").then_some("invalid key");
        let io = io();
        let error = mutate(
            root.path(),
            Change::Add {
                title,
                owner,
                notes: &notes,
            },
            expected,
            key,
            &io,
        )
        .unwrap_err();
        if case == "request" {
            assert_eq!(error.code(), "snapshot_request_id_unsupported");
        }
        assert!(
            !root.path().join("state/.workspace.lock").exists(),
            "{case}"
        );
        absent(&io, &["replace", "commit", "event", "dashboard"]);
        assert_eq!(raw(root.path()), b"[]\n");
    }
}

#[test]
fn raw_binding_precedes_backend_discovery_and_empty_owner_only_binding_normalizes() {
    let missing = Path::new("/nonexistent-coven-task-fixture");
    assert_eq!(
        add(missing, " ", None, "", None, Some("key"))
            .unwrap_err()
            .code(),
        "invalid_task_title"
    );
    let root = fixture(true);
    let first = add(root.path(), "Task", Some(""), "", None, Some("key")).unwrap();
    let replay = add(root.path(), "Task", None, "", None, Some("key")).unwrap();
    assert_eq!(
        get(&first, "result").compact().unwrap(),
        get(&replay, "result").compact().unwrap()
    );
    assert!(matches!(
        get(status(&replay), "replayed"),
        Value::Bool(true)
    ));
}

#[test]
fn binding_semantic_errors_have_service_context_without_changing_constructor_order() {
    for (change, expected_code) in [
        (
            Change::Add {
                title: "",
                owner: None,
                notes: "",
            },
            "invalid_task_title",
        ),
        (
            Change::Add {
                title: "\u{1c} \t",
                owner: Some("missing"),
                notes: "",
            },
            "invalid_task_title",
        ),
        (
            Change::Add {
                title: "Task",
                owner: Some(" \t"),
                notes: "",
            },
            "invalid_task_owner",
        ),
        (
            Change::Assign {
                task_id: "",
                agent: "",
            },
            "invalid_task_id",
        ),
        (
            Change::Assign {
                task_id: " \u{1c}",
                agent: "missing",
            },
            "invalid_task_id",
        ),
        (
            Change::Assign {
                task_id: "missing",
                agent: "",
            },
            "invalid_task_agent",
        ),
        (
            Change::Assign {
                task_id: "missing",
                agent: " \u{1c}",
            },
            "invalid_task_agent",
        ),
    ] {
        assert_eq!(change.binding(None).unwrap_err().code(), expected_code);
        // Revision validation must still win over any blank selector/title.
        assert_eq!(
            change.binding(Some("bad-revision")).unwrap_err().code(),
            "invalid_task_revision"
        );
        let io = io();
        assert_eq!(
            mutate(
                Path::new("/nonexistent-coven-task-fixture"),
                change,
                None,
                Some("key"),
                &io
            )
            .unwrap_err()
            .code(),
            expected_code
        );
        assert!(io.trace.borrow().is_empty());
    }
    assert_eq!(
        ledger::RequestBinding::add("", None, "", None)
            .unwrap_err()
            .code(),
        "invalid_ledger_text"
    );
    assert_eq!(
        ledger::RequestBinding::assign("Task", "", None)
            .unwrap_err()
            .code(),
        "invalid_ledger_text"
    );
    assert_eq!(
        ledger::RequestBinding::add("Task", None, "", Some("bad-revision"))
            .unwrap_err()
            .code(),
        "invalid_handoff_token"
    );
}

#[test]
fn binding_context_preserves_resource_errors_and_raw_identity() {
    let oversized = "x".repeat(values::MAX_STRING + 1);
    for change in [
        Change::Add {
            title: &oversized,
            owner: None,
            notes: "",
        },
        Change::Add {
            title: "Task",
            owner: Some(&oversized),
            notes: "",
        },
        Change::Add {
            title: "Task",
            owner: None,
            notes: &oversized,
        },
        Change::Assign {
            task_id: &oversized,
            agent: "other",
        },
        Change::Assign {
            task_id: "task",
            agent: &oversized,
        },
    ] {
        assert_eq!(
            change.binding(None).unwrap_err().code(),
            "unsupported_json_string_profile"
        );
        assert_eq!(
            change.binding(Some("bad-revision")).unwrap_err().code(),
            "invalid_task_revision"
        );
    }
    // Semantic validation precedes canonical resource admission in the core.
    assert_eq!(
        Change::Add {
            title: &oversized,
            owner: Some(" "),
            notes: ""
        }
        .binding(None)
        .unwrap_err()
        .code(),
        "invalid_task_owner"
    );
    let expected = "tasks-missing:v1";
    assert_eq!(
        Change::Add {
            title: " Task ",
            owner: Some(" Worker_ID "),
            notes: " notes "
        }
        .binding(Some(expected))
        .unwrap(),
        ledger::RequestBinding::add(" Task ", Some(" Worker_ID "), " notes ", Some(expected))
            .unwrap(),
    );
    assert_eq!(
        Change::Assign {
            task_id: " Task ",
            agent: " Worker_ID "
        }
        .binding(Some(expected))
        .unwrap(),
        ledger::RequestBinding::assign(" Task ", " Worker_ID ", Some(expected)).unwrap(),
    );
}

#[test]
fn authority_admission_precedes_config_errors_like_python_but_still_requires_config() {
    for directory_config in [false, true] {
        for (case, code, python_message) in [
            (
                "snapshot-key",
                "snapshot_request_id_unsupported",
                "snapshot-v1 does not provide",
            ),
            ("pending", "task_import_pending", "Task import is pending"),
            (
                "active-no-key",
                "ledger_request_id_required",
                "requires --request-id",
            ),
            ("active-bad-key", "invalid_request_key", "request key"),
            ("corrupt", "invalid_compat_json", "invalid strict JSON"),
            ("snapshot-revision", "invalid_snapshot_revision", "revision"),
        ] {
            let root = fixture(case.starts_with("active"));
            fs::remove_file(root.path().join("coven.json")).unwrap();
            if directory_config {
                fs::create_dir(root.path().join("coven.json")).unwrap();
            }
            if case == "pending" {
                fs::create_dir(root.path().join(ledger::DIRECTORY)).unwrap();
            }
            if case == "corrupt" {
                fs::write(root.path().join(ledger::MARKER), b"bad").unwrap();
            }
            let key = match case {
                "snapshot-key" => Some("key"),
                "active-bad-key" => Some("bad key"),
                _ => None,
            };
            let expected = (case == "snapshot-revision").then_some("bad-revision");
            let before = raw(root.path());
            let io = io();
            let error = mutate(
                root.path(),
                Change::Add {
                    title: "Task",
                    owner: None,
                    notes: "",
                },
                expected,
                key,
                &io,
            )
            .unwrap_err();
            assert_eq!(
                error.code(),
                code,
                "{case}, directory config: {directory_config}"
            );
            let script = r#"
import sys
from pathlib import Path
from runner import coven
try:
    coven.add_task(Path(sys.argv[1]), 'Task', expected_revision=sys.argv[2] or None, request_id=sys.argv[3] or None)
except (Exception, SystemExit) as error:
    print(type(error).__name__ + ': ' + str(error))
else:
    raise AssertionError('invalid request unexpectedly accepted')
"#;
            let output = std::process::Command::new("python3")
                .args(["-c", script])
                .arg(root.path())
                .arg(expected.unwrap_or(""))
                .arg(key.unwrap_or(""))
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let diagnostic = String::from_utf8(output.stdout).unwrap();
            assert!(diagnostic.contains(python_message), "{case}: {diagnostic}");
            assert_eq!(raw(root.path()), before);
            absent(
                &io,
                &[
                    "before_lock",
                    "now",
                    "commit",
                    "replace",
                    "event",
                    "dashboard",
                ],
            );
        }
    }
    for active in [false, true] {
        let root = fixture(active);
        fs::remove_file(root.path().join("coven.json")).unwrap();
        assert_eq!(
            add(root.path(), "Task", None, "", None, active.then_some("key"))
                .unwrap_err()
                .code(),
            "invalid_checkpoint_workspace"
        );
        if active {
            let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO)
                .err()
                .unwrap();
            assert_eq!(guard.code(), "invalid_handoff_workspace");
        } else {
            assert!(!root.path().join("state/.workspace.lock").exists());
        }
        assert_eq!(raw(root.path()), b"[]\n");
    }
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        add(&root.path().join("missing"), "Task", None, "", None, None)
            .unwrap_err()
            .code(),
        "invalid_task_workspace"
    );
    let file = root.path().join("not-a-directory");
    fs::write(&file, b"file").unwrap();
    assert_eq!(
        add(&file, "Task", None, "", None, None).unwrap_err().code(),
        "invalid_task_workspace"
    );
    assert_eq!(
        add(&file, "", None, "", None, Some("key"))
            .unwrap_err()
            .code(),
        "invalid_task_title"
    );
}

#[test]
fn snapshot_replace_errors_are_hard_but_observed_uncertainty_skips_secondaries() {
    for uncertain in [false, true] {
        let root = fixture(false);
        let io = fault(
            "replace",
            if uncertain {
                Fault::Uncertain
            } else {
                Fault::Error
            },
        );
        let result = run_add(root.path(), None, &io);
        if uncertain {
            let result = result.unwrap();
            assert_eq!(s(&result, "outcome"), "durability_uncertain");
            assert_eq!(render_result(&result, true, "add").unwrap().exit_code, 2);
            assert_ne!(raw(root.path()), b"[]\n");
        } else {
            assert!(result.is_err());
            assert_eq!(raw(root.path()), b"[]\n");
        }
        absent(&io, &["event", "dashboard"]);
    }
}

#[test]
fn secondary_failures_are_independent_and_do_not_change_committed_outcome() {
    for active in [false, true] {
        let root = fixture(active);
        let io = TaskIo {
            faults: [("event", Fault::Error), ("dashboard", Fault::Error)].into(),
            ..io()
        };
        let result = run_add(root.path(), active.then_some("key"), &io).unwrap();
        assert_eq!(s(status(&result), "outcome"), "committed");
        assert_eq!(warnings(&result).len(), 2);
        assert!(
            string(&warnings(&result)[0])
                .unwrap()
                .contains("history incomplete")
        );
        assert!(
            string(&warnings(&result)[1])
                .unwrap()
                .contains("dashboard stale")
        );
        let trace = io.trace.borrow();
        assert!(
            trace.iter().position(|v| *v == "event") < trace.iter().position(|v| *v == "dashboard")
        );
        assert_eq!(render_result(&result, true, "add").unwrap().exit_code, 0);
    }
}

#[test]
fn projection_failure_does_not_suppress_real_event_or_dashboard_publication() {
    let root = fixture(true);
    fs::write(root.path().join("dashboard.md"), b"old dashboard\n").unwrap();
    let before = raw(root.path());
    let io = fault("projection", Fault::Error);
    let result = run_add(root.path(), Some("key"), &io).unwrap();
    assert_eq!(s(status(&result), "outcome"), "committed");
    assert_eq!(render_result(&result, true, "add").unwrap().exit_code, 0);
    assert_eq!(raw(root.path()), before);
    assert_eq!(warnings(&result).len(), 2);
    assert_eq!(
        string(&warnings(&result)[0]).unwrap(),
        "Ledger task committed; task projection stale: injected_task_io_failure"
    );
    assert_eq!(string(&warnings(&result)[1]).unwrap(), PROJECTION_WARNING);
    let event = values::parse(
        &fs::read(root.path().join("logs/events.jsonl")).unwrap(),
        Profile::LegacySnapshot,
    )
    .unwrap();
    assert_eq!(s(&event, "type"), "task.created");
    assert_eq!(s(get(&event, "task"), "id"), "test-title");
    assert_eq!(
        s(&event, "mutation_id"),
        s(get(&result, "result"), "mutation_id")
    );
    let dashboard = fs::read_to_string(root.path().join("dashboard.md")).unwrap();
    assert!(dashboard.starts_with("# Coven Dashboard: test\n"));
    assert!(
        dashboard.contains("| `test-title` | assigned | Worker_ID |  Test Title  |"),
        "{dashboard}"
    );
    let view = ledger::read(root.path()).unwrap().unwrap();
    let frozen = view
        .lookup(
            &ledger::RequestKey::parse("key").unwrap(),
            &ledger::RequestBinding::add(" Test Title ", Some(" worker_id "), "note", None)
                .unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        frozen.value().canonical().unwrap(),
        get(&result, "result").canonical().unwrap()
    );
}

#[test]
fn real_postcommit_event_failure_preserves_partial_evidence_and_publishes_dashboard() {
    for active in [false, true] {
        let root = fixture(active);
        fs::write(root.path().join("dashboard.md"), b"old dashboard\n").unwrap();
        let io = TaskIo {
            event_hook: Some(|root| {
                fs::write(root.join("logs/events.jsonl"), b"{\"partial\":").unwrap()
            }),
            ..io()
        };
        let result = run_add(root.path(), active.then_some("key"), &io).unwrap();
        assert_eq!(s(status(&result), "outcome"), "committed");
        assert_eq!(warnings(&result).len(), 1);
        assert_eq!(
            string(&warnings(&result)[0]).unwrap(),
            if active {
                "Ledger task committed; history incomplete: event_log_unterminated_line"
            } else {
                "Task committed; history incomplete: event_log_unterminated_line"
            }
        );
        assert_eq!(
            fs::read(root.path().join("logs/events.jsonl")).unwrap(),
            b"{\"partial\":"
        );
        let dashboard = fs::read_to_string(root.path().join("dashboard.md")).unwrap();
        assert!(
            dashboard.contains("| `test-title` | assigned | Worker_ID |  Test Title  |"),
            "{dashboard}"
        );
        let tasks = if active {
            ledger::read(root.path()).unwrap().unwrap().tasks().clone()
        } else {
            snapshot(root.path()).unwrap().1
        };
        assert_eq!(s(&tasks.as_array().unwrap()[0], "id"), "test-title");
        assert_eq!(render_result(&result, true, "add").unwrap().exit_code, 0);
    }
}

#[test]
fn fresh_snapshot_reloads_source_team_logs_and_cas_under_guard() {
    let root = fixture(false);
    let insertion_io = TaskIo {
        hook: Some(|root| {
            fs::write(
                root.join("state/tasks.json"),
                br#"[{"id":"test-title","title":"existing","status":"pending"}]"#,
            )
            .unwrap()
        }),
        ..io()
    };
    let result = run_add(root.path(), None, &insertion_io).unwrap();
    assert_eq!(s(get(&result, "task"), "id"), "test-title-2");
    let (_, tasks) = snapshot(root.path()).unwrap();
    let tasks = tasks.as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(s(&tasks[0], "id"), "test-title");
    assert_eq!(s(&tasks[0], "title"), "existing");
    assert_eq!(
        tasks[1].storage().unwrap(),
        get(&result, "task").storage().unwrap()
    );
    for (hook, code) in [
        (
            (|root: &Path| {
                fs::write(
                    root.join("coven.json"),
                    br#"{"name":"test","agents":["other"]}"#,
                )
                .unwrap()
            }) as fn(&Path),
            "unknown_agent",
        ),
        (
            |root| fs::write(root.join("logs/messages.jsonl"), b"bad\n").unwrap(),
            "invalid_jsonl_record",
        ),
    ] {
        let root = fixture(false);
        let io = TaskIo {
            hook: Some(hook),
            ..io()
        };
        let before = raw(root.path());
        assert_eq!(run_add(root.path(), None, &io).unwrap_err().code(), code);
        assert_eq!(raw(root.path()), before);
        assert!(!root.path().join(ledger::MARKER).exists());
        assert!(!root.path().join("logs/events.jsonl").exists());
        assert!(!root.path().join("dashboard.md").exists());
        absent(&io, &["replace", "commit", "event", "dashboard"]);
    }
    let root = fixture(false);
    let before = snapshot(root.path()).unwrap().0;
    let io = TaskIo {
        hook: Some(|root| fs::write(root.join("state/tasks.json"), b"[] \n").unwrap()),
        ..io()
    };
    assert_eq!(
        mutate(
            root.path(),
            Change::Add {
                title: "Task",
                owner: None,
                notes: ""
            },
            Some(&before),
            None,
            &io
        )
        .unwrap_err()
        .code(),
        "task_revision_conflict"
    );
    absent(&io, &["replace"]);
}

#[test]
fn snapshot_reselection_never_overwrites_new_ledger_or_pending_import() {
    for active in [false, true] {
        let root = fixture(false);
        let io = TaskIo {
            hook: Some(if active {
                activate
            } else {
                |root| fs::create_dir(root.join(ledger::DIRECTORY)).unwrap()
            }),
            ..io()
        };
        let error = run_add(root.path(), None, &io).unwrap_err();
        assert_eq!(
            error.code(),
            if active {
                "ledger_request_id_required"
            } else {
                "task_import_pending"
            }
        );
        assert_eq!(raw(root.path()), b"[]\n");
        absent(&io, &["replace", "commit", "event", "dashboard"]);
    }
}

#[test]
fn known_receipt_precedes_current_selectors_cas_projection_and_execution() {
    let root = fixture(true);
    let first = run_add(root.path(), Some("key"), &io()).unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["other"]}"#,
    )
    .unwrap();
    fs::write(root.path().join("state/tasks.json"), b"bad projection").unwrap();
    fs::create_dir(root.path().join("state/execution")).unwrap();
    fs::write(
        root.path().join("state/execution/control.json"),
        b"bad execution",
    )
    .unwrap();
    let io = io();
    let replay = run_add(root.path(), Some("key"), &io).unwrap();
    assert_eq!(
        get(&first, "result").canonical().unwrap(),
        get(&replay, "result").canonical().unwrap()
    );
    assert_eq!(get(status(&replay), "replayed").compact().unwrap(), b"true");
    assert_eq!(string(&warnings(&replay)[0]).unwrap(), PROJECTION_WARNING);
    absent(
        &io,
        &[
            "now",
            "uuid",
            "health",
            "commit",
            "projection",
            "event",
            "dashboard",
        ],
    );
    assert_eq!(
        assign(root.path(), "test-title", "other", None, Some("key"))
            .unwrap_err()
            .code(),
        "request_key_binding_conflict"
    );
}

#[test]
fn lookup_conflict_precedes_confirmation_and_hard_confirmation_errors_stay_hard() {
    let root = fixture(true);
    run_add(root.path(), Some("key"), &io()).unwrap();
    let io = fault("confirm", Fault::Error);
    assert_eq!(
        mutate(
            root.path(),
            Change::Assign {
                task_id: "test-title",
                agent: "other"
            },
            None,
            Some("key"),
            &io
        )
        .unwrap_err()
        .code(),
        "request_key_binding_conflict"
    );
    absent(&io, &["confirm"]);
    assert_eq!(
        run_add(root.path(), Some("key"), &io).unwrap_err().code(),
        "injected_task_io_failure"
    );
    absent(&io, &["now", "commit", "projection", "event", "dashboard"]);
}

#[test]
fn uncertain_confirmation_hit_returns_frozen_receipt_miss_never_drafts() {
    for hit in [false, true] {
        let root = fixture(true);
        if hit {
            run_add(root.path(), Some("key"), &io()).unwrap();
        }
        let io = fault("confirm", Fault::Uncertain);
        let result = run_add(root.path(), Some("key"), &io);
        if hit {
            let result = result.unwrap();
            assert_eq!(s(status(&result), "outcome"), "durability_uncertain");
            assert!(matches!(
                get(status(&result), "replayed"),
                Value::Bool(true)
            ));
            assert_eq!(render_result(&result, false, "add").unwrap().exit_code, 2);
        } else {
            assert_eq!(
                result.unwrap_err().code(),
                "ledger_synchronization_uncertain"
            );
        }
        absent(
            &io,
            &["now", "uuid", "commit", "projection", "event", "dashboard"],
        );
    }
}

#[test]
fn new_uncertain_frame_requires_fresh_matching_receipt_and_skips_all_delivery() {
    for mode in [
        "hit",
        "read-error",
        "absent-authority",
        "removed-frame",
        "corrupt-authority",
        "conflict",
    ] {
        let root = fixture(true);
        let mut io = fault("commit", Fault::Uncertain);
        if mode == "read-error" {
            io.faults.insert("uncertain_read", Fault::Error);
        }
        if mode == "absent-authority" {
            io.uncertain_hook = Some(|root| fs::remove_file(root.join(ledger::MARKER)).unwrap());
        }
        if mode == "conflict" {
            io.uncertain_hook = Some(|root| {
                let epoch = fs::read_dir(root.join(ledger::DIRECTORY))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                let path = epoch.join("frames/00000000000000000001.frame");
                let envelope =
                    values::parse(&fs::read(&path).unwrap(), Profile::Authority).unwrap();
                let mut payload = get(&envelope, "payload").clone();
                let mut binding = get(&payload, "binding").clone();
                let mut args = get(&binding, "args").clone();
                args.set_field("owner", text("Worker_ID")).unwrap();
                binding.set_field("args", args).unwrap();
                payload.set_field("binding", binding).unwrap();
                let bytes = payload.canonical().unwrap();
                let envelope = object(vec![
                    ("version", Value::integer("1").unwrap()),
                    ("length", Value::integer(&bytes.len().to_string()).unwrap()),
                    ("sha256", text(crate::canonical::digest(&bytes))),
                    ("payload", payload),
                ])
                .unwrap();
                let mut bytes = envelope.canonical().unwrap();
                bytes.push(b'\n');
                fs::write(path, bytes).unwrap();
            });
        }
        if mode == "removed-frame" {
            io.uncertain_hook = Some(|root| {
                let epoch = fs::read_dir(root.join(ledger::DIRECTORY))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                fs::remove_file(epoch.join("frames/00000000000000000001.frame")).unwrap();
            });
        }
        if mode == "corrupt-authority" {
            io.uncertain_hook = Some(|root| fs::write(root.join(ledger::MARKER), b"bad").unwrap());
        }
        let result = run_add(root.path(), Some("key"), &io);
        if mode == "hit" {
            let result = result.unwrap();
            assert_eq!(s(get(&result, "result"), "outcome"), "committed");
            assert_eq!(s(status(&result), "outcome"), "durability_uncertain");
            assert!(matches!(
                get(status(&result), "replayed"),
                Value::Bool(false)
            ));
            assert_eq!(warnings(&result).len(), 2);
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.code(),
                match mode {
                    "read-error" => "injected_task_io_failure",
                    "absent-authority" | "removed-frame" => "ledger_uncertain_receipt_missing",
                    "corrupt-authority" => "invalid_compat_json",
                    "conflict" => "request_key_binding_conflict",
                    _ => unreachable!(),
                },
                "{mode}"
            );
        }
        assert_eq!(
            raw(root.path()),
            b"[]\n",
            "never publish the cached proposal: {mode}"
        );
        assert!(!root.path().join("logs/events.jsonl").exists());
        assert!(!root.path().join("dashboard.md").exists());
        assert_eq!(count(&io, "uncertain_read"), 1);
        absent(&io, &["projection", "event", "dashboard"]);
    }
}

#[test]
fn wrapper_projection_before_cas_is_deliberately_not_low_level_writer_order() {
    let root = fixture(true);
    let before = ledger::read(root.path()).unwrap().unwrap();
    run_add(root.path(), Some("first"), &io()).unwrap();
    let current = ledger::read(root.path()).unwrap().unwrap();
    fs::remove_file(root.path().join("state/tasks.json")).unwrap();
    assert_eq!(
        add(
            root.path(),
            "Next",
            None,
            "",
            Some(before.revision()),
            Some("next")
        )
        .unwrap_err()
        .code(),
        "task_projection_not_current"
    );
    let key = ledger::RequestKey::parse("next").unwrap();
    let binding = ledger::RequestBinding::add("Next", None, "", Some(before.revision())).unwrap();
    let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
    let error = ledger::commit_task(root.path(), &current, &key, &binding, Value::Null, &guard)
        .err()
        .unwrap();
    assert_eq!(error.code(), "ledger_expected_revision_mismatch");
}

#[test]
fn ledger_secondary_order_and_trailing_health_failure_keep_known_commit() {
    let root = fixture(true);
    let io = TaskIo {
        faults: [
            ("projection", Fault::Error),
            ("event", Fault::Error),
            ("dashboard", Fault::Error),
            ("final_health", Fault::Error),
        ]
        .into(),
        ..io()
    };
    let result = run_add(root.path(), Some("key"), &io).unwrap();
    assert_eq!(s(status(&result), "outcome"), "committed");
    assert_eq!(warnings(&result).len(), 5);
    assert_eq!(
        s(get(status(&result), "projection"), "status"),
        "unreadable"
    );
    let trace = io.trace.borrow();
    let secondary: Vec<_> = trace
        .iter()
        .copied()
        .filter(|v| ["projection", "event", "dashboard", "final_health"].contains(v))
        .collect();
    assert_eq!(
        secondary,
        ["projection", "event", "dashboard", "final_health"]
    );
    assert_eq!(render_result(&result, true, "add").unwrap().exit_code, 0);
}

#[test]
fn ledger_does_not_validate_log_contents_before_authority_commit() {
    let root = fixture(true);
    fs::write(root.path().join("logs/events.jsonl"), b"bad\n").unwrap();
    let result = run_add(root.path(), Some("key"), &io()).unwrap();
    assert_eq!(s(status(&result), "outcome"), "committed");
    assert!(
        warnings(&result)
            .iter()
            .any(|v| string(v).unwrap().contains("history incomplete"))
    );
    assert!(
        ledger::read(root.path())
            .unwrap()
            .unwrap()
            .lookup(
                &ledger::RequestKey::parse("key").unwrap(),
                &ledger::RequestBinding::add(" Test Title ", Some(" worker_id "), "note", None)
                    .unwrap()
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn emitted_ledger_event_uses_original_draft_order_and_frame_metadata() {
    let root = fixture(true);
    let result = run_add(root.path(), Some("key"), &io()).unwrap();
    let event = fs::read(root.path().join("logs/events.jsonl")).unwrap();
    let event = values::parse(event.strip_suffix(b"\n").unwrap(), Profile::LegacySnapshot).unwrap();
    // append_event flattens the effect fields, just like Python kwargs.
    assert_eq!(
        get(&event, "task").as_object().unwrap().entries()[0].0,
        "id"
    );
    assert_eq!(
        get(&event, "task").as_object().unwrap().entries()[1].0,
        "title"
    );
    assert_eq!(
        s(&event, "mutation_id"),
        s(get(&result, "result"), "mutation_id")
    );
    let view = ledger::read(root.path()).unwrap().unwrap();
    for (key, value) in ledger::event_fields(&view)
        .unwrap()
        .as_object()
        .unwrap()
        .entries()
    {
        assert_eq!(
            get(&event, key).compact().unwrap(),
            value.compact().unwrap()
        );
    }
}

#[test]
fn presentation_preserves_values_in_json_but_escapes_human_controls() {
    let root = fixture(false);
    let result = add(root.path(), "A\n\u{1b}\u{e9}", None, "", None, None).unwrap();
    let human = render_result(&result, false, "add").unwrap();
    assert_eq!(human.stdout, "Created a: A\\n\\u001b\\u00e9");
    assert_eq!(human.exit_code, 0);
    let json = render_result(&result, true, "add").unwrap();
    assert!(json.stdout.is_ascii());
    assert!(json.warnings.is_empty());
    assert_eq!(
        values::parse(json.stdout.as_bytes(), Profile::LegacySnapshot)
            .unwrap()
            .canonical()
            .unwrap(),
        result.canonical().unwrap()
    );
    let root = fixture(true);
    let result = run_add(root.path(), Some("key"), &io()).unwrap();
    assert_eq!(
        render_result(&result, false, "add").unwrap().stdout,
        "Committed ledger task test-title (sequence 1)"
    );
    let result = run_add(root.path(), Some("key"), &io()).unwrap();
    assert_eq!(
        render_result(&result, false, "add").unwrap().stdout,
        "Replayed committed ledger task test-title (sequence 1)"
    );
}

#[test]
fn human_escaping_uses_exact_json_spelling_for_titles_and_warnings() {
    let source = "\u{08}\u{0c}\n\r\t\u{1f}\u{7f}\u{3bb}\u{1f600} '\"\\";
    let escaped = "\\b\\f\\n\\r\\t\\u001f\\u007f\\u03bb\\ud83d\\ude00 '\"\\";
    assert_eq!(human(source), escaped);
    assert!(human(source).is_ascii());
    let root = fixture(false);
    let mut result = add(root.path(), &format!("Task {source}"), None, "", None, None).unwrap();
    result
        .set_field("warnings", warning_values(vec![source.to_owned()]))
        .unwrap();
    let rendered = render_result(&result, false, "add").unwrap();
    assert_eq!(rendered.stdout, format!("Created task: Task {escaped}"));
    assert_eq!(rendered.warnings, [format!("[WARN] {escaped}")]);
    let json = render_result(&result, true, "add").unwrap();
    let parsed = values::parse(json.stdout.as_bytes(), Profile::LegacySnapshot).unwrap();
    assert_eq!(parsed.canonical().unwrap(), result.canonical().unwrap());
    assert_eq!(s(get(&parsed, "task"), "title"), format!("Task {source}"));
    assert_eq!(
        get(&parsed, "warnings").as_array().unwrap()[0].as_str(),
        Some(source)
    );
}

#[test]
fn actual_storage_and_event_payload_must_remain_readable_before_replace() {
    let root = fixture(false);
    let notes = "x".repeat(values::MAX_STRING);
    let mut tasks = Vec::new();
    for index in 0..8 {
        tasks.push(
            object(vec![
                ("id", text(format!("task-{index}"))),
                ("title", text("old")),
                ("status", text("pending")),
                ("notes", text(&notes)),
            ])
            .unwrap(),
        );
    }
    // This SDK value encodes beyond the reader's 8 MiB ceiling even though the
    // low-level replacement transport accepts it.
    let value = Value::Array(tasks);
    assert_eq!(
        encoded_tasks(&value).unwrap_err().code(),
        "unsupported_task_snapshot_size"
    );
    let before = raw(root.path());
    let io = io();
    let result = mutate(
        root.path(),
        Change::Add {
            title: "Task",
            owner: None,
            notes: &"x".repeat(values::MAX_STRING + 1),
        },
        None,
        None,
        &io,
    );
    assert!(result.is_err());
    assert_eq!(raw(root.path()), before);
    absent(&io, &["replace"]);
}

#[test]
fn response_reserve_proves_worst_case_delivery_envelope_is_under_eight_kib() {
    // At most three independent secondaries, one failed health diagnostic, and
    // the static projection warning. All dynamic warning codes are <=128 ASCII.
    let code = "x".repeat(128);
    let mut maximal_warnings = [
        "Ledger task committed; task projection stale",
        "Ledger task committed; history incomplete",
        "Ledger task committed; dashboard stale",
    ]
    .map(|label| format!("{label}: {code}"))
    .to_vec();
    maximal_warnings.push(format!("Task projection health unavailable: {code}"));
    maximal_warnings.push(PROJECTION_WARNING.to_owned());
    let mut envelope = object(vec![
        ("result", Value::Null),
        (
            "delivery",
            object(vec![
                ("outcome", text("durability_uncertain")),
                ("replayed", Value::Bool(false)),
                ("warnings", warning_values(maximal_warnings)),
                ("current_revision", text("r".repeat(256))),
                (
                    "projection",
                    bounded_health(
                        object(vec![
                            ("status", text("unreadable")),
                            ("expected_revision", text("x".repeat(128))),
                            ("observed_revision", text("x".repeat(128))),
                            ("error", text("x".repeat(128))),
                        ])
                        .unwrap(),
                    )
                    .unwrap(),
                ),
            ])
            .unwrap(),
        ),
    ])
    .unwrap();
    let compact = envelope.compact().unwrap();
    assert!(compact.len() < task_admission::RESPONSE_RESERVE);
    assert!(
        crate::output::ascii_json(std::str::from_utf8(&compact).unwrap())
            .unwrap()
            .len()
            < task_admission::RESPONSE_RESERVE
    );
    drop(compact);

    // Put a single receipt exactly on both admitted output limits, using only
    // 1 MiB strings in a legal task extension. Keep only one large Value alive.
    let root = fixture(true);
    let committed = run_add(root.path(), Some("key"), &io()).unwrap();
    let mut receipt = get(&committed, "result").clone();
    let mut task = get(&receipt, "task").clone();
    task.set_field("extension", Value::Array(Vec::new()))
        .unwrap();
    receipt.set_field("task", task.clone()).unwrap();
    let target = values::MAX_OUTPUT - task_admission::RESPONSE_RESERVE;
    let count = 32;
    let overhead = receipt.compact().unwrap().len() + 3 * count - 1;
    let mut remaining = target - overhead;
    let mut parts = Vec::new();
    for _ in 0..count {
        let size = remaining.min(values::MAX_STRING);
        parts.push(text("x".repeat(size)));
        remaining -= size;
    }
    assert_eq!(remaining, 0);
    task.set_field("extension", Value::Array(parts)).unwrap();
    receipt.set_field("task", task).unwrap();
    assert_eq!(receipt.compact().unwrap().len(), target);
    task_admission::response(&receipt).unwrap();
    receipt.set_field("outcome", text("committed!")).unwrap();
    assert_eq!(
        task_admission::response(&receipt).unwrap_err().code(),
        "unsupported_task_response_profile"
    );
    receipt.set_field("outcome", text("committed")).unwrap();
    envelope.set_field("result", receipt).unwrap();
    // Both committed and uncertain deliveries must remain renderable after
    // adding all five warnings and maximally bounded projection metadata.
    for outcome in ["durability_uncertain", "committed"] {
        let mut metadata = get(&envelope, "delivery").clone();
        metadata.set_field("outcome", text(outcome)).unwrap();
        envelope.set_field("delivery", metadata).unwrap();
        let output = render_result(&envelope, true, "add").unwrap();
        assert!(output.stdout.len() <= values::MAX_OUTPUT);
        assert!(output.stdout.len() > target);
        assert!(output.stdout.is_ascii());
        assert_eq!(output.exit_code, if outcome == "committed" { 0 } else { 2 });
        assert!(output.warnings.is_empty());
        // Validate the entire emitted document without retaining a second tree
        // or imposing the separate 8 MiB persisted-input profile.
        serde_json::from_str::<serde::de::IgnoredAny>(&output.stdout).unwrap();
        assert_eq!(
            output.stdout.matches("\"outcome\":\"committed\"").count(),
            if outcome == "committed" { 2 } else { 1 }
        );
        assert_eq!(s(get(&envelope, "result"), "outcome"), "committed");
        assert_eq!(warnings(&envelope).len(), 5);
    }
}

#[test]
fn result_and_next_list_ascii_budgets_are_checked_before_either_authority_publishes() {
    for active in [false, true] {
        for giant_task in [false, true] {
            let root = fixture(false);
            let expanded = "\u{7f}".repeat(values::MAX_STRING);
            let mut tasks = Vec::new();
            for index in 0..6 {
                if !giant_task || index == 0 {
                    tasks.push(
                        object(vec![
                            ("id", text(format!("task-{index}"))),
                            ("title", text("Task")),
                            ("status", text("pending")),
                            ("notes", text(&expanded)),
                        ])
                        .unwrap(),
                    );
                } else {
                    tasks[0]
                        .set_field(&format!("extension-{index}"), text(&expanded))
                        .unwrap();
                }
            }
            let tasks = Value::Array(tasks);
            let source = tasks.compact().unwrap();
            assert!(source.len() < values::MAX_INPUT);
            fs::write(root.path().join("state/tasks.json"), &source).unwrap();
            if active {
                activate(root.path());
            }
            let before = raw(root.path());
            let io = io();
            let result = mutate(
                root.path(),
                Change::Assign {
                    task_id: "task-0",
                    agent: "other",
                },
                None,
                active.then_some("key"),
                &io,
            );
            let error = result.unwrap_err();
            assert!(
                matches!(
                    error.code(),
                    "unsupported_json_output_profile" | "unsupported_task_response_profile"
                ),
                "{}",
                error.code()
            );
            assert_eq!(raw(root.path()), before);
            absent(&io, &["replace", "projection", "event", "dashboard"]);
            if active {
                let view = ledger::read(root.path()).unwrap().unwrap();
                assert!(
                    view.lookup(
                        &ledger::RequestKey::parse("key").unwrap(),
                        &ledger::RequestBinding::assign("task-0", "other", None).unwrap()
                    )
                    .unwrap()
                    .is_none()
                );
            }
        }
    }
}

#[test]
fn fresh_same_owner_assign_is_not_exempt_from_prepared_execution_guard() {
    for active in [false, true] {
        let root = fixture(active);
        run_add(root.path(), active.then_some("key"), &io()).unwrap();
        // Generate a real, validated control through the unchanged Python SDK.
        // This never dispatches a worker or uses any execution/runtime CLI.
        let script = r#"
import sys
from pathlib import Path
from runner import coven, storage, task_attempts
root = Path(sys.argv[1])
store = task_attempts.Store(root)
with storage.workspace_lock(root, root / 'state/.workspace.lock') as held:
    for operation, parameters in [
        ('enable', {}),
        ('enroll', {'task_id':'test-title','rebind_source':False}),
        ('prepare', {'task_id':'test-title','intent':{
            'schema_version':1,'kind':'task-intent','task_id':'test-title',
            'owner':'Worker_ID','acceptance_criteria':[{'id':'a','text':'evidence'}]}}),
    ]:
        view = store.read()
        store.mutate(operation, parameters,
            expected_execution_revision=view.revision if view else task_attempts.DISABLED,
            expected_tasks_revision=coven.read_task_snapshot(root)[0],
            request_id=operation, source_loader=lambda sid: coven.execution_source(root, sid), lock=held)
"#;
        let output = std::process::Command::new("python3")
            .args(["-c", script])
            .arg(root.path())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let before = raw(root.path());
        let io = io();
        let error = mutate(
            root.path(),
            Change::Assign {
                task_id: "test-title",
                agent: "worker_id",
            },
            None,
            active.then_some("assign"),
            &io,
        )
        .unwrap_err();
        assert_eq!(error.code(), "prepared_attempt_blocks_mutation");
        assert_eq!(raw(root.path()), before);
        absent(&io, &["replace", "projection", "event", "dashboard"]);
        if active {
            // A known receipt remains replayable even with a live reservation.
            let replay = run_add(root.path(), Some("key"), &io).unwrap();
            assert!(matches!(
                get(status(&replay), "replayed"),
                Value::Bool(true)
            ));
        }
    }
}

#[test]
fn receipt_with_stale_bound_cas_replays_without_clock_ids_or_secondaries() {
    let root = fixture(true);
    let expected = ledger::read(root.path())
        .unwrap()
        .unwrap()
        .revision()
        .to_owned();
    let first = add(
        root.path(),
        "Task",
        None,
        "",
        Some(&expected),
        Some("first"),
    )
    .unwrap();
    assign(root.path(), "task", "other", None, Some("second")).unwrap();
    let events = fs::read(root.path().join("logs/events.jsonl")).unwrap();
    let dashboard = fs::read(root.path().join("dashboard.md")).unwrap();
    let io = io();
    let result = mutate(
        root.path(),
        Change::Add {
            title: "Task",
            owner: None,
            notes: "",
        },
        Some(&expected),
        Some("first"),
        &io,
    )
    .unwrap();
    assert_eq!(
        get(&first, "result").canonical().unwrap(),
        get(&result, "result").canonical().unwrap()
    );
    assert_ne!(
        s(status(&result), "current_revision"),
        s(get(&result, "result"), "after_revision")
    );
    assert_eq!(
        fs::read(root.path().join("logs/events.jsonl")).unwrap(),
        events
    );
    assert_eq!(
        fs::read(root.path().join("dashboard.md")).unwrap(),
        dashboard
    );
    absent(
        &io,
        &[
            "now",
            "uuid",
            "health",
            "commit",
            "projection",
            "event",
            "dashboard",
        ],
    );
}
