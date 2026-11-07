use super::*;
use std::cell::RefCell;

fn minimal() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("coven.json"),
        br#"{"name":"Test team","agents":["operator","Reviewer"],"multiplexer":"tmux"}"#,
    )
    .unwrap();
    temp
}

fn error<T>(result: Result<T>) -> &'static str {
    result.err().expect("expected refusal").code()
}

fn lock_busy(root: &Path) {
    assert_eq!(
        error(WorkspaceGuard::existing(root, Duration::ZERO)),
        "workspace_lock_busy"
    );
}

#[test]
fn disposable_prepare_and_success_preserve_authored_state() {
    let temp = minimal();
    let root = temp.path();
    fs::create_dir(root.join("prompts")).unwrap();
    fs::write(root.join("prompts/operator.md"), b"custom\xff").unwrap();
    fs::write(root.join("goal.md"), "# Goal\r\n\r\nA goal\r").unwrap();
    let before = fs::read(root.join("coven.json")).unwrap();
    let prepared = prepare(root).unwrap();
    assert_eq!(prepared.missing.len(), 1);
    assert!(prepared.dashboard.contains("\nA goal\n"));
    assert!(!root.join("state").exists());
    assert!(!root.join("tmux").exists());
    let output = refresh(root).unwrap();
    let runtime = workspace::load(root).unwrap().multiplexer;
    assert_eq!(
        output,
        format!("[INFO] Regenerated tmux/coven.toml and starter prompt files (runtime={runtime})")
    );
    assert_eq!(fs::read(root.join("coven.json")).unwrap(), before);
    assert_eq!(
        fs::read(root.join("prompts/operator.md")).unwrap(),
        b"custom\xff"
    );
    assert!(!root.join("state/tasks.json").exists());
    assert!(!root.join("logs").exists());
    assert_eq!(
        fs::read_to_string(root.join("dashboard.md")).unwrap(),
        prepared.dashboard
    );
    assert_eq!(
        fs::read_to_string(root.join("tmux/coven.toml")).unwrap(),
        prepared.manifest
    );
    assert!(prepared.manifest.contains("bin/coven"));
}

#[test]
fn deterministic_failures_do_not_create_first_lock_or_outputs() {
    for (relative, content) in [
        ("goal.md", vec![0xff]),
        ("logs/messages.jsonl", b"{}".to_vec()),
        ("logs/events.jsonl", b"[]\n".to_vec()),
        ("logs/messages.jsonl", b"{\"checkpoint\":null}\n".to_vec()),
        (
            "logs/messages.jsonl",
            b"{\"checkpoint\":\"../outside\"}\n".to_vec(),
        ),
    ] {
        let temp = minimal();
        let path = temp.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        assert!(refresh(temp.path()).is_err(), "{relative}");
        assert!(!temp.path().join("state/.workspace.lock").exists());
        assert!(!temp.path().join("tmux").exists());
    }
}

