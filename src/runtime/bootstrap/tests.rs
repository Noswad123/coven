//! Immediate bootstrap characterizations, not public send semantics.
use super::*;
use crate::runtime::{
    lifecycle::tests::{Fake, fixture, json, ok},
    logs,
};
use crate::runtime_command::Status;
use std::{cell::RefCell, fs};

fn run(root: &Path, fake: &Fake, tmux: bool) -> Result<CommandResult> {
    bootstrap_using(root, |r, _| Ok(transport::Backend::injected(r, tmux, fake)))
}

#[test]
fn aggregate_prompt_staging_refuses_before_locks_or_missing_starter_creation() {
    use crate::runtime::lifecycle::tests::preflight::snapshot;
    let root = fixture("tmux");
    let agents: Vec<_> = std::iter::once("missing".to_owned())
        .chain((0..12).map(|index| format!("agent{index}")))
        .collect();
    fs::write(
        root.path().join("coven.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "Test", "agents": agents, "multiplexer": "tmux",
        }))
        .unwrap(),
    )
    .unwrap();
    let text = "x".repeat(900_000);
    for agent in &agents[1..] {
        fs::write(
            root.path().join(format!("prompts/{agent}.start.txt")),
            &text,
        )
        .unwrap();
    }
    assert_eq!(
        workspace::load(root.path())
            .unwrap()
            .team
            .agent_ids()
            .unwrap()
            .len(),
        13
    );
    let before = snapshot(root.path());
    let fake = Fake::new(root.path(), vec![]);
    assert_eq!(
        run(root.path(), &fake, true).unwrap_err().code(),
        "unsupported_runtime_staging_profile"
    );
    assert_eq!(snapshot(root.path()), before);
    assert!(fake.calls.borrow().is_empty());
    assert!(!root.path().join("prompts/missing.start.txt").exists());
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("logs").exists());
}

#[test]
fn late_oversized_prompt_batch_rechecks_same_budget_before_any_starter_write() {
    use crate::runtime::lifecycle::tests::preflight::snapshot;
    for after_workspace_lock in [false, true] {
        let root = fixture("tmux");
        let agents: Vec<_> = std::iter::once("missing".to_owned())
            .chain((0..12).map(|index| format!("agent{index}")))
            .collect();
        fs::write(
            root.path().join("coven.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "Test", "agents": agents, "multiplexer": "tmux",
            }))
            .unwrap(),
        )
        .unwrap();
        let fake = Fake::new(root.path(), vec![]);
        let before = RefCell::new(None);
        let enlarge = |root: &Path| {
            let text = "x".repeat(900_000);
            for agent in &agents[1..] {
                fs::write(root.join(format!("prompts/{agent}.start.txt")), &text).unwrap();
            }
            *before.borrow_mut() = Some(snapshot(root));
        };
        let result = bootstrap_admitting(
            root.path(),
            |r, _| {
                if after_workspace_lock {
                    enlarge(r);
                }
                Ok(transport::Backend::injected(r, true, &fake))
            },
            |root| {
                let gate = TransportGuard::shared(root, TIMEOUT)?;
                if !after_workspace_lock {
                    enlarge(root);
                }
                Ok(gate)
            },
        );
        assert_eq!(
            result.unwrap_err().code(),
            "unsupported_runtime_staging_profile"
        );
        assert_eq!(snapshot(root.path()), before.into_inner().unwrap());
        assert!(!root.path().join("prompts/missing.start.txt").exists());
        assert!(fake.calls.borrow().is_empty());
        if !after_workspace_lock {
            assert!(!root.path().join("state/.workspace.lock").exists());
        }
    }
}

