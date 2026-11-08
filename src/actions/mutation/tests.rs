use super::*;
use std::{
    cell::{Cell, RefCell},
    fs,
    io::Write,
    process::{Command, Stdio},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
};

fn parsed(output: CommandResult) -> Value {
    values::parse(output.body.as_bytes(), Profile::Authority).unwrap()
}
fn oracle(root: &Path, script: &str, input: &[u8]) -> Vec<u8> {
    let mut child = Command::new("python3")
        .args(["-B", "-c", script])
        .arg(root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
pub(super) fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in ["state", "prompts", "agents", "checkpoints", "logs"] {
        fs::create_dir(temp.path().join(path)).unwrap();
    }
    fs::write(temp.path().join("coven.json"), br#"{"name":"test","agents":["alice"],"multiplexer":"tmux","opencode_command":"oc --flag"}"#).unwrap();
    fs::write(temp.path().join("state/tasks.json"), b"[]").unwrap();
    fs::write(
        temp.path().join("prompts/alice.md"),
        "Role <literal> `code` \u{96ea}\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("prompts/alice.start.txt"),
        "Start\nnow <literal> `code` \u{1f98a}",
    )
    .unwrap();
    temp
}
type DuringTransport = Box<dyn Fn(&Value)>;
pub(super) struct Fake {
    executes: Cell<usize>,
    stops: Cell<usize>,
    writes: Cell<usize>,
    fail_write: usize,
    uncertain_write: usize,
    confirm_uncertain: bool,
    failed: bool,
    history_fails: bool,
    publications: RefCell<Vec<PathBuf>>,
    fail_publish: usize,
    during: Option<DuringTransport>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            executes: Cell::new(0),
            stops: Cell::new(0),
            writes: Cell::new(0),
            fail_write: 0,
            uncertain_write: 0,
            confirm_uncertain: false,
            failed: false,
            history_fails: false,
            publications: RefCell::new(vec![]),
            fail_publish: 0,
            during: None,
        }
    }
}
pub(super) fn report(failed: bool) -> Value {
    object(vec![
        ("operation", string("runtime_launch")),
        (
            "status",
            string(if failed {
                "transport_failed_or_partial"
            } else {
                "transport_completed"
            }),
        ),
        ("completed_steps", Value::integer("1").unwrap()),
        ("handles", object(vec![]).unwrap()),
        ("diagnostics", Value::Array(vec![])),
        ("outcome_unknown", Value::Bool(failed)),
    ])
    .unwrap()
}
impl Io for Fake {
    fn replace(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        self.writes.set(self.writes.get() + 1);
        if self.fail_write == self.writes.get() {
            return Err(Error("injected_replace_failure"));
        }
        let delivered = SystemIo.replace(guard, bytes)?;
        Ok(if self.uncertain_write == self.writes.get() {
            Delivery::DurabilityUncertain
        } else {
            delivered
        })
    }
    fn confirm(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        if self.confirm_uncertain {
            return Ok(Delivery::DurabilityUncertain);
        }
        SystemIo.confirm(guard, bytes)
    }
    fn publish(&self, guard: &WorkspaceGuard, relative: &Path, bytes: &[u8]) -> Result<Delivery> {
        self.publications.borrow_mut().push(relative.to_owned());
        if self.publications.borrow().len() == self.fail_publish {
            return Err(Error("injected_publication_failure"));
        }
        SystemIo.publish(guard, relative, bytes)
    }
    fn execute(&self, plan: &Value) -> Value {
        self.executes.set(self.executes.get() + 1);
        let root = Path::new(text(plan, "root").unwrap());
        let guard = WorkspaceGuard::execution_maintenance(root, Duration::ZERO)
            .expect("transport must run outside workspace lock");
        drop(guard);
        if let Some(during) = &self.during {
            during(plan);
        }
        report(self.failed)
    }
    fn stop(&self, plan: &Value) -> Value {
        self.stops.set(self.stops.get() + 1);
        let _guard = WorkspaceGuard::execution_maintenance(
            Path::new(text(plan, "root").unwrap()),
            Duration::ZERO,
        )
        .unwrap();
        if let Some(during) = &self.during {
            during(plan);
        }
        let mut result = report(self.failed);
        result
            .set_field("operation", string("runtime_stop"))
            .unwrap();
        result
            .set_field("stop_command_attempted", Value::Bool(true))
            .unwrap();
        result
            .set_field("stop_command_succeeded", Value::Bool(!self.failed))
            .unwrap();
        result
    }
    fn history(&self, _: &WorkspaceGuard, _: &str, _: &Value) -> Result<()> {
        if self.history_fails {
            Err(Error("injected_history_failure"))
        } else {
            Ok(())
        }
    }
}
struct Reviewed {
    id: String,
    manifest: String,
    checkpoint: String,
    decision: String,
}
fn reviewed(root: &Path) -> Reviewed {
    let preparation = parsed(prepare_with(root, true, &Fake::default()).unwrap());
    let id = text(&preparation, "action_id").unwrap().to_owned();
    let cp = text(&preparation, "checkpoint").unwrap();
    let view = oracle(
        root,
        "import json,sys;from pathlib import Path;from runner import checkpoints\nw=Path(sys.argv[1]); cp=json.load(sys.stdin); p=w/'checkpoints'/f'{cp}.md'; raw,view=checkpoints.prepare(cp,p.read_bytes(),'approved','reviewed',decision_id='eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',decided_at='now',source='cli');p.write_bytes(raw);print(json.dumps(view.to_dict()))",
        &string(cp).canonical().unwrap(),
    );
    let view = parse_authority(&view).unwrap();
    Reviewed {
        id,
        manifest: text(&preparation, "manifest_digest").unwrap().to_owned(),
        checkpoint: text(&view, "revision").unwrap().to_owned(),
        decision: text(get(&view, "decision").unwrap(), "decision_id")
            .unwrap()
            .to_owned(),
    }
}
fn submit_with(root: &Path, review: &Reviewed, io: &impl Io) -> Result<CommandResult> {
    submission::submit(
        root,
        &review.id,
        &review.manifest,
        &review.checkpoint,
        &review.decision,
        true,
        io,
    )
}
pub(super) fn activated() -> tempfile::TempDir {
    let temp = fixture();
    enable_with(temp.path(), true, &Fake::default()).unwrap();
    temp
}

