use super::*;
use std::{cell::Cell, fs, process::Command};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("coven.json"), br#"{"name":"Status Team","agents":["Beta","alpha"],"display_names":{"Beta":"Label\n<&>*\u200b"},"multiplexer":"tmux"}"#).unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    root
}

#[test]
fn standup_body_preserves_intrinsic_newline_and_writeln_matches_python_print() {
    use std::io::Write;
    let root = fixture();
    let result = standup(root.path()).unwrap();
    let expected = concat!(
        "# Coven Standup: Status Team\n",
        "## Coven Lead\n\n- None set.\n\n",
        "## Done\n\n- Nothing completed yet.\n\n",
        "## In Progress\n\n- Nothing in progress.\n\n",
        "## Blocked / Waiting\n\n- No blockers recorded.\n\n",
        "## Pending\n\n- No pending tasks.\n\n",
        "## Agents\n\n- No agents recorded.\n\n",
        "## Checkpoints\n\n",
        "- `plan-approval` \u{2014} missing (`checkpoints/plan-approval.md`)\n",
        "- `pre-delivery-review` \u{2014} missing (`checkpoints/pre-delivery-review.md`)\n\n",
        "## Suggested Next Action\n\n",
        "- Continue from the dashboard and current task assignments.\n",
    );
    assert_eq!(result.body, expected);
    let mut stdout = Vec::new();
    writeln!(stdout, "{}", result.body).unwrap();
    assert_eq!(stdout, format!("{expected}\n").as_bytes());
    assert!(stdout.ends_with(b"\n\n"));
    let oracle = Command::new("python3").args(["-B", "-c", "import argparse,sys;from runner import coven;coven.command_standup(argparse.Namespace(workspace=sys.argv[1]))"])
        .arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(stdout, oracle.stdout);
}
fn approve(root: &Path) {
    let location = crate::platform::workspace_location(root).unwrap();
    fs::write(root.join("state/runtime-launch-control.json"), serde_json::to_vec(&serde_json::json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),"epoch_id":"b".repeat(32),
        "location":{"root":location.root,"device":location.device,"inode":location.inode},"head":null,"claims":{}
    })).unwrap()).unwrap();
}
#[test]
fn python_standup_all_buckets_persisted_rows_historical_lead_and_labels() {
    let root = fixture();
    fs::write(root.path().join("state/agents.json"), br#"[{"id":"outside","status":"waiting","role":"old"},{"id":"Beta","status":"active","role":"dev"}]"#).unwrap();
    fs::write(
        root.path().join("state/lead.json"),
        br#"{"agent":"historical","extra":null}"#,
    )
    .unwrap();
    let statuses = [
        "done",
        "completed",
        "BLOCKED",
        "awaiting",
        "in_progress",
        "assigned",
        "active",
        "review",
        "",
        "new",
    ];
    let tasks: Vec<_> = statuses.iter().enumerate().map(|(i, s)| serde_json::json!({"id":format!("t{i}"),"title":format!("Title {i}"),"status":s,"owner":if i%2 == 0 {Some("outside")} else {None}})).collect();
    fs::write(
        root.path().join("state/tasks.json"),
        serde_json::to_vec(&tasks).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    fs::write(
        root.path().join("checkpoints/plan-approval.md"),
        "# Plan\r\nStatus: approved\r\n",
    )
    .unwrap();
    let state = workspace::load(root.path()).unwrap();
    let mut seen = Vec::new();
    let rendered = standup::render(root.path(), &state, |agent| {
        seen.push(agent.to_owned());
        Ok(("running".into(), Some(format!("pane-{agent}"))))
    })
    .unwrap();
    assert_eq!(seen, ["outside", "Beta", "historical"]);
    let oracle = Command::new("python3").args(["-B", "-c", "import sys;from pathlib import Path;from runner import coven;coven.agent_runtime_status=lambda team,a:('running','pane-'+a);sys.stdout.write(coven.render_standup(Path(sys.argv[1])))"])
        .arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(rendered, String::from_utf8(oracle.stdout).unwrap());
}
#[test]
fn approved_observation_skips_backend_and_public_statuses_preserve_roster_defaults() {
    let root = fixture();
    approve(root.path());
    fs::write(
        root.path().join("state/agents.json"),
        br#"[{"id":"Beta","status":"saved","role":"dev"}]"#,
    )
    .unwrap();
    let gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    let state = workspace::load(root.path()).unwrap();
    let calls = Cell::new(0);
    let observed = observe(root.path(), &state, &gate, "unlisted", |_| {
        calls.set(calls.get() + 1);
        panic!("backend constructed")
    })
    .unwrap();
    assert_eq!(calls.get(), 0);
    assert_eq!(
        observed,
        ("unverified (inspect action transport records)".into(), None)
    );
    drop(gate);
    let actual = agent_status(root.path(), &[]).unwrap();
    let oracle = Command::new("python3").args(["-B", "-c", "import argparse,sys;from runner import coven;coven.command_agent_status(argparse.Namespace(items=[sys.argv[1]]))"])
        .arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(format!("{}\n", actual.body).as_bytes(), oracle.stdout);
    assert_eq!(
        agent_status(root.path(), &["Beta".into(), "Label".into()])
            .unwrap_err()
            .code(),
        "unknown_agent"
    );
    assert!(standup(root.path()).unwrap().body.ends_with('\n'));
}
#[test]
fn legacy_observation_admits_unlisted_ids_without_workspace_exclusion() {
    let root = fixture();
    let gate = TransportGuard::shared(root.path(), TIMEOUT).unwrap();
    let state = workspace::load(root.path()).unwrap();
    let observed = observe(root.path(), &state, &gate, "unlisted", |namespace| {
        assert_eq!(namespace, "coven-status-team");
        assert!(
            crate::storage::WorkspaceGuard::acquire(root.path(), std::time::Duration::ZERO).is_ok()
        );
        assert!(TransportGuard::exclusive(root.path(), std::time::Duration::ZERO).is_err());
        Ok(Some("%42".into()))
    })
    .unwrap();
    assert_eq!(observed, ("running".into(), Some("%42".into())));
    assert_eq!(
        observe(root.path(), &state, &gate, "../bad", |_| panic!())
            .unwrap_err()
            .code(),
        "invalid_record_identifier"
    );
}
#[test]
fn arbitrary_python_values_use_the_existing_bounded_repr() {
    let value = values::parse(
        br#"{"extension":[null,true,{"nested":"line\n"}],"number":123456789012345678901234567890}"#,
        values::Profile::LegacySnapshot,
    )
    .unwrap();
    let mut out = python::Output::default();
    out.value(&value).unwrap();
    assert_eq!(
        out.finish(),
        "{'extension': [None, True, {'nested': 'line\\n'}], 'number': 123456789012345678901234567890}"
    );
}

#[test]
fn ledger_authority_ignores_corrupt_projection_and_matches_python() {
    let root = fixture();
    let raw = br#"[{"id":"ledger-task","title":"Ledger authority wins","status":"done"}]"#;
    fs::write(root.path().join("state/tasks.json"), raw).unwrap();
    let guard =
        crate::storage::WorkspaceGuard::acquire(root.path(), std::time::Duration::ZERO).unwrap();
    let revision = crate::ledger::snapshot_revision(Some(raw));
    let _ = crate::ledger::import_tasks(root.path(), &revision, &guard).unwrap();
    drop(guard);
    fs::write(root.path().join("state/tasks.json"), b"corrupt projection").unwrap();
    approve(root.path());
    let actual = standup(root.path()).unwrap();
    assert!(actual.body.contains("Ledger authority wins"));
    let oracle = Command::new("python3").args(["-B", "-c", "import sys;from pathlib import Path;from runner import coven;sys.stdout.write(coven.render_standup(Path(sys.argv[1])))"])
        .arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(actual.body.as_bytes(), oracle.stdout);
    assert_eq!(
        fs::read(root.path().join("state/tasks.json")).unwrap(),
        b"corrupt projection"
    );
}

#[test]
fn effective_checkpoint_statuses_suggestion_branches_and_empty_sections_match_python() {
    let root = fixture();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    let (approved, _) = crate::checkpoints::prepare(
        "pre-delivery-review",
        Some(b"# Review\r\nStatus: pending\r\n## User decision\r\n- [ ] Approved\r\n"),
        "approved",
        "",
        "11111111111111111111111111111111",
        "test",
        "cli",
        None,
    )
    .unwrap();
    let approved = String::from_utf8(approved).unwrap();
    for (content, expected) in [
        (approved.clone(), "approved"),
        (
            approved.replace("# Review\r\n", "# Changed review\r\n"),
            "approval_stale",
        ),
        (
            approved.replace("Status: approved", "Status: pending"),
            "decision_invalid",
        ),
        ("# Review\nStatus: pending\n".into(), "pending"),
    ] {
        fs::write(
            root.path().join("checkpoints/pre-delivery-review.md"),
            content,
        )
        .unwrap();
        for tasks in [
            b"[]".as_slice(),
            br#"[{"id":"done","title":"Complete","status":"done"}]"#,
        ] {
            fs::write(root.path().join("state/tasks.json"), tasks).unwrap();
            let state = workspace::load(root.path()).unwrap();
            let actual =
                standup::render(root.path(), &state, |_| panic!("no persisted rows or lead"))
                    .unwrap();
            assert!(actual.contains(&format!("`pre-delivery-review` \u{2014} {expected}")));
            let oracle = Command::new("python3").args(["-B", "-c", "import sys;from pathlib import Path;from runner import coven;sys.stdout.write(coven.render_standup(Path(sys.argv[1])))"])
                .arg(root.path()).env("COVEN_MULTIPLEXER", "").current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
            assert!(
                oracle.status.success(),
                "{}",
                String::from_utf8_lossy(&oracle.stderr)
            );
            assert_eq!(actual, String::from_utf8(oracle.stdout).unwrap());
        }
    }
}

#[test]
fn retained_agent_rows_stop_rendering_at_budget_not_after_the_entire_batch() {
    let root = fixture();
    let mut state = workspace::load(root.path()).unwrap();
    let mut team = state.team.value().clone();
    team.set_field(
        "display_names",
        Value::object(vec![(
            "Beta".into(),
            Value::String("_".repeat(values::MAX_STRING)),
        )])
        .unwrap(),
    )
    .unwrap();
    state.team = crate::records::TeamRecord::validate(team).unwrap();
    let saved = Value::object(vec![
        ("id".into(), Value::String("Beta".into())),
        ("status".into(), Value::String("idle".into())),
        ("role".into(), Value::String("dev".into())),
    ])
    .unwrap();
    // Duplicate IDs are refused on disk. This counterfactual in-memory fixture
    // tests the renderer's independent resource boundary if that policy changes.
    state.agents = Value::Array(vec![saved; 64]);
    assert_eq!(
        crate::records::agents(&state.agents).unwrap_err().code(),
        "duplicate_record_identifier"
    );
    let mut observations = 0;
    let error = standup::render(root.path(), &state, |_| {
        observations += 1;
        Ok(("stopped".into(), None))
    })
    .unwrap_err();
    assert_eq!(error.code(), "unsupported_history_output_profile");
    // Markdown doubles each underscore: 15 rows fit, the 16th exceeds 32 MiB.
    assert_eq!(observations, 16);
}
