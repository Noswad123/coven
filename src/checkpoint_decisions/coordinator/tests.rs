use super::*;
use std::{
    cell::{Cell, RefCell},
    fs,
};

pub(crate) fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["a","b"]}"#,
    )
    .unwrap();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    fs::write(
        root.path().join("checkpoints/plan.md"),
        b"# Plan\r\nStatus: pending\r\n\r\n## Purpose\r\nEvidence\r\n",
    )
    .unwrap();
    root
}
fn request(root: &Path, conditional: bool) -> DecisionRequest {
    DecisionRequest {
        checkpoint: "plan".into(),
        action: DecisionAction::Approve,
        notes: "Reviewed".into(),
        source: "cli".into(),
        agent: "all".into(),
        expected_revision: conditional.then(|| {
            field(&view(root, "plan").unwrap(), "revision")
                .unwrap()
                .into()
        }),
    }
}
#[derive(Default)]
struct TestIo {
    uncertain: bool,
    fail_replace: bool,
    fail_secondary: bool,
    fail_send: bool,
    fail_history: bool,
    stale: bool,
    supersede: bool,
    race: bool,
    notices: Cell<usize>,
    secondary_calls: Cell<usize>,
    phases: RefCell<Vec<String>>,
}
impl Io for TestIo {
    fn phase(&self, phase: &str, root: &Path) -> Result<()> {
        self.phases.borrow_mut().push(phase.into());
        if self.race {
            let held = WorkspaceGuard::acquire(root, Duration::ZERO)?;
            let path = relative("plan")?;
            // A competing machinery-only edit changes snapshot, not scope.
            let raw = fs::read(root.join(&path))?;
            let (bytes, _) = checkpoints::prepare(
                "plan",
                Some(&raw),
                "approved",
                "other",
                "11111111111111111111111111111111",
                "now",
                "cli",
                None,
            )?;
            storage::atomic_replace_bytes(&held, &path, &bytes)?;
        }
        Ok(())
    }
    fn replace(&self, held: &WorkspaceGuard, relative: &Path, bytes: &[u8]) -> Result<Delivery> {
        if self.fail_replace {
            return Err(Error("injected_prepublication_failure"));
        }
        let delivery = storage::atomic_replace_bytes(held, relative, bytes)?;
        Ok(if self.uncertain {
            Delivery::DurabilityUncertain
        } else {
            delivery
        })
    }
    fn secondary(
        &self,
        label: &str,
        root: &Path,
        request: &DecisionRequest,
        report: &Report,
        held: &WorkspaceGuard,
    ) -> Result<()> {
        self.secondary_calls.set(self.secondary_calls.get() + 1);
        if self.fail_secondary {
            Err(Error("injected_secondary_failure"))
        } else {
            SystemIo.secondary(label, root, request, report, held)
        }
    }
    fn targets(&self, _: &Path, _: &str) -> Result<Vec<String>> {
        Ok(vec!["a".into(), "b".into()])
    }
    fn send(&self, root: &Path, _: &str, text: &str) -> Result<bool> {
        let _held = WorkspaceGuard::acquire(root, Duration::ZERO)?;
        assert!(text.contains("Reinspect current checkpoint status before acting"));
        assert!(text.contains("not execution authorization"));
        assert!(text.contains("scope checkpoint-scope-v1:"));
        self.notices.set(self.notices.get() + 1);
        if self.stale {
            let path = root.join("checkpoints/plan.md");
            let mut raw = fs::read(&path)?;
            raw.extend_from_slice(b"\nChanged\n");
            fs::write(path, raw)?;
        }
        if self.supersede {
            let path = root.join("checkpoints/plan.md");
            let raw = fs::read(&path)?;
            let (bytes, _) = checkpoints::prepare(
                "plan",
                Some(&raw),
                "approved",
                "new",
                "22222222222222222222222222222222",
                "now",
                "cli",
                None,
            )?;
            fs::write(path, bytes)?;
        }
        if self.fail_send {
            Err(Error("enter_rejected"))
        } else {
            Ok(true)
        }
    }
    fn notice_history(&self, root: &Path, agent: &str, report: &Report) -> Result<()> {
        if self.fail_history {
            Err(Error("notice_history_failed"))
        } else {
            SystemIo.notice_history(root, agent, report)
        }
    }
}