#[test]
fn inventory_plan_checkpoint_and_historical_read_match_python_exactly() {
    let temp = activated();
    let root = temp.path();
    let prepared = parsed(prepare_with(root, true, &Fake::default()).unwrap());
    let id = text(&prepared, "action_id").unwrap();
    let bundle_raw = fs::read(root.join(format!("state/actions/{id}.json"))).unwrap();
    let bundle = parse_authority(&bundle_raw).unwrap();
    let oracle_bytes = oracle(
        root,
        "import json,sys;from pathlib import Path;from runner import coven,actions\nw=Path(sys.argv[1]);b=json.load(sys.stdin);i=coven.runtime_launch_inputs(w);assert i==b['inputs'];assert coven.frozen_launch_plan(actions.ActionStore(w).read_control(),b['action_id'],i)==b['plan'];sys.stdout.buffer.write(actions.checkpoint_bytes(b))",
        &bundle_raw,
    );
    assert_eq!(
        oracle_bytes,
        fs::read(root.join(format!("checkpoints/runtime-launch-{id}.md"))).unwrap()
    );
    let inspected = super::super::inspect(root, id).unwrap();
    assert_eq!(
        text(&inspected, "manifest_digest").unwrap(),
        text(&prepared, "manifest_digest").unwrap()
    );
    assert_eq!(
        get(&bundle, "inputs").unwrap().canonical().unwrap(),
        inputs::capture(root)
            .unwrap()
            .inventory
            .canonical()
            .unwrap()
    );
    assert!(
        fs::metadata(root.join("state/.runtime-transport.lock"))
            .unwrap()
            .is_file()
    );
}
#[test]
fn enable_idempotence_and_initial_uncertainty_preserve_activation() {
    let temp = fixture();
    let root = temp.path();
    let uncertain = Fake {
        uncertain_write: 1,
        ..Fake::default()
    };
    let first = parsed(enable_with(root, true, &uncertain).unwrap());
    assert_eq!(text(&first, "outcome").unwrap(), "durability_uncertain");
    let again = parsed(enable_with(root, true, &Fake::default()).unwrap());
    assert_eq!(
        get(&first, "control").unwrap().canonical().unwrap(),
        get(&again, "control").unwrap().canonical().unwrap()
    );
    assert!(!root.join("state/actions").exists());
}
#[test]
fn completed_failed_and_known_retries_never_execute_again() {
    for failed in [false, true] {
        let temp = activated();
        let root = temp.path();
        let review = reviewed(root);
        let io = Fake {
            failed,
            ..Fake::default()
        };
        let first = submit_with(root, &review, &io).unwrap();
        assert_eq!(first.exit_code, if failed { 1 } else { 0 });
        let first = parsed(first);
        let repeat = parsed(submit_with(root, &review, &io).unwrap());
        assert_eq!(io.executes.get(), 1);
        assert!(matches!(
            get(&repeat, "replayed").unwrap(),
            Value::Bool(true)
        ));
        assert_eq!(
            get(&first, "claim").unwrap().canonical().unwrap(),
            get(&repeat, "claim").unwrap().canonical().unwrap()
        );
        oracle(
            root,
            "import sys,json;from pathlib import Path;from runner import actions\ns=actions.ActionStore(Path(sys.argv[1]));s.read_control();s.read_bundle(json.load(sys.stdin))",
            &string(&review.id).canonical().unwrap(),
        );
    }
}
#[test]
fn known_retry_precedes_artifacts_config_environment_and_ref_conflicts() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake::default();
    submit_with(root, &review, &io).unwrap();
    fs::remove_file(root.join(format!("state/actions/{}.json", review.id))).unwrap();
    fs::write(
        root.join(format!("checkpoints/runtime-launch-{}.md", review.id)),
        b"revoked",
    )
    .unwrap();
    fs::remove_file(root.join("coven.json")).unwrap();
    let uncertain = Fake {
        confirm_uncertain: true,
        ..Fake::default()
    };
    assert_eq!(submit_with(root, &review, &uncertain).unwrap().exit_code, 2);
    assert_eq!(uncertain.executes.get(), 0);
    let repeat = parsed(submit_with(root, &review, &io).unwrap());
    assert_eq!(text(&repeat, "outcome").unwrap(), "transport_completed");
    for field in 0..3 {
        let (m, c, d) = (&review.manifest, &review.checkpoint, &review.decision);
        let m2 = format!("runtime-launch-manifest-v1:{}", "0".repeat(64));
        let c2 = format!("checkpoint-snapshot-v1:{}", "0".repeat(64));
        let error = submission::submit(
            root,
            &review.id,
            if field == 0 { &m2 } else { m },
            if field == 1 { &c2 } else { c },
            if field == 2 {
                "00000000000000000000000000000000"
            } else {
                d
            },
            true,
            &io,
        )
        .unwrap_err();
        assert_eq!(error.code(), "action_consumed_references_conflict");
    }
    assert_eq!(io.executes.get(), 1);
}
#[test]
fn changed_inventory_and_revocation_refuse_before_consumption() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake::default();
    for path in [
        "goal.md",
        "spec.md",
        "state/lead.json",
        "agents/alice.md",
        "prompts/alice.md",
        "prompts/alice.start.txt",
        "coven.toml",
    ] {
        let path = root.join(path);
        let original = fs::read(&path).ok();
        fs::write(
            &path,
            if path.ends_with("coven.toml") {
                b"# changed".as_slice()
            } else if path.ends_with("lead.json") {
                br#"{"agent":"alice","until":"now"}"#.as_slice()
            } else {
                b"changed".as_slice()
            },
        )
        .unwrap();
        assert!(submit_with(root, &review, &io).is_err());
        if let Some(bytes) = original {
            fs::write(path, bytes).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
    fs::write(
        root.join(format!("checkpoints/runtime-launch-{}.md", review.id)),
        b"Status: changes_requested\n",
    )
    .unwrap();
    assert!(submit_with(root, &review, &io).is_err());
    assert_eq!(io.executes.get(), 0);
    assert!(
        get(&enabled(root).unwrap().0, "claims")
            .unwrap()
            .as_object()
            .unwrap()
            .entries()
            .is_empty()
    );
}
#[test]
fn replacement_faults_distinguish_unconsumed_from_known_consumed() {
    for uncertain in [false, true] {
        let temp = activated();
        let root = temp.path();
        let review = reviewed(root);
        let io = Fake {
            fail_write: if uncertain { 0 } else { 1 },
            uncertain_write: if uncertain { 1 } else { 0 },
            ..Fake::default()
        };
        let attempt = submit_with(root, &review, &io);
        assert_eq!(io.executes.get(), 0);
        if uncertain {
            assert_eq!(
                text(&parsed(attempt.unwrap()), "outcome").unwrap(),
                "durability_uncertain"
            );
            let retry = Fake::default();
            let output = submit_with(root, &review, &retry).unwrap();
            assert_eq!(output.exit_code, 2);
            assert_eq!(retry.executes.get(), 0);
            assert!(prepare_with(root, true, &retry).is_err());
        } else {
            assert_eq!(attempt.unwrap_err().code(), "injected_replace_failure");
            let retry = Fake::default();
            assert_eq!(submit_with(root, &review, &retry).unwrap().exit_code, 0);
            assert_eq!(retry.executes.get(), 1);
        }
    }
}
#[test]
fn outcome_failure_keeps_observed_report_and_consumption() {
    for uncertain in [false, true] {
        let temp = activated();
        let root = temp.path();
        let review = reviewed(root);
        let io = Fake {
            fail_write: if uncertain { 0 } else { 2 },
            uncertain_write: if uncertain { 2 } else { 0 },
            ..Fake::default()
        };
        let output = submit_with(root, &review, &io).unwrap();
        assert_eq!(output.exit_code, 2);
        let output = parsed(output);
        assert_eq!(text(&output, "outcome").unwrap(), "outcome_unknown");
        assert_eq!(
            get(&output, "transport_report")
                .unwrap()
                .canonical()
                .unwrap(),
            report(false).canonical().unwrap()
        );
        assert_eq!(io.executes.get(), 1);
        let retry = Fake::default();
        submit_with(root, &review, &retry).unwrap();
        assert_eq!(retry.executes.get(), 0);
    }
}
#[test]
fn acknowledged_uncertainty_allows_only_new_action_and_late_outcome_keeps_head() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake {
        fail_write: 2,
        ..Fake::default()
    };
    let first = parsed(submit_with(root, &review, &io).unwrap());
    let claim = text(get(&first, "claim").unwrap(), "claim_id").unwrap();
    assert!(
        acknowledge_with(
            root,
            &review.id,
            "00000000000000000000000000000000",
            true,
            &io
        )
        .is_err()
    );
    let ack = parsed(acknowledge_with(root, &review.id, claim, true, &Fake::default()).unwrap());
    assert_eq!(
        text(get(&ack, "claim").unwrap(), "status").unwrap(),
        "outcome_unknown"
    );
    let fresh = reviewed(root);
    assert_ne!(fresh.id, review.id);
    let retry = Fake::default();
    assert_eq!(submit_with(root, &review, &retry).unwrap().exit_code, 2);
    assert_eq!(retry.executes.get(), 0);
    submit_with(root, &fresh, &retry).unwrap();
    assert_eq!(text(&enabled(root).unwrap().0, "head").unwrap(), fresh.id);
}
#[test]
fn transport_after_claim_uses_frozen_plan_and_late_ack_never_overwrites_new_head() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let newer = Arc::new(std::sync::Mutex::new(String::new()));
    let captured = newer.clone();
    let path = root.to_owned();
    let id = review.id.clone();
    let io = Fake {
        during: Some(Box::new(move |plan| {
            let (control, _) = enabled(&path).unwrap();
            let claim = get(get(&control, "claims").unwrap(), &id).unwrap();
            acknowledge_with(
                &path,
                &id,
                text(claim, "claim_id").unwrap(),
                true,
                &Fake::default(),
            )
            .unwrap();
            let fresh = reviewed(&path);
            submit_with(&path, &fresh, &Fake::default()).unwrap();
            *captured.lock().unwrap() = fresh.id;
            fs::remove_file(path.join("coven.json")).unwrap();
            assert_eq!(text(plan, "root").unwrap(), path.to_str().unwrap());
        })),
        ..Fake::default()
    };
    let output = parsed(submit_with(root, &review, &io).unwrap());
    assert_eq!(text(&output, "outcome").unwrap(), "transport_completed");
    let (control, _) = enabled(root).unwrap();
    assert_eq!(text(&control, "head").unwrap(), *newer.lock().unwrap());
    assert!(matches!(
        get(get(&output, "claim").unwrap(), "operator_acknowledged").unwrap(),
        Value::Bool(true)
    ));
}
#[test]
fn history_failure_warns_after_terminal_outcome() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake {
        history_fails: true,
        ..Fake::default()
    };
    let output = submit_with(root, &review, &io).unwrap();
    assert_eq!(output.exit_code, 0);
    assert_eq!(
        get(&parsed(output), "warnings")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn publication_order_and_interrupted_preparation_do_not_adopt_or_execute() {
    let temp = activated();
    let root = temp.path();
    let io = Fake {
        fail_publish: 2,
        ..Fake::default()
    };
    assert!(prepare_with(root, true, &io).is_err());
    let paths = io.publications.borrow();
    assert_eq!(paths.len(), 2);
    assert!(paths[0].starts_with("state/actions"));
    assert!(paths[1].starts_with("checkpoints"));
    assert!(root.join(&paths[0]).is_file());
    assert!(!root.join(&paths[1]).exists());
    let fresh = parsed(prepare_with(root, true, &Fake::default()).unwrap());
    assert!(
        !paths[0]
            .to_str()
            .unwrap()
            .contains(text(&fresh, "action_id").unwrap())
    );
    assert_eq!(io.executes.get(), 0);
}
#[test]
fn stop_uses_consumed_plan_without_config_bundle_checkpoint_or_reset() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake::default();
    let first = parsed(submit_with(root, &review, &io).unwrap());
    let frozen = get(get(&first, "claim").unwrap(), "plan")
        .unwrap()
        .canonical()
        .unwrap();
    fs::remove_file(root.join("coven.json")).unwrap();
    fs::remove_dir_all(root.join("state/actions")).unwrap();
    fs::remove_dir_all(root.join("checkpoints")).unwrap();
    let before = fs::read(root.join(CONTROL)).unwrap();
    let io = Fake {
        during: Some(Box::new(move |plan| {
            assert_eq!(plan.canonical().unwrap(), frozen)
        })),
        ..Fake::default()
    };
    assert_eq!(
        stop_with(root, Some(&review.id), true, &io)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(stop_with(root, None, true, &io).unwrap().exit_code, 0);
    assert_eq!(io.stops.get(), 2);
    assert_eq!(fs::read(root.join(CONTROL)).unwrap(), before);
}
#[test]
fn simultaneous_submitters_consume_once() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let barrier = Arc::new(Barrier::new(2));
    let count = Arc::new(AtomicUsize::new(0));
    struct Concurrent(Arc<AtomicUsize>);
    impl Io for Concurrent {
        fn execute(&self, plan: &Value) -> Value {
            let _guard =
                WorkspaceGuard::acquire(Path::new(text(plan, "root").unwrap()), TIMEOUT).unwrap();
            self.0.fetch_add(1, Ordering::SeqCst);
            report(false)
        }
        fn history(&self, _: &WorkspaceGuard, _: &str, _: &Value) -> Result<()> {
            Ok(())
        }
    }
    std::thread::scope(|scope| {
        let mut handles = vec![];
        for _ in 0..2 {
            let barrier = barrier.clone();
            let count = count.clone();
            let review = &review;
            handles.push(scope.spawn(move || {
                barrier.wait();
                parsed(submit_with(root, review, &Concurrent(count)).unwrap())
            }));
        }
        let results = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            get(get(&results[0], "claim").unwrap(), "claim_id")
                .unwrap()
                .canonical()
                .unwrap(),
            get(get(&results[1], "claim").unwrap(), "claim_id")
                .unwrap()
                .canonical()
                .unwrap()
        );
        assert_eq!(
            results
                .iter()
                .filter(|v| matches!(get(v, "replayed").unwrap(), Value::Bool(false)))
                .count(),
            1
        );
    });
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn activation_waits_for_shared_admission_without_holding_workspace_lock() {
    let temp = fixture();
    let root = temp.path();
    let shared = crate::transport_gate::TransportGuard::shared(root, Duration::ZERO).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let child = scope.spawn(move || {
            send.send(()).unwrap();
            enable_with(root, true, &Fake::default()).unwrap()
        });
        receive.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!root.join(CONTROL).exists());
        let guard = WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
        drop(guard);
        drop(shared);
        assert_eq!(child.join().unwrap().exit_code, 0);
    });
}