#[test]
fn characterization_custom_starter_buffers_ignored_nonzero_continue_and_attempt_events() {
    let root = fixture("tmux");
    let custom = "custom\nUTF-8: \u{03bb}\n";
    fs::write(root.path().join("prompts/Beta.start.txt"), custom).unwrap();
    let fake = Fake::new(
        root.path(),
        vec![
            (0, "%5\n%6".into()),
            (1, "".into()),
            (1, "".into()),
            (1, "".into()),
            (0, "%7".into()),
            ok(),
            ok(),
            ok(),
        ],
    );
    assert_eq!(
        run(root.path(), &fake, true).unwrap().body,
        "[INFO] Bootstrap prompts sent: 2"
    );
    fake.assert_done();
    assert_eq!(
        fs::read_to_string(root.path().join("prompts/Beta.start.txt")).unwrap(),
        custom
    );
    let calls = fake.calls.borrow();
    assert_eq!(
        calls[1],
        [
            "load-buffer",
            "-b",
            "coven-test-Beta-starter",
            root.path().join("prompts/Beta.start.txt").to_str().unwrap()
        ]
    );
    assert_eq!(
        calls[2],
        ["paste-buffer", "-b", "coven-test-Beta-starter", "-t", "%5"]
    );
    assert_eq!(calls[3], ["send-keys", "-t", "%5", "Enter"]);
    assert!(root.path().join("prompts/alpha.start.txt").is_file());
    assert_eq!(
        logs::read(root.path(), logs::LogKind::Events)
            .unwrap()
            .len(),
        2
    );
    assert!(!root.path().join("logs/messages.jsonl").exists());
    assert!(!root.path().join("dashboard.md").exists());
}

#[test]
fn characterization_entire_utf8_batch_before_missing_files_created() {
    let root = fixture("tmux");
    fs::write(root.path().join("prompts/alpha.start.txt"), [0xff]).unwrap();
    let fake = Fake::new(root.path(), vec![]);
    assert_eq!(
        run(root.path(), &fake, true).unwrap_err().code(),
        "invalid_utf8"
    );
    assert!(!root.path().join("prompts/Beta.start.txt").exists());
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn characterization_late_invalid_prompt_prevents_next_missing_creation() {
    let root = fixture("tmux");
    fs::write(root.path().join("prompts/Beta.start.txt"), "authored").unwrap();
    let mut fake = Fake::new(root.path(), vec![(1, "".into())]);
    fake.after = Some(|root, _| {
        fs::write(root.join("prompts/Beta.start.txt"), [0xff]).unwrap();
    });
    assert_eq!(
        run(root.path(), &fake, true).unwrap_err().code(),
        "invalid_utf8"
    );
    assert!(!root.path().join("prompts/alpha.start.txt").exists());
    fake.assert_done();
}

#[test]
fn characterization_setup_timeout_capture_failure_stop_without_retry_or_history() {
    for status in [Status::SpawnError, Status::Timeout, Status::OutputLimit] {
        let root = fixture("tmux");
        let fake = Fake::new(root.path(), vec![(0, "%1".into()), ok()]);
        fake.fail_last(status);
        assert!(run(root.path(), &fake, true).is_err());
        fake.assert_done();
        assert_eq!(fake.calls.borrow().len(), 2);
        assert!(!root.path().join("prompts/alpha.start.txt").exists());
        assert!(!root.path().join("logs/events.jsonl").exists());
    }
}

#[test]
fn characterization_herdr_text_then_enter_ignored_nonzero_and_missing_skip() {
    let root = fixture("herdr");
    fs::write(root.path().join("prompts/Beta.start.txt"), "custom\n").unwrap();
    let fake = Fake::new(
        root.path(),
        vec![
            json(serde_json::json!({"workspaces":[{"label":"coven-test","workspace_id":"w1"}]})),
            json(
                serde_json::json!({"tabs":[{"label":"agent-Beta","workspace_id":"w1","tab_id":"t1"}]}),
            ),
            json(serde_json::json!({"panes":[{"tab_id":"t1","pane_id":"p1"}]})),
            (1, "".into()),
            (1, "".into()),
            json(serde_json::json!({"workspaces":[]})),
        ],
    );
    let result = run(root.path(), &fake, false).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        result
            .body
            .contains("No running agent pane found for: alpha")
    );
    assert!(result.body.ends_with("Bootstrap prompts sent: 1"));
    assert_eq!(
        fake.calls.borrow()[3],
        ["pane", "send-text", "p1", "custom\n"]
    );
    assert_eq!(fake.calls.borrow()[4], ["pane", "send-keys", "p1", "enter"]);
    fake.assert_done();
}

#[test]
fn characterization_late_history_failure_reports_known_attempt_without_retry() {
    let root = fixture("tmux");
    let mut fake = Fake::new(root.path(), vec![(0, "%1".into()), ok(), ok(), ok()]);
    fake.after = Some(|root, call| {
        if call == 4 {
            fs::create_dir_all(root.join("logs")).unwrap();
            fs::write(root.join("logs/events.jsonl"), "invalid\n").unwrap();
        }
    });
    let result = run(root.path(), &fake, true).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.body.contains("attempted for 1 agent(s)"));
    assert!(result.body.contains("no retry performed"));
    fake.assert_done();
}