#[test]
fn characterization_commit_report_and_unlocked_real_secondary_history() {
    let root = fixture();
    let io = TestIo::default();
    let report = commit(root.path(), &request(root.path(), true), &io).unwrap();
    assert_eq!(report.outcome, "committed");
    assert_eq!(report.selection, "conditional");
    assert_eq!(report.sent, 2);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        render(&report, false).unwrap().body,
        format!(
            "[INFO] Recorded plan decision {}: approved (selection=conditional)",
            report.decision_id
        )
    );
    assert_eq!(
        field(&view(root.path(), "plan").unwrap(), "effective_status").unwrap(),
        "approved"
    );
    let events = logs::read(root.path(), LogKind::Events).unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(field(&events[0], "type").unwrap(), "checkpoint.approved");
    assert_eq!(field(&events[1], "type").unwrap(), "message.sent");
    assert_eq!(field(&events[2], "type").unwrap(), "checkpoint.notice.sent");
    let parsed = report.value().unwrap();
    for key in [
        "outcome",
        "checkpoint",
        "action",
        "decision_id",
        "selection",
        "before_revision",
        "after_revision",
        "scope_revision",
        "warnings",
        "sent",
        "missing",
    ] {
        assert!(parsed.as_object().unwrap().get(key).is_some());
    }
}

#[test]
fn characterization_uncertainty_and_prepublication_failures_skip_secondary_and_notices() {
    for uncertain in [false, true] {
        let root = fixture();
        let before = fs::read(root.path().join("checkpoints/plan.md")).unwrap();
        let io = TestIo {
            uncertain,
            fail_replace: !uncertain,
            ..Default::default()
        };
        let result = commit(root.path(), &request(root.path(), true), &io);
        if uncertain {
            let report = result.unwrap();
            assert_eq!(report.outcome, "durability_uncertain");
            assert_eq!(render(&report, true).unwrap().exit_code, 2);
            assert_eq!(
                render(&report, false).unwrap().body,
                format!(
                    "[INFO] Replaced; durability uncertain for plan decision {}: approved (selection=conditional)\n[WARN] Checkpoint replaced; directory synchronization failed. Inspect before retrying.",
                    report.decision_id
                )
            );
            assert_ne!(
                fs::read(root.path().join("checkpoints/plan.md")).unwrap(),
                before
            );
        } else {
            assert!(result.is_err());
            assert_eq!(
                fs::read(root.path().join("checkpoints/plan.md")).unwrap(),
                before
            );
        }
        assert_eq!(io.secondary_calls.get(), 0);
        assert_eq!(io.notices.get(), 0);
        assert!(!root.path().join("logs").exists());
        assert!(!root.path().join("dashboard.md").exists());
    }
}

