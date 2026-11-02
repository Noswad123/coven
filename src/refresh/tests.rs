use super::*;
use std::cell::Cell;

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["architect","CUSTOM"]}"#,
    )
    .unwrap();
    root
}

#[test]
fn refresh_preserves_authored_bytes_and_updates_only_generated_files() {
    let root = workspace();
    fs::create_dir(root.path().join("prompts")).unwrap();
    fs::create_dir(root.path().join("logs")).unwrap();
    fs::create_dir(root.path().join("state")).unwrap();
    let authored = [
        ("prompts/architect.md", b"\xffcustom role".as_slice()),
        ("goal.md", b"# Goal\n\nPreserve me".as_slice()),
        (
            "logs/messages.jsonl",
            b"{\"body\":\"history\"}\n".as_slice(),
        ),
        ("logs/events.jsonl", b"{\"type\":\"custom\"}\n".as_slice()),
        ("state/tasks.json", b"[]".as_slice()),
    ];
    for (path, bytes) in authored {
        fs::write(root.path().join(path), bytes).unwrap();
    }
    let output = arguments(root.path()).unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(output.warnings.is_empty());
    for (path, bytes) in authored {
        assert_eq!(fs::read(root.path().join(path)).unwrap(), bytes);
    }
    assert!(root.path().join("prompts/CUSTOM.md").is_file());
    assert!(root.path().join("prompts/CUSTOM.start.txt").is_file());
    assert!(root.path().join("dashboard.md").is_file());
    assert!(!root.path().join("agents").exists());
    let before = fs::read(root.path().join("tmux/coven.toml")).unwrap();
    arguments(root.path()).unwrap();
    assert_eq!(
        fs::read(root.path().join("tmux/coven.toml")).unwrap(),
        before
    );
}

#[test]
fn deterministic_invalid_inputs_never_create_first_lock() {
    for (path, bytes) in [
        ("goal.md", b"\xff".as_slice()),
        ("logs/messages.jsonl", b"{\"checkpoint\":42}\n".as_slice()),
        (
            "logs/events.jsonl",
            b"{\"checkpoint\":\"../escape\"}\n".as_slice(),
        ),
        (
            "coven.json",
            br#"{"name":"test","agents":["a"],"example":"custom"}"#.as_slice(),
        ),
    ] {
        let root = workspace();
        if path.starts_with("logs/") {
            fs::create_dir(root.path().join("logs")).unwrap();
        }
        fs::write(root.path().join(path), bytes).unwrap();
        assert!(arguments(root.path()).is_err(), "{path}");
        assert!(!root.path().join("state/.workspace.lock").exists());
        assert!(!root.path().join("tmux").exists());
        assert!(!root.path().join("prompts").exists());
    }
}

struct Rebind;
impl RefreshIo for Rebind {
    fn prepared(&self, root: &Path, guarded: bool) -> Result<()> {
        if !guarded {
            fs::write(
                root.join("coven.json"),
                br#"{"name":"fresh","agents":["fresh"]}"#,
            )?;
        } else {
            fs::create_dir(root.join("prompts"))?;
            fs::write(root.join("prompts/fresh.md"), b"concurrent custom role")?;
        }
        Ok(())
    }
}
#[test]
fn guarded_preparation_rebinds_and_preserves_new_role_collision() {
    let root = workspace();
    assert_eq!(arguments_with(root.path(), &Rebind).unwrap().exit_code, 0);
    assert_eq!(
        fs::read(root.path().join("prompts/fresh.md")).unwrap(),
        b"concurrent custom role"
    );
    assert!(!root.path().join("prompts/architect.start.txt").exists());
    assert!(
        fs::read_to_string(root.path().join("tmux/coven.toml"))
            .unwrap()
            .contains("Coven: fresh")
    );
}