#[test]
fn logs_validate_batches_then_all_references_messages_before_events() {
    let temp = minimal();
    let root = temp.path();
    fs::create_dir(root.join("logs")).unwrap();
    fs::write(
        root.join("logs/messages.jsonl"),
        b"{\"checkpoint\":null}\n[]\n",
    )
    .unwrap();
    fs::write(root.join("logs/events.jsonl"), b"broken\n").unwrap();
    assert_eq!(error(checkpoint_logs(root)), "invalid_jsonl_record");
    fs::write(root.join("logs/messages.jsonl"), b"{\"checkpoint\":null}\n").unwrap();
    assert_eq!(
        error(checkpoint_logs(root)),
        "invalid_checkpoint_identifier"
    );
    for id in ["", "../outside", "with.dot", "/absolute"] {
        fs::write(
            root.join("logs/messages.jsonl"),
            format!(
                "{{\"checkpoint\":{}}}\n",
                serde_json::to_string(id).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(
            error(checkpoint_logs(root)),
            "invalid_checkpoint_identifier"
        );
    }
    fs::write(
        root.join("logs/messages.jsonl"),
        b"{\"checkpoint\":\"missing\",\"type\":\"not-a-request\"}\r\n\r\n",
    )
    .unwrap();
    assert_eq!(error(checkpoint_logs(root)), "invalid_jsonl_record"); // now reaches events
    fs::write(root.join("logs/events.jsonl"), b"{}\n").unwrap();
    checkpoint_logs(root).unwrap();
    fs::create_dir(root.join("checkpoints")).unwrap();
    fs::write(root.join("checkpoints/missing.md"), [0xff]).unwrap();
    assert_eq!(error(checkpoint_logs(root)), "invalid_utf8");
    fs::write(root.join("checkpoints/missing.md"), "one\r\ntwo\rthree").unwrap();
    assert_eq!(
        optional_text(root, "checkpoints/missing.md").unwrap(),
        "one\ntwo\nthree"
    );
}

struct Changes {
    phase: &'static str,
}
impl RefreshIo for Changes {
    fn observe(&self, phase: &str, root: &Path) -> Result<()> {
        if phase == self.phase {
            fs::write(root.join("coven.json"), "null")?;
        }
        Ok(())
    }
}

#[test]
fn outer_held_nested_and_locked_prepare_preflights_are_effective() {
    for phase in [
        "draft",
        "outer",
        "locked",
        "outer-held",
        "nested-before",
        "nested",
    ] {
        let temp = minimal();
        assert_eq!(
            error(run(temp.path(), &Changes { phase })),
            "invalid_team_record"
        );
        assert_eq!(
            temp.path().join("state/.workspace.lock").exists(),
            phase != "draft"
        );
        assert!(!temp.path().join("tmux").exists());
        assert!(!temp.path().join("dashboard.md").exists());
    }
}

struct Fresh;
impl RefreshIo for Fresh {
    fn observe(&self, phase: &str, root: &Path) -> Result<()> {
        if phase == "draft" {
            let _guard = WorkspaceGuard::acquire(root, Duration::ZERO)?;
            fs::write(root.join("goal.md"), "# Goal\nNew locked goal")?;
        }
        if matches!(phase, "locked" | "nested" | "prepared") {
            lock_busy(root);
        }
        Ok(())
    }
    fn write(&self, root: &Path, relative: &str, text: &str, missing: bool) -> Result<()> {
        lock_busy(root);
        direct_write(root, relative, text, missing)
    }
    fn dashboard(&self, guard: &WorkspaceGuard, text: &str) -> Result<Delivery> {
        lock_busy(guard.root());
        assert!(text.contains("New locked goal"));
        SystemIo.dashboard(guard, text)
    }
}

#[test]
fn fresh_prepare_and_all_publication_are_inside_same_guard() {
    let temp = minimal();
    fs::write(temp.path().join("goal.md"), "old goal").unwrap();
    run(temp.path(), &Fresh).unwrap();
    WorkspaceGuard::existing(temp.path(), Duration::ZERO).unwrap();
}

struct FailPublication {
    fail: usize,
    seen: RefCell<Vec<String>>,
}
impl RefreshIo for FailPublication {
    fn write(&self, root: &Path, relative: &str, text: &str, missing: bool) -> Result<()> {
        let mut seen = self.seen.borrow_mut();
        seen.push(relative.into());
        if seen.len() == self.fail {
            return Err(Error("injected_publication_failure"));
        }
        direct_write(root, relative, text, missing)
    }
    fn dashboard(&self, _: &WorkspaceGuard, _: &str) -> Result<Delivery> {
        self.seen.borrow_mut().push("dashboard.md".into());
        Err(Error("injected_publication_failure"))
    }
}

#[test]
fn publication_is_ordered_partial_and_dashboard_last() {
    let expected = [
        "tmux/coven.toml",
        "prompts/operator.md",
        "prompts/Reviewer.md",
        "prompts/operator.start.txt",
        "prompts/Reviewer.start.txt",
        "dashboard.md",
    ];
    for fail in 1..=6 {
        let temp = minimal();
        fs::write(temp.path().join("dashboard.md"), "old dashboard").unwrap();
        let io = FailPublication {
            fail,
            seen: RefCell::new(Vec::new()),
        };
        assert_eq!(error(run(temp.path(), &io)), "injected_publication_failure");
        assert_eq!(*io.seen.borrow(), &expected[..fail]);
        assert_eq!(
            fs::read_to_string(temp.path().join("dashboard.md")).unwrap(),
            "old dashboard"
        );
        if fail > 1 {
            assert!(temp.path().join("tmux/coven.toml").is_file());
        }
    }
}

#[test]
fn assets_and_bounded_helpers_refuse_explicitly() {
    assert_eq!(
        error(example_directory(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Some("no-such-native-refresh-example")
        )),
        "refresh_example_assets_unavailable"
    );
    let mut total = MAX_OUTPUT;
    assert_eq!(
        error(admit(&mut total, 1)),
        "unsupported_refresh_output_profile"
    );
    assert_eq!(total, MAX_OUTPUT);
    let temp = minimal();
    fs::write(temp.path().join("goal.md"), vec![b'x'; MAX_TEXT + 1]).unwrap();
    assert_eq!(error(optional_text(temp.path(), "goal.md")), PROFILE);
    assert_eq!(templates::slug("  A.!B__- ").unwrap(), "a-b__");
    assert!(templates::slug("_-_").is_err());
    assert_eq!(templates::shell_quote("a'b"), "'a'\"'\"'b'");
    assert_eq!(templates::shell_quote("/safe/path"), "/safe/path");
}

fn assert_refresh_refusal_preserves(root: &Path, code: &'static str, files: &[&str]) {
    let before: Vec<_> = files
        .iter()
        .map(|path| fs::read(root.join(path)).unwrap())
        .collect();
    assert!(!root.join("state/.workspace.lock").exists());
    assert_eq!(error(refresh(root)), code);
    assert!(!root.join("state/.workspace.lock").exists());
    for (path, bytes) in files.iter().zip(before) {
        assert_eq!(fs::read(root.join(path)).unwrap(), bytes, "{path}");
    }
}

#[test]
fn preparation_generated_output_budget_is_inclusive_and_one_byte_over_has_no_effects() {
    let temp = minimal();
    let root = temp.path();
    let agents: Vec<_> = (0..40).map(|index| format!("a{index:02}")).collect();
    let mut team = serde_json::json!({
        "name": "budget", "agents": agents, "multiplexer": "tmux", "opencode_command": "x"
    });
    fs::write(root.join("coven.json"), serde_json::to_vec(&team).unwrap()).unwrap();
    for directory in ["tmux", "prompts", "state"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    for (path, text) in [
        ("tmux/coven.toml", "old manifest"),
        ("prompts/a00.md", "authored role"),
        ("prompts/a00.start.txt", "old starter"),
        ("dashboard.md", "old dashboard"),
        ("state/tasks.json", "[]"),
        ("goal.md", "x"),
    ] {
        fs::write(root.join(path), text).unwrap();
    }
    let output_len = |prepared: &Prepared| {
        prepared.manifest.len()
            + prepared.dashboard.len()
            + prepared
                .missing
                .iter()
                .chain(&prepared.starters)
                .map(|(_, text)| text.len())
                .sum::<usize>()
    };
    let base = prepare(root).unwrap();
    let available = MAX_OUTPUT - output_len(&base);
    let agent_refs: Vec<_> = agents.iter().map(String::as_str).collect();
    // A simple command grows once in vars and once per agent. Derive the
    // coefficient rather than hard-coding manifest/path/CLI overhead.
    let slope = templates::manifest(
        "budget",
        root.to_str().unwrap(),
        &agent_refs,
        "xx",
        &launch_cli().unwrap(),
    )
    .unwrap()
    .len()
        - base.manifest.len();
    assert_eq!(slope, 41);
    drop(base);
    let command = "x".repeat(1 + available / slope);
    assert!(command.len() < MAX_TEXT);
    team["opencode_command"] = command.into();
    fs::write(root.join("coven.json"), serde_json::to_vec(&team).unwrap()).unwrap();
    let mut goal = "x".repeat(1 + available % slope);
    fs::write(root.join("goal.md"), &goal).unwrap();
    let exact = prepare(root).unwrap();
    assert!(exact.manifest.len() < MAX_OUTPUT);
    assert_eq!(exact.starters.len(), 40);
    assert_eq!(exact.missing.len(), 39);
    assert_eq!(output_len(&exact), MAX_OUTPUT);
    drop(exact);
    assert!(!root.join("state/.workspace.lock").exists());

    // Only the final dashboard grows: the manifest still fits independently.
    goal.push('x');
    fs::write(root.join("goal.md"), goal).unwrap();
    assert_refresh_refusal_preserves(
        root,
        "unsupported_refresh_output_profile",
        &[
            "coven.json",
            "goal.md",
            "state/tasks.json",
            "tmux/coven.toml",
            "prompts/a00.md",
            "prompts/a00.start.txt",
            "dashboard.md",
        ],
    );
    for agent in &agents[1..] {
        assert!(!root.join(format!("prompts/{agent}.md")).exists());
        assert!(!root.join(format!("prompts/{agent}.start.txt")).exists());
    }
    assert!(!root.join("logs").exists());
}

#[test]
fn preparation_checkpoint_work_budget_is_inclusive_and_overages_have_no_effects() {
    let temp = minimal();
    let root = temp.path();
    for directory in ["checkpoints", "logs", "tmux", "prompts"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    for (path, text) in [
        ("tmux/coven.toml", "old manifest"),
        ("prompts/operator.md", "authored role"),
        ("prompts/operator.start.txt", "old starter"),
        ("dashboard.md", "old dashboard"),
        ("goal.md", "authored goal"),
        ("checkpoints/byte.md", "x"),
    ] {
        fs::write(root.join(path), text).unwrap();
    }
    // Reuse one safe file, not 256 MiB of stored fixtures. Non-request records
    // must consume work too, and the budget is shared across the two logs.
    fs::write(root.join("checkpoints/budget.md"), vec![b'x'; MAX_TEXT]).unwrap();
    let record = "{\"checkpoint\":\"budget\",\"type\":\"unrelated\"}\n";
    let half = record.repeat(128);
    fs::write(root.join("logs/messages.jsonl"), &half).unwrap();
    fs::write(root.join("logs/events.jsonl"), &half).unwrap();
    prepare(root).unwrap(); // Exactly 256 * 1 MiB must be accepted.
    assert!(!root.join("state").exists());

    for last in ["{\"checkpoint\":\"byte\"}\n", record] {
        fs::write(root.join("logs/events.jsonl"), format!("{half}{last}")).unwrap();
        // Test both exactly one byte over and a 257th full-size reference.
        assert_refresh_refusal_preserves(
            root,
            "unsupported_refresh_checkpoint_work_profile",
            &[
                "coven.json",
                "goal.md",
                "tmux/coven.toml",
                "dashboard.md",
                "prompts/operator.md",
                "prompts/operator.start.txt",
                "logs/messages.jsonl",
                "logs/events.jsonl",
                "checkpoints/budget.md",
                "checkpoints/byte.md",
            ],
        );
        assert!(!root.join("state").exists());
        assert!(!root.join("prompts/Reviewer.md").exists());
        assert!(!root.join("prompts/Reviewer.start.txt").exists());
    }
}

#[test]
fn pending_import_is_not_an_automatic_refresh_blocker() {
    let temp = minimal();
    fs::create_dir_all(temp.path().join("state/task-ledger")).unwrap();
    fs::write(temp.path().join("state/tasks.json"), "[]").unwrap();
    refresh(temp.path()).unwrap();
    assert!(!temp.path().join("state/task-authority.json").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("state/tasks.json")).unwrap(),
        "[]"
    );
}

#[cfg(unix)]
#[test]
fn direct_outputs_preserve_inode_mode_and_partial_failures() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let temp = minimal();
    let root = temp.path();
    fs::create_dir(root.join("tmux")).unwrap();
    let path = root.join("tmux/coven.toml");
    fs::write(&path, "old long manifest").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let before = fs::metadata(&path).unwrap();
    direct_write(root, "tmux/coven.toml", "new", false).unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!((before.ino(), before.mode()), (after.ino(), after.mode()));
    assert_eq!(fs::read(&path).unwrap(), b"new");
    struct Partial;
    impl DirectIo for Partial {
        fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
            file.write_all(&bytes[..2])?;
            Err(std::io::Error::other("injected partial write"))
        }
    }
    assert!(direct_with(root, "tmux/coven.toml", "partial", false, &Partial).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"pa");
    assert_eq!(fs::metadata(&path).unwrap().ino(), before.ino());
    direct_write(root, "tmux/coven.toml", "not written", true).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"pa");
}