#[test]
fn crash_child() {
    let Ok(root) = std::env::var("COVEN_ACTION_CRASH_ROOT") else {
        return;
    };
    let review = Reviewed {
        id: std::env::var("COVEN_ACTION_CRASH_ID").unwrap(),
        manifest: std::env::var("COVEN_ACTION_CRASH_MANIFEST").unwrap(),
        checkpoint: std::env::var("COVEN_ACTION_CRASH_CHECKPOINT").unwrap(),
        decision: std::env::var("COVEN_ACTION_CRASH_DECISION").unwrap(),
    };
    let io = Fake {
        during: Some(Box::new(|_| std::process::exit(73))),
        ..Fake::default()
    };
    submit_with(Path::new(&root), &review, &io).unwrap();
    panic!("fresh executor did not exit child");
}
#[test]
fn process_death_after_claim_is_permanently_consumed() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "actions::mutation::tests::crash_child",
            "--exact",
            "--nocapture",
        ])
        .env("COVEN_ACTION_CRASH_ROOT", root)
        .env("COVEN_ACTION_CRASH_ID", &review.id)
        .env("COVEN_ACTION_CRASH_MANIFEST", &review.manifest)
        .env("COVEN_ACTION_CRASH_CHECKPOINT", &review.checkpoint)
        .env("COVEN_ACTION_CRASH_DECISION", &review.decision)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(73),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let retry = Fake::default();
    let output = parsed(submit_with(root, &review, &retry).unwrap());
    assert_eq!(text(&output, "outcome").unwrap(), "claimed");
    assert_eq!(retry.executes.get(), 0);
    assert!(prepare_with(root, true, &retry).is_err());
}