struct Fault {
    index: Cell<usize>,
    fail: usize,
    uncertain: bool,
}
impl RefreshIo for Fault {
    fn write(&self, file: &mut File, bytes: &[u8]) -> Result<()> {
        let index = self.index.get();
        self.index.set(index + 1);
        if index == self.fail {
            file.write_all(&bytes[..bytes.len().min(7)])?;
            return Err(Error("injected_generated_write"));
        }
        Ok(file.write_all(bytes)?)
    }
    fn dashboard(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
        if self.fail == self.index.get() {
            return Err(Error("injected_dashboard_write"));
        }
        let delivery = storage::atomic_replace_bytes(guard, Path::new("dashboard.md"), bytes)?;
        Ok(if self.uncertain {
            Delivery::DurabilityUncertain
        } else {
            delivery
        })
    }
}
#[test]
fn publication_faults_retain_partial_generated_prefix_and_dashboard_is_last() {
    for fail in 0..=5 {
        let root = workspace();
        fs::write(root.path().join("goal.md"), b"goal").unwrap();
        fs::write(root.path().join("dashboard.md"), b"old dashboard").unwrap();
        let output = arguments_with(
            root.path(),
            &Fault {
                index: Cell::new(0),
                fail,
                uncertain: false,
            },
        )
        .unwrap();
        assert_eq!(output.exit_code, 1);
        assert!(output.stdout.is_empty());
        assert!(output.warnings[0].starts_with("[ERROR] refresh_outputs_may_be_partial: "));
        assert_eq!(
            fs::read(root.path().join("dashboard.md")).unwrap(),
            b"old dashboard"
        );
        assert_eq!(fs::read(root.path().join("goal.md")).unwrap(), b"goal");
        assert!(!root.path().join("logs").exists());
        assert!(!root.path().join("state/tasks.json").exists());
    }
    let root = workspace();
    let output = arguments_with(
        root.path(),
        &Fault {
            index: Cell::new(0),
            fail: usize::MAX,
            uncertain: true,
        },
    )
    .unwrap();
    assert_eq!(output.exit_code, 1);
    assert!(output.warnings[0].contains("refresh_dashboard_durability_uncertain"));
    assert!(root.path().join("dashboard.md").is_file());
}

#[test]
fn custom_checkpoint_reference_is_text_not_decision_grammar_and_messages_are_first() {
    let root = workspace();
    fs::create_dir(root.path().join("logs")).unwrap();
    fs::create_dir(root.path().join("checkpoints")).unwrap();
    fs::write(
        root.path().join("checkpoints/custom.md"),
        b"not decision grammar",
    )
    .unwrap();
    fs::write(
        root.path().join("logs/messages.jsonl"),
        b"{\"checkpoint\":\"custom\",\"type\":\"other\"}\n",
    )
    .unwrap();
    assert_eq!(arguments(root.path()).unwrap().exit_code, 0);
    fs::write(
        root.path().join("logs/messages.jsonl"),
        b"{\"checkpoint\":42}\n",
    )
    .unwrap();
    fs::write(root.path().join("logs/events.jsonl"), b"broken\n").unwrap();
    assert_eq!(
        arguments(root.path()).err().unwrap().code(),
        "invalid_checkpoint_history_reference"
    );
}

#[test]
fn staged_limits_are_incremental_and_do_not_mutate_stage_on_refusal() {
    let mut stage = Prepared {
        files: Vec::new(),
        dashboard: Vec::new(),
        runtime: "tmux".into(),
        size: 0,
    };
    assert_eq!(
        stage
            .push("large".into(), "x".repeat(MAX_TEMPLATE + 1), false)
            .unwrap_err()
            .code(),
        "unsupported_refresh_template_profile"
    );
    assert!(stage.files.is_empty());
    for index in 0..MAX_FILES {
        stage.push(index.to_string(), String::new(), false).unwrap();
    }
    assert_eq!(
        stage
            .push("overflow".into(), String::new(), true)
            .unwrap_err()
            .code(),
        "unsupported_refresh_stage_profile"
    );
    stage.files.clear();
    for index in 0..32 {
        stage
            .push(index.to_string(), "x".repeat(MAX_TEMPLATE), false)
            .unwrap();
    }
    assert_eq!(stage.size, MAX_STAGE);
    assert_eq!(
        stage
            .push("overflow".into(), "x".into(), false)
            .unwrap_err()
            .code(),
        "unsupported_refresh_stage_profile"
    );
    assert_eq!(stage.size, MAX_STAGE);
    assert_eq!(stage.files.len(), 32);
}