#[test]
fn characterization_admission_race_rechecks_mode_before_prompt_or_workspace_reads() {
    let root = fixture("tmux");
    let gate = TransportGuard::exclusive(root.path(), TIMEOUT).unwrap();
    let path = root.path().to_owned();
    let (started, waiting) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        let fake = Fake::new(&path, vec![]);
        bootstrap_admitting(
            &path,
            |r, _| Ok(transport::Backend::injected(r, true, &fake)),
            |root| {
                started.send(()).unwrap();
                TransportGuard::shared(root, TIMEOUT)
            },
        )
        .unwrap_err()
        .code()
    });
    waiting.recv().unwrap();
    crate::runtime::lifecycle::tests::enable_control(root.path());
    fs::write(root.path().join("coven.toml"), "broken [").unwrap();
    drop(gate);
    assert_eq!(child.join().unwrap(), "legacy_runtime_disabled");
    assert!(!root.path().join("prompts/Beta.start.txt").exists());
}

#[test]
fn bad_workspace_or_prompt_batch_leaves_uninitialized_workspace_unchanged() {
    use crate::runtime::lifecycle::tests::preflight::snapshot;
    for bad_config in [false, true] {
        let root = fixture("tmux");
        if bad_config {
            fs::write(root.path().join("coven.toml"), "broken [").unwrap();
        } else {
            fs::write(root.path().join("prompts/alpha.start.txt"), [0xff]).unwrap();
        }
        let before = snapshot(root.path());
        let fake = Fake::new(root.path(), vec![]);
        assert!(run(root.path(), &fake, true).is_err());
        assert_eq!(snapshot(root.path()), before);
        assert!(super::bootstrap(root.path()).is_err());
        assert_eq!(snapshot(root.path()), before);
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn late_bad_prompt_batch_is_freshly_checked_without_earlier_creation() {
    use crate::runtime::lifecycle::tests::preflight::snapshot;
    for after_workspace_lock in [false, true] {
        let root = fixture("tmux");
        let fake = Fake::new(root.path(), vec![]);
        let before = RefCell::new(None);
        let invalidate = |root: &Path| {
            fs::write(root.join("prompts/alpha.start.txt"), [0xff]).unwrap();
            *before.borrow_mut() = Some(snapshot(root));
        };
        let result = bootstrap_admitting(
            root.path(),
            |r, _| {
                if after_workspace_lock {
                    invalidate(r);
                }
                Ok(transport::Backend::injected(r, true, &fake))
            },
            |root| {
                let gate = TransportGuard::shared(root, TIMEOUT)?;
                if !after_workspace_lock {
                    invalidate(root);
                }
                Ok(gate)
            },
        );
        assert_eq!(result.unwrap_err().code(), "invalid_utf8");
        assert_eq!(snapshot(root.path()), before.into_inner().unwrap());
        assert!(!root.path().join("prompts/Beta.start.txt").exists());
        assert!(fake.calls.borrow().is_empty());
        if !after_workspace_lock {
            assert!(!root.path().join("state/.workspace.lock").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn characterization_missing_parent_and_symlink_never_repaired_or_replaced() {
    let root = fixture("tmux");
    fs::remove_dir(root.path().join("prompts")).unwrap();
    let fake = Fake::new(root.path(), vec![]);
    assert!(run(root.path(), &fake, true).is_err());
    assert!(!root.path().join("prompts").exists());
    fs::create_dir(root.path().join("prompts")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("prompts/alpha.start.txt"))
        .unwrap();
    assert!(run(root.path(), &fake, true).is_err());
    assert!(!root.path().join("prompts/Beta.start.txt").exists());
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn characterization_history_is_validated_after_native_attempt_not_before() {
    let root = fixture("tmux");
    fs::create_dir(root.path().join("logs")).unwrap();
    fs::write(root.path().join("logs/events.jsonl"), "not json\n").unwrap();
    let fake = Fake::new(root.path(), vec![(0, "%1".into()), ok(), ok(), ok()]);
    let result = run(root.path(), &fake, true).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.body.contains("attempted for 1 agent(s)"));
    fake.assert_done();
    assert!(!root.path().join("prompts/alpha.start.txt").exists());
}