#[test]
fn imported_task_authority_and_projection_health_match_oracle() {
    let temp = fixture();
    let root = temp.path();
    oracle(
        root,
        "import sys;from pathlib import Path;from runner import coven\nw=Path(sys.argv[1]);r=coven.read_task_snapshot(w)[0];coven.main(['state','import-tasks',str(w),'--expect-revision',r,'--json'])",
        b"",
    );
    enable_with(root, true, &Fake::default()).unwrap();
    let captured = inputs::capture(root).unwrap();
    let python = oracle(
        root,
        "import sys,json;from pathlib import Path;from runner import coven;print(json.dumps(coven.runtime_launch_inputs(Path(sys.argv[1]))))",
        b"",
    );
    assert_eq!(
        captured.inventory.canonical().unwrap(),
        parse_authority(&python).unwrap().canonical().unwrap()
    );
    fs::write(root.join("state/tasks.json"), b"[ ]\n").unwrap();
    assert_eq!(
        inputs::capture(root).err().unwrap().code(),
        "action_task_projection_unhealthy"
    );
}

#[test]
fn prepared_task_execution_control_is_not_a_runtime_inventory_input() {
    let temp = activated();
    let root = temp.path();
    let before = inputs::capture(root)
        .unwrap()
        .inventory
        .canonical()
        .unwrap();
    fs::create_dir(root.join("state/execution")).unwrap();
    fs::write(
        root.join("state/execution/control.json"),
        b"not startup authority",
    )
    .unwrap();
    assert_eq!(
        before,
        inputs::capture(root)
            .unwrap()
            .inventory
            .canonical()
            .unwrap()
    );
}