struct StageFailure;
impl RefreshIo for StageFailure {
    fn prepared(&self, _root: &Path, _guarded: bool) -> Result<()> {
        Err(Error("injected_stage_failure"))
    }
}
#[test]
fn staged_failure_cannot_create_lock_or_outputs() {
    let root = workspace();
    assert_eq!(
        arguments_with(root.path(), &StageFailure)
            .err()
            .unwrap()
            .code(),
        "injected_stage_failure"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn force_writes_keep_inode_and_mode_but_role_symlink_collisions_are_errors() {
    use std::os::unix::{
        fs::symlink,
        fs::{MetadataExt, PermissionsExt},
    };
    let root = workspace();
    fs::create_dir(root.path().join("tmux")).unwrap();
    let manifest = root.path().join("tmux/coven.toml");
    fs::write(&manifest, b"old").unwrap();
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o640)).unwrap();
    let before = fs::metadata(&manifest).unwrap();
    assert_eq!(arguments(root.path()).unwrap().exit_code, 0);
    let after = fs::metadata(&manifest).unwrap();
    assert_eq!(before.ino(), after.ino());
    assert_eq!(after.mode() & 0o777, 0o640);

    struct UnsafeCollision;
    impl RefreshIo for UnsafeCollision {
        fn prepared(&self, root: &Path, guarded: bool) -> Result<()> {
            if guarded {
                fs::create_dir(root.join("prompts"))?;
                symlink(root.join("goal.md"), root.join("prompts/architect.md"))?;
            }
            Ok(())
        }
    }
    let root = workspace();
    fs::write(root.path().join("goal.md"), b"authored").unwrap();
    let output = arguments_with(root.path(), &UnsafeCollision).unwrap();
    assert_eq!(output.exit_code, 1);
    assert_eq!(fs::read(root.path().join("goal.md")).unwrap(), b"authored");
    assert!(!root.path().join("dashboard.md").exists());
}

#[test]
fn effective_runtime_changes_info_not_tmux_launch_data() {
    let root = workspace();
    fs::write(
        root.path().join("coven.toml"),
        "[runtime]\nmultiplexer='tmux'\nopencode_command='opencode --special'\n",
    )
    .unwrap();
    let output = arguments(root.path()).unwrap();
    assert!(output.stdout.ends_with("(runtime=tmux)"));
    let manifest = fs::read_to_string(root.path().join("tmux/coven.toml")).unwrap();
    assert!(manifest.contains("tmux load-buffer"));
    assert!(manifest.contains("opencode --special"));
    fs::write(
        root.path().join("coven.toml"),
        "[runtime]\nmultiplexer='herdr'\nopencode_command='opencode --special'\n",
    )
    .unwrap();
    let output = arguments(root.path()).unwrap();
    assert!(output.stdout.ends_with("(runtime=herdr)"));
    assert_eq!(
        fs::read_to_string(root.path().join("tmux/coven.toml")).unwrap(),
        manifest
    );
}