#[cfg(unix)]
#[test]
fn unsafe_outputs_and_checkpoint_references_never_follow_or_block() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let temp = minimal();
    let root = temp.path();
    fs::create_dir(root.join("checkpoints")).unwrap();
    fs::create_dir(root.join("logs")).unwrap();
    fs::write(
        root.join("logs/messages.jsonl"),
        "{\"checkpoint\":\"external\"}\n",
    )
    .unwrap();
    let target = root.join("checkpoints/external.md");
    let name = std::ffi::CString::new(target.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(error(checkpoint_logs(root)), "unsafe_archive_path");
    fs::remove_file(&target).unwrap();
    symlink(root.join("coven.json"), &target).unwrap();
    assert_eq!(error(checkpoint_logs(root)), "unsafe_archive_path");
    fs::create_dir(root.join("tmux")).unwrap();
    symlink(root.join("coven.json"), root.join("tmux/coven.toml")).unwrap();
    assert_eq!(
        error(direct_write(root, "tmux/coven.toml", "bad", false)),
        "unsafe_archive_path"
    );
}

#[test]
fn generated_templates_retain_coordination_rules_and_valid_toml() {
    let manifest = templates::manifest(
        "A team",
        "/a/space's/\u{1f600}",
        &["operator"],
        "oc --flag",
        "/source/bin/coven",
    )
    .unwrap();
    let parsed: toml::Value = manifest.parse().unwrap();
    let vars = parsed.get("vars").unwrap();
    assert_eq!(
        vars.get("COVEN_DIR").unwrap().as_str(),
        Some("/a/space's/\u{1f600}")
    );
    assert!(
        vars.get("OPENCODE_CMD")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("exec opencode --flag")
    );
    assert!(manifest.ends_with('\n') && !manifest.ends_with("\n\n"));
    assert!(
        templates::prompt("Reviewer", "/workspace", "\r\n context \n")
            .contains("Role: Review diffs")
    );
    assert!(templates::prompt("operator", "/workspace", " context ").ends_with("\n\ncontext\n\n"));
    let starter = templates::starter("operator", "/workspace");
    assert!(starter.starts_with("Read `/workspace/prompts/operator.md`"));
    assert!(starter.contains("same key\nand original arguments/revision"));
}