#[test]
fn empty_missing_invalid_utf8_prompts_and_pending_import_are_not_preparable() {
    let temp = activated();
    let root = temp.path();
    for raw in [b"".as_slice(), b" \t\n", b"\xff"] {
        fs::write(root.join("prompts/alice.md"), raw).unwrap();
        assert!(prepare_with(root, true, &Fake::default()).is_err());
        assert!(!root.join("state/actions").exists());
    }
    fs::write(root.join("prompts/alice.md"), b"role").unwrap();
    fs::create_dir(root.join("state/task-ledger")).unwrap();
    assert!(prepare_with(root, true, &Fake::default()).is_err());
}

#[cfg(unix)]
#[test]
fn malformed_and_unsafe_authority_never_executes_or_adopts() {
    use std::os::unix::fs::symlink;
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let io = Fake::default();
    let bundle = root.join(format!("state/actions/{}.json", review.id));
    let original = fs::read(&bundle).unwrap();
    for raw in [
        b"null".as_slice(),
        br#"{"version":1,"version":1}"#,
        b"{bad}",
    ] {
        fs::write(&bundle, raw).unwrap();
        assert!(submit_with(root, &review, &io).is_err());
    }
    fs::remove_file(&bundle).unwrap();
    symlink("/does-not-exist", &bundle).unwrap();
    assert!(submit_with(root, &review, &io).is_err());
    fs::remove_file(&bundle).unwrap();
    fs::write(&bundle, original).unwrap();
    let control = root.join(CONTROL);
    fs::remove_file(&control).unwrap();
    assert!(enable_with(root, true, &io).is_err());
    assert_eq!(io.executes.get(), 0);
}