fn state_bytes(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    fn visit(root: &Path, directory: &Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.push((path.strip_prefix(root).unwrap().to_owned(), Vec::new()));
                visit(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, &root.join("state"), &mut out);
    out.sort();
    out
}

#[test]
fn pending_import_is_not_refresh_authority_and_active_unsafe_projection_is_ignored() {
    for active in [false, true] {
        let root = workspace();
        fs::create_dir(root.path().join("state")).unwrap();
        fs::write(
            root.path().join("state/tasks.json"),
            br#"[{"id":"original","title":"Original task","status":"assigned"}]"#,
        )
        .unwrap();
        let snapshot = crate::tasks::read_snapshot(root.path()).unwrap();
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        let _report =
            crate::ledger::import_tasks(root.path(), snapshot.revision(), &guard).unwrap();
        drop(guard);
        if active {
            fs::remove_file(root.path().join("state/tasks.json")).unwrap();
            fs::create_dir(root.path().join("state/tasks.json")).unwrap();
        } else {
            fs::remove_file(root.path().join(crate::ledger::MARKER)).unwrap();
        }
        fs::write(
            root.path().join("state/handoff.json"),
            b"opaque handoff bytes",
        )
        .unwrap();
        fs::create_dir(root.path().join("checkpoints")).unwrap();
        let checkpoint = b"# Plan\nStatus: pending\n\n## User decision\n- [ ] Approved\n";
        fs::write(root.path().join("checkpoints/plan-approval.md"), checkpoint).unwrap();
        fs::create_dir(root.path().join("logs")).unwrap();
        let history = b"{\"type\":\"checkpoint.approved\",\"checkpoint\":\"plan-approval\"}\n";
        fs::write(root.path().join("logs/events.jsonl"), history).unwrap();
        let before = state_bytes(root.path());
        let output = arguments(root.path()).unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(state_bytes(root.path()), before);
        assert_eq!(
            fs::read(root.path().join("checkpoints/plan-approval.md")).unwrap(),
            checkpoint
        );
        assert_eq!(
            fs::read(root.path().join("logs/events.jsonl")).unwrap(),
            history
        );
        let dashboard = fs::read_to_string(root.path().join("dashboard.md")).unwrap();
        assert!(dashboard.contains("Original task"));
    }
}

#[cfg(unix)]
#[test]
fn forced_hardlinks_are_rejected_before_first_lock_without_changing_any_inode() {
    use std::{
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
    };
    type Entry = (PathBuf, u64, u64, u32, Vec<u8>);
    fn tree(directory: &Path) -> Vec<Entry> {
        let mut result = Vec::new();
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let bytes = if metadata.is_dir() {
                result.extend(tree(&path));
                Vec::new()
            } else {
                fs::read(&path).unwrap()
            };
            result.push((
                path,
                metadata.ino(),
                metadata.nlink(),
                metadata.mode(),
                bytes,
            ));
        }
        result.sort();
        result
    }
    for target in [
        "tmux/coven.toml",
        "prompts/architect.start.txt",
        "prompts/CUSTOM.start.txt",
    ] {
        for authored in [
            "goal.md",
            "prompts/architect.md",
            "state/tasks.json",
            "unlisted-authored.bin",
        ] {
            let root = workspace();
            for directory in ["tmux", "prompts", "state"] {
                fs::create_dir(root.path().join(directory)).unwrap();
            }
            fs::write(root.path().join("goal.md"), b"authored goal").unwrap();
            fs::write(root.path().join("prompts/architect.md"), b"\xffcustom role").unwrap();
            fs::write(root.path().join("state/tasks.json"), b"[]").unwrap();
            fs::write(
                root.path().join("unlisted-authored.bin"),
                b"\xffarbitrary bytes",
            )
            .unwrap();
            fs::set_permissions(
                root.path().join(authored),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
            fs::hard_link(root.path().join(authored), root.path().join(target)).unwrap();
            let before = tree(root.path());
            assert_eq!(
                arguments(root.path()).err().unwrap().code(),
                "unsafe_generated_hardlink",
                "{target} -> {authored}"
            );
            assert_eq!(tree(root.path()), before, "{target} -> {authored}");
            assert!(!root.path().join("state/.workspace.lock").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn existing_role_and_atomic_dashboard_hardlinks_do_not_modify_authored_inode() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = workspace();
    fs::create_dir(root.path().join("prompts")).unwrap();
    let goal = root.path().join("goal.md");
    fs::write(&goal, b"authored goal").unwrap();
    fs::set_permissions(&goal, fs::Permissions::from_mode(0o640)).unwrap();
    let role = root.path().join("prompts/architect.md");
    fs::hard_link(&goal, &role).unwrap();
    fs::hard_link(&goal, root.path().join("dashboard.md")).unwrap();
    let before = fs::metadata(&goal).unwrap();
    assert_eq!(arguments(root.path()).unwrap().exit_code, 0);
    for path in [&goal, &role] {
        assert_eq!(fs::read(path).unwrap(), b"authored goal");
        let after = fs::metadata(path).unwrap();
        assert_eq!(after.ino(), before.ino());
        assert_eq!(after.mode(), before.mode());
        assert_eq!(after.nlink(), 2);
    }
    assert_ne!(
        fs::metadata(root.path().join("dashboard.md"))
            .unwrap()
            .ino(),
        before.ino()
    );
}

#[cfg(unix)]
#[test]
fn forced_hardlinks_are_rechecked_under_guard_and_before_descriptor_truncation() {
    struct AddAlias {
        guarded: bool,
    }
    impl RefreshIo for AddAlias {
        fn prepared(&self, root: &Path, guarded: bool) -> Result<()> {
            if guarded == self.guarded {
                fs::hard_link(root.join("goal.md"), root.join("prompts/CUSTOM.start.txt"))?;
            }
            Ok(())
        }
    }
    for guarded in [false, true] {
        let root = workspace();
        fs::create_dir(root.path().join("prompts")).unwrap();
        fs::create_dir(root.path().join("tmux")).unwrap();
        fs::write(root.path().join("goal.md"), b"authored goal").unwrap();
        fs::write(root.path().join("tmux/coven.toml"), b"old manifest").unwrap();
        let result = arguments_with(root.path(), &AddAlias { guarded });
        if guarded {
            let output = result.unwrap();
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.warnings,
                ["[ERROR] refresh_outputs_may_be_partial: unsafe_generated_hardlink"]
            );
        } else {
            assert_eq!(result.err().unwrap().code(), "unsafe_generated_hardlink");
            assert_eq!(
                fs::read(root.path().join("tmux/coven.toml")).unwrap(),
                b"old manifest"
            );
            assert!(!root.path().join("prompts/architect.md").exists());
        }
        assert_eq!(
            fs::read(root.path().join("goal.md")).unwrap(),
            b"authored goal"
        );
        assert!(!root.path().join("dashboard.md").exists());
    }
}