#[test]
fn characterization_preflight_output_bound_reserved_notes_and_scope_tokens_create_no_lock() {
    for case in 0..4 {
        let root = fixture();
        let mut request = request(root.path(), false);
        match case {
            0 => {
                request.expected_revision = Some(
                    field(&view(root.path(), "plan").unwrap(), "scope_revision")
                        .unwrap()
                        .into(),
                )
            }
            1 => request.notes = "raw -->".into(),
            2 => {
                fs::write(
                    root.path().join("checkpoints/plan.md"),
                    vec![b'a'; 1_048_576],
                )
                .unwrap();
            }
            _ => request.source = "forged".into(),
        }
        assert!(commit(root.path(), &request, &TestIo::default()).is_err());
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn characterization_reload_under_lock_rejects_same_scope_competing_snapshot() {
    let root = fixture();
    let request = request(root.path(), true);
    let scope = field(&view(root.path(), "plan").unwrap(), "scope_revision")
        .unwrap()
        .to_owned();
    let io = TestIo {
        race: true,
        ..Default::default()
    };
    assert_eq!(
        commit(root.path(), &request, &io).err().unwrap().code(),
        "stale_checkpoint_revision"
    );
    assert_eq!(
        field(&view(root.path(), "plan").unwrap(), "scope_revision").unwrap(),
        scope
    );
    assert_eq!(io.notices.get(), 0);
    assert_eq!(io.secondary_calls.get(), 0);
}

#[test]
fn characterization_secondary_and_transport_failures_warn_without_rollback() {
    for case in 0..4 {
        let root = fixture();
        let io = TestIo {
            fail_secondary: case == 0,
            fail_send: case == 1,
            fail_history: case == 2,
            ..Default::default()
        };
        if case == 3 {
            fs::create_dir(root.path().join("logs")).unwrap();
            fs::write(root.path().join("logs/events.jsonl"), b"corrupt\n").unwrap();
        }
        let report = commit(root.path(), &request(root.path(), false), &io).unwrap();
        assert_eq!(render(&report, true).unwrap().exit_code, 0);
        assert!(!report.warnings.is_empty());
        assert_eq!(report.sent, if case == 1 { 0 } else { 2 });
        assert_eq!(
            field(&view(root.path(), "plan").unwrap(), "effective_status").unwrap(),
            "approved"
        );
    }
}

#[test]
fn characterization_per_recipient_recheck_suppresses_stale_and_same_scope_superseded() {
    for stale in [true, false] {
        let root = fixture();
        let io = TestIo {
            stale,
            supersede: !stale,
            ..Default::default()
        };
        let report = commit(root.path(), &request(root.path(), false), &io).unwrap();
        assert_eq!(report.sent, 1);
        assert_eq!(io.notices.get(), 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|text| text.contains("suppressed"))
        );
    }
}

#[test]
fn characterization_missing_default_preserves_legacy_title_and_purpose() {
    assert!(
        String::from_utf8(initial("plan2_test-CHECK"))
            .unwrap()
            .starts_with("# Checkpoint: Plan2_Test Check\n")
    );
    let root = fixture();
    let mut request = request(root.path(), false);
    request.checkpoint = "custom".into();
    request.expected_revision = Some("checkpoint-missing-v1:custom".into());
    let report = commit(root.path(), &request, &TestIo::default()).unwrap();
    assert_eq!(report.before_revision, "checkpoint-missing-v1:custom");
    let current = view(root.path(), "custom").unwrap();
    assert!(
        field(&current, "content")
            .unwrap()
            .contains("User approval checkpoint.")
    );
}

#[test]
fn characterization_conditional_and_missing_races_have_one_winner_latest_keeps_all_history() {
    use std::sync::{Arc, Barrier};
    struct RacingIo(Arc<Barrier>);
    impl Io for RacingIo {
        fn phase(&self, _: &str, _: &Path) -> Result<()> {
            self.0.wait();
            Ok(())
        }
        fn targets(&self, _: &Path, _: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }
    for mode in ["conditional", "missing", "latest"] {
        let root = fixture();
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();
        for index in 0..3 {
            let mut request = request(root.path(), mode == "conditional");
            if mode == "missing" {
                request.checkpoint = "new".into();
                request.expected_revision = Some("checkpoint-missing-v1:new".into());
            }
            request.action = if index == 0 {
                DecisionAction::Suggest
            } else {
                DecisionAction::Approve
            };
            request.notes = format!("Review {index}");
            let path = root.path().to_owned();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                commit(&path, &request, &RacingIo(barrier))
            }));
        }
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        let winners = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(winners, if mode == "latest" { 3 } else { 1 });
        for failure in results.iter().filter_map(|r| r.as_ref().err()) {
            assert_eq!(failure.code(), "stale_checkpoint_revision");
        }
        let current = view(root.path(), if mode == "missing" { "new" } else { "plan" }).unwrap();
        assert_eq!(
            current
                .as_object()
                .unwrap()
                .get("history")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            winners
        );
    }
}

#[test]
fn characterization_unavailable_targets_and_missing_panes_preserve_legacy_counts() {
    struct Unavailable;
    impl Io for Unavailable {
        fn targets(&self, _: &Path, _: &str) -> Result<Vec<String>> {
            Err(Error("runtime_unavailable"))
        }
    }
    struct Missing;
    impl Io for Missing {
        fn targets(&self, _: &Path, _: &str) -> Result<Vec<String>> {
            Ok(vec!["a".into(), "b".into()])
        }
        fn send(&self, _: &Path, _: &str, _: &str) -> Result<bool> {
            Ok(false)
        }
    }
    let root = fixture();
    let report = commit(root.path(), &request(root.path(), false), &Unavailable).unwrap();
    assert_eq!(report.sent, 0);
    assert!(report.missing.is_empty());
    assert_eq!(report.warnings.len(), 1);
    let report = commit(root.path(), &request(root.path(), false), &Missing).unwrap();
    assert_eq!(report.sent, 0);
    assert_eq!(report.missing, vec!["a", "b"]);
    assert!(report.warnings.is_empty());
}