#[test]
fn reapproving_an_edited_scope_cannot_authorize_the_bundle() {
    let temp = activated();
    let root = temp.path();
    let mut review = reviewed(root);
    let view = oracle(
        root,
        "import sys,json;from pathlib import Path;from runner import checkpoints\nw=Path(sys.argv[1]);i=json.load(sys.stdin);cp='runtime-launch-'+i;p=w/'checkpoints'/f'{cp}.md';original=p.read_bytes();raw,view=checkpoints.prepare(cp,original.replace(b'unrestricted',b'read-only'),'approved','changed scope',decision_id='ffffffffffffffffffffffffffffffff',decided_at='now',source='cli');p.write_bytes(raw);print(json.dumps(view.to_dict()))",
        &string(&review.id).canonical().unwrap(),
    );
    let view = parse_authority(&view).unwrap();
    review.checkpoint = text(&view, "revision").unwrap().to_owned();
    review.decision = text(get(&view, "decision").unwrap(), "decision_id")
        .unwrap()
        .to_owned();
    let io = Fake::default();
    assert_eq!(
        submit_with(root, &review, &io).unwrap_err().code(),
        "action_checkpoint_not_exact_bound_approval"
    );
    assert_eq!(io.executes.get(), 0);
}

#[test]
fn checkpoint_directory_capacity_refuses_before_immutable_bundle_publication() {
    let temp = activated();
    let root = temp.path();
    for index in 0..4096 {
        fs::write(root.join(format!("checkpoints/{index}.ignored")), b"").unwrap();
    }
    let io = Fake::default();
    assert_eq!(
        prepare_with(root, true, &io).unwrap_err().code(),
        "unsupported_action_writer_checkpoint_capacity"
    );
    assert!(io.publications.borrow().is_empty());
    assert!(!root.join("state/actions").exists());
}