#[test]
fn example_manifest_is_an_object_and_context_is_read_even_with_existing_roles() {
    let temp = minimal();
    let root = temp.path();
    fs::write(
        root.join("coven.json"),
        br#"{"name":"test","agents":["operator"],"example":" Demo "}"#,
    )
    .unwrap();
    fs::create_dir(root.join("prompts")).unwrap();
    fs::write(root.join("prompts/operator.md"), "custom role").unwrap();
    let source = tempfile::tempdir().unwrap();
    let example = source.path().join("examples/demo");
    fs::create_dir_all(&example).unwrap();
    for raw in ["[]", "null", "1", "\"string\""] {
        fs::write(example.join("example.json"), raw).unwrap();
        assert_eq!(
            error(prepare_with_assets(root, source.path())),
            "invalid_example_manifest"
        );
    }
    fs::write(example.join("example.json"), "{}").unwrap();
    fs::write(example.join("prompt-context.md"), [0xff]).unwrap();
    assert_eq!(
        error(prepare_with_assets(root, source.path())),
        "invalid_utf8"
    );
    fs::write(example.join("prompt-context.md"), "\r\nExample context\r").unwrap();
    assert!(
        prepare_with_assets(root, source.path())
            .unwrap()
            .missing
            .is_empty()
    );
    fs::remove_file(root.join("prompts/operator.md")).unwrap();
    let prepared = prepare_with_assets(root, source.path()).unwrap();
    assert!(prepared.missing[0].1.ends_with("\n\nExample context\n\n"));
    assert!(!root.join("state/.workspace.lock").exists());
}

#[test]
fn dashboard_sync_uncertainty_keeps_the_published_dashboard_without_success() {
    struct Uncertain;
    impl RefreshIo for Uncertain {
        fn dashboard(&self, guard: &WorkspaceGuard, text: &str) -> Result<Delivery> {
            SystemIo.dashboard(guard, text)?;
            Ok(Delivery::DurabilityUncertain)
        }
    }
    let temp = minimal();
    fs::write(temp.path().join("dashboard.md"), "old").unwrap();
    assert_eq!(
        error(run(temp.path(), &Uncertain)),
        "dashboard_synchronization_uncertain"
    );
    assert!(
        fs::read_to_string(temp.path().join("dashboard.md"))
            .unwrap()
            .starts_with("# Coven Dashboard:")
    );
    assert!(temp.path().join("tmux/coven.toml").exists());
}