#[cfg(unix)]
#[test]
fn immutable_publication_fault_io_preserves_visibility_and_never_adopts() {
    use std::fs::File;
    struct Fault(&'static str);
    impl storage::EvidenceIo for Fault {
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            if self.0 == "write" {
                return Err(std::io::Error::other("write"));
            }
            file.write_all(bytes)
        }
        fn sync_file(&self, file: &File) -> std::io::Result<()> {
            if self.0 == "file_sync" {
                return Err(std::io::Error::other("file_sync"));
            }
            file.sync_all()
        }
        fn link(&self, source: &Path, target: &Path) -> std::io::Result<()> {
            if self.0 == "link" {
                return Err(std::io::Error::other("link"));
            }
            fs::hard_link(source, target)
        }
        fn sync_dir(&self, path: &Path) -> std::io::Result<()> {
            if self.0 == "dir_sync" {
                return Err(std::io::Error::other("dir_sync"));
            }
            platform::sync_directory(path)
        }
    }
    struct PublicationFault(&'static str);
    impl Io for PublicationFault {
        fn publish(
            &self,
            guard: &WorkspaceGuard,
            relative: &Path,
            bytes: &[u8],
        ) -> Result<Delivery> {
            storage::publish_new_with(guard, relative, bytes, &Fault(self.0))
        }
    }
    for fault in ["write", "file_sync", "link", "dir_sync"] {
        let temp = activated();
        let root = temp.path();
        assert!(prepare_with(root, true, &PublicationFault(fault)).is_err());
        let bundles = fs::read_dir(root.join("state/actions"))
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(bundles.len(), usize::from(fault == "dir_sync"));
        assert!(
            get(&enabled(root).unwrap().0, "claims")
                .unwrap()
                .as_object()
                .unwrap()
                .entries()
                .is_empty()
        );
        assert_eq!(fs::read_dir(root.join("checkpoints")).unwrap().count(), 0);
        let fresh = parsed(prepare_with(root, true, &Fake::default()).unwrap());
        if let Some(orphan) = bundles.first() {
            assert_ne!(
                orphan.path().file_stem().unwrap().to_str().unwrap(),
                text(&fresh, "action_id").unwrap()
            );
        }
    }
}

#[test]
fn environment_child() {
    let Ok(root) = std::env::var("COVEN_ACTION_ENV_ROOT") else {
        return;
    };
    let review = Reviewed {
        id: std::env::var("COVEN_ACTION_ENV_ID").unwrap(),
        manifest: std::env::var("COVEN_ACTION_ENV_MANIFEST").unwrap(),
        checkpoint: std::env::var("COVEN_ACTION_ENV_CHECKPOINT").unwrap(),
        decision: std::env::var("COVEN_ACTION_ENV_DECISION").unwrap(),
    };
    let io = Fake::default();
    let attempt = submit_with(Path::new(&root), &review, &io);
    if std::env::var("COVEN_ACTION_ENV_KNOWN").is_ok() {
        assert_eq!(attempt.unwrap().exit_code, 0);
    } else {
        assert_eq!(
            attempt.unwrap_err().code(),
            "action_captured_inputs_changed"
        );
    }
    assert_eq!(io.executes.get(), 0);
}
#[test]
fn effective_environment_selection_changes_block_fresh_but_not_known_consumption() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    for known in [false, true] {
        if known {
            submit_with(root, &review, &Fake::default()).unwrap();
        }
        for (key, value) in [
            ("TERM", "coven-action-test-changed-term"),
            (
                "COVEN_MULTIPLEXER",
                if std::env::var("COVEN_MULTIPLEXER").is_ok_and(|v| v == "tmux") {
                    "herdr"
                } else {
                    "tmux"
                },
            ),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "actions::mutation::tests::environment_child",
                    "--exact",
                    "--nocapture",
                ])
                .env("COVEN_ACTION_ENV_ROOT", root)
                .env("COVEN_ACTION_ENV_ID", &review.id)
                .env("COVEN_ACTION_ENV_MANIFEST", &review.manifest)
                .env("COVEN_ACTION_ENV_CHECKPOINT", &review.checkpoint)
                .env("COVEN_ACTION_ENV_DECISION", &review.decision)
                .env(key, value);
            if known {
                child.env("COVEN_ACTION_ENV_KNOWN", "1");
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn acknowledgement_faults_accept_uncertainty_without_reset_or_transport() {
    for uncertain in [false, true] {
        let temp = activated();
        let root = temp.path();
        let review = reviewed(root);
        let output = parsed(
            submit_with(
                root,
                &review,
                &Fake {
                    fail_write: 2,
                    ..Fake::default()
                },
            )
            .unwrap(),
        );
        let claim = text(get(&output, "claim").unwrap(), "claim_id").unwrap();
        let io = Fake {
            fail_write: if uncertain { 0 } else { 1 },
            uncertain_write: if uncertain { 1 } else { 0 },
            ..Fake::default()
        };
        let ack = acknowledge_with(root, &review.id, claim, true, &io);
        if uncertain {
            assert_eq!(ack.unwrap().exit_code, 2);
            assert!(prepare_with(root, true, &Fake::default()).is_ok());
        } else {
            assert_eq!(ack.unwrap_err().code(), "injected_replace_failure");
            assert!(prepare_with(root, true, &Fake::default()).is_err());
        }
        let retry = Fake::default();
        assert_eq!(submit_with(root, &review, &retry).unwrap().exit_code, 2);
        assert_eq!(retry.executes.get(), 0);
        assert_eq!(io.stops.get(), 0);
    }
}

#[cfg(unix)]
#[test]
fn consumed_control_operations_ignore_even_unsafe_current_config() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let first = parsed(
        submit_with(
            root,
            &review,
            &Fake {
                fail_write: 2,
                ..Fake::default()
            },
        )
        .unwrap(),
    );
    let claim = text(get(&first, "claim").unwrap(), "claim_id").unwrap();
    fs::remove_file(root.join("coven.json")).unwrap();
    std::os::unix::fs::symlink("/does-not-exist", root.join("coven.json")).unwrap();
    let io = Fake::default();
    assert_eq!(submit_with(root, &review, &io).unwrap().exit_code, 2);
    assert_eq!(
        acknowledge_with(root, &review.id, claim, true, &io)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(
        stop_with(root, Some(&review.id), true, &io)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(io.executes.get(), 0);
}

#[test]
fn native_unknown_acknowledgement_and_known_retry_match_python_transitions() {
    let temp = activated();
    let root = temp.path();
    let review = reviewed(root);
    let output = parsed(
        submit_with(
            root,
            &review,
            &Fake {
                fail_write: 2,
                ..Fake::default()
            },
        )
        .unwrap(),
    );
    let claim_id = text(get(&output, "claim").unwrap(), "claim_id").unwrap();
    let before = fs::read(root.join(CONTROL)).unwrap();
    let native =
        parsed(acknowledge_with(root, &review.id, claim_id, true, &Fake::default()).unwrap());
    let native_control = fs::read(root.join(CONTROL)).unwrap();
    // Reuse precisely the same consumed identities, on a disposable oracle root.
    fs::write(root.join(CONTROL), before).unwrap();
    let request = object(vec![
        ("id", string(&review.id)),
        ("claim", string(claim_id)),
        ("manifest", string(&review.manifest)),
        ("checkpoint", string(&review.checkpoint)),
        ("decision", string(&review.decision)),
    ])
    .unwrap();
    let python = oracle(
        root,
        "import sys,json;from pathlib import Path;from runner import actions,storage\nw=Path(sys.argv[1]);r=json.load(sys.stdin);s=actions.ActionStore(w)\nwith storage.workspace_lock(w,w/'state/.workspace.lock') as lock:\n a=s.acknowledge_unknown(r['id'],r['claim'],lock);b,replayed=s.claim(r['id'],r['manifest'],r['checkpoint'],r['decision'],{},lock);assert a==b and replayed;print(json.dumps(a))",
        &request.canonical().unwrap(),
    );
    assert_eq!(
        get(&native, "claim").unwrap().canonical().unwrap(),
        parse_authority(&python).unwrap().canonical().unwrap()
    );
    assert_eq!(native_control, fs::read(root.join(CONTROL)).unwrap());
    let retry = Fake::default();
    let repeated = parsed(submit_with(root, &review, &retry).unwrap());
    assert_eq!(
        get(&repeated, "claim").unwrap().canonical().unwrap(),
        parse_authority(&python).unwrap().canonical().unwrap()
    );
    assert_eq!(retry.executes.get(), 0);
}
