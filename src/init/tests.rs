use super::*;
use std::cell::{Cell, RefCell};

fn source() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}
fn blank() -> Options {
    Options {
        blank: true,
        name: Some("Test".into()),
        ..Options::default()
    }
}

#[test]
fn default_and_blank_are_readable_snapshots_with_ordered_events() {
    for options in [Options::default(), blank()] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("new");
        let message = init(&root, &options).unwrap();
        assert_eq!(message.lines().count(), 3);
        assert!(!message.ends_with('\n'));
        let state = workspace::load(&root).unwrap();
        assert_eq!(
            state.team.agent_ids().unwrap(),
            vec!["clippy", "srikanth", "jamal", "kris", "eric"]
        );
        assert_eq!(root.join("spec.md").exists(), !options.blank);
        assert!(!root.join("state/task-authority.json").exists());
        assert!(!root.join("state/task-ledger").exists());
        assert!(!root.join("state/execution").exists());
        let raw = fs::read(root.join("logs/events.jsonl")).unwrap();
        let mut types = Vec::new();
        task_delivery::visit_log_records(&raw, &mut |v| {
            types.push(
                v.as_object()
                    .unwrap()
                    .get("type")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(types[0], "workspace.created");
        if options.blank {
            assert_eq!(types.len(), 1);
        } else {
            assert_eq!(types[1], "goal.seeded");
            assert_eq!(types.len(), 2 + state.tasks.as_array().unwrap().len());
            assert!(types[2..].iter().all(|s| s == "task.seeded"));
        }
        assert_eq!(
            fs::read_to_string(root.join("dashboard.md")).unwrap(),
            task_delivery::prepare_dashboard(&root).unwrap()
        );
    }
}

#[test]
fn reserved_inventory_matches_oracle_and_force_never_adopts_any_entry() {
    let python = include_str!("../../runner/coven.py");
    let inventory = python
        .split("INIT_RESERVED_NAMES = (")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let names = inventory
        .split('"')
        .enumerate()
        .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
        .collect::<Vec<_>>();
    assert_eq!(RESERVED, names);
    for name in RESERVED {
        for kind in ["file", "directory", "link", "fifo"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            match kind {
                "file" => fs::write(&path, b"untouched").unwrap(),
                "directory" => fs::create_dir(&path).unwrap(),
                #[cfg(unix)]
                "link" => std::os::unix::fs::symlink("missing", &path).unwrap(),
                #[cfg(unix)]
                "fifo" => {
                    use std::os::unix::ffi::OsStrExt;
                    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                _ => continue,
            }
            let options = Options {
                force: true,
                agents: "!!!".into(),
                ..blank()
            };
            assert_eq!(
                init(dir.path(), &options).unwrap_err().code(),
                "init_reserved_entry_exists_force_does_not_authorize_reset"
            );
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
            if kind == "file" {
                assert_eq!(fs::read(path).unwrap(), b"untouched");
            }
        }
    }
}

#[test]
fn resolver_is_noncreating_and_processes_symlink_before_parent() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    fs::create_dir_all(dir.join("real/child")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("real/child", dir.join("alias")).unwrap();
        std::os::unix::fs::symlink("missing/suffix", dir.join("dangling")).unwrap();
        assert_eq!(
            resolve_root(&dir.join("alias/../new/../target")).unwrap(),
            dir.join("real/target")
        );
        assert_eq!(
            resolve_root(&dir.join("dangling/../next")).unwrap(),
            dir.join("missing/next")
        );
    }
    assert_eq!(
        resolve_root(&dir.join("absent/../new/deep")).unwrap(),
        dir.join("new/deep")
    );
    assert!(!dir.join("new").exists());
    fs::write(dir.join("file"), b"x").unwrap();
    assert_eq!(
        init(&dir.join("file"), &blank()).unwrap_err().code(),
        "init_workspace_not_directory"
    );
}

#[test]
fn initial_json_is_ascii_ordered_and_roundtrips_without_changing_storage() {
    let value = obj(vec![
        ("z", s("\u{7f}\u{e9}\u{1f600}")),
        ("a", s("literal \\u00e9")),
    ])
    .unwrap();
    assert_eq!(
        initial_json(&value).unwrap(),
        "{\n  \"z\": \"\\u007f\\u00e9\\ud83d\\ude00\",\n  \"a\": \"literal \\\\u00e9\"\n}\n"
    );
    assert!(!value.storage().unwrap().is_ascii());
    let huge = Value::Array((0..9).map(|_| s("\u{e9}".repeat(MAX_TEXT / 2))).collect());
    assert!(initial_json(&huge).is_err());
}

#[test]
fn deterministic_inputs_and_limits_reject_before_any_mkdir() {
    let dir = tempfile::tempdir().unwrap();
    for options in [
        Options {
            agents: "a,A".into(),
            ..blank()
        },
        Options {
            opencode_command: " \t".into(),
            ..blank()
        },
        Options {
            name: Some("!!!".into()),
            ..blank()
        },
        Options {
            multiplexer: "screen".into(),
            ..blank()
        },
        Options {
            example: "missing-example".into(),
            ..Options::default()
        },
        Options {
            name: Some("x".repeat(MAX_TEXT)),
            ..blank()
        },
    ] {
        let root = dir.path().join("absent/deep");
        assert!(init(&root, &options).is_err());
        assert!(!dir.path().join("absent").exists());
    }
    let assets = tempfile::tempdir().unwrap();
    let example = assets.path().join("examples/custom");
    fs::create_dir_all(&example).unwrap();
    let options = Options {
        example: "custom".into(),
        ..Options::default()
    };
    let root = dir.path().join("absent/deep");
    for (manifest, tasks, goal) in [
        ("{\"id\":7}", "[]", String::new()),
        ("{\"seed_message\":{}}", "[]", String::new()),
        ("{}", "[{\"owner\":7}]", String::new()),
        ("{}", "[{\"id\":\"a\"},{\"id\":\"a\"}]", String::new()),
        ("{}", "[]", "x".repeat(MAX_TEXT)),
    ] {
        fs::write(example.join("example.json"), manifest).unwrap();
        fs::write(example.join("tasks.json"), tasks).unwrap();
        fs::write(example.join("goal.md"), goal).unwrap();
        assert!(run(&root, &options, assets.path(), &SystemIo).is_err());
        assert!(!dir.path().join("absent").exists());
    }
}

struct FaultIo {
    order: RefCell<Vec<String>>,
    collision: Option<String>,
    partial_at: Option<usize>,
    writes: Cell<usize>,
    fail_event: Option<usize>,
    events: Cell<usize>,
    fail_dashboard: bool,
}
impl FaultIo {
    fn new() -> Self {
        Self {
            order: RefCell::new(vec![]),
            collision: None,
            partial_at: None,
            writes: Cell::new(0),
            fail_event: None,
            events: Cell::new(0),
            fail_dashboard: false,
        }
    }
}
impl InitIo for FaultIo {
    fn before_file(&self, root: &Path, relative: &str) -> Result<()> {
        self.order.borrow_mut().push(relative.into());
        if self.collision.as_deref() == Some(relative) {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, b"racing writer")?;
        }
        Ok(())
    }
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
        let index = self.writes.get();
        self.writes.set(index + 1);
        if self.partial_at == Some(index) {
            file.write_all(&bytes[..bytes.len().min(7)])?;
            return Err(std::io::Error::other("injected"));
        }
        file.write_all(bytes)
    }
    fn event(
        &self,
        root: &Path,
        kind: &str,
        payload: &Value,
        guard: &WorkspaceGuard,
    ) -> Result<()> {
        let index = self.events.get();
        self.events.set(index + 1);
        if self.fail_event == Some(index) {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(root.join("logs/events.jsonl"))?;
            file.write_all(b"{partial")?;
            return Err(Error("event_append_may_be_partial"));
        }
        task_delivery::append_event(root, kind, payload, guard)
    }
    fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
        guard.validate_for(root)?;
        if self.fail_dashboard {
            task_delivery::refresh_dashboard(root, guard)?;
            return Err(Error("dashboard_synchronization_uncertain"));
        }
        task_delivery::refresh_dashboard(root, guard)
    }
}

#[test]
fn exact_creation_order_collision_and_partial_write_preserve_progress() {
    let options = blank();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new");
    let prepared = prepare(&root, &options, source()).unwrap();
    let expected = prepared
        .files
        .iter()
        .map(|(p, _)| p.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        &expected[..12],
        &[
            "coven.json",
            "state/agents.json",
            "state/tasks.json",
            "goal.md",
            "coven.toml",
            "logs/events.jsonl",
            "logs/messages.jsonl",
            "tmux/coven.toml",
            "work/README.md",
            "output/README.md",
            "checkpoints/plan-approval.md",
            "checkpoints/pre-delivery-review.md"
        ]
    );
    for index in 0..expected.len() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("new");
        let mut io = FaultIo::new();
        // Every exclusive-create boundary must stop, never adopt or retry.
        io.collision = Some(expected[index].clone());
        assert_eq!(
            run(&root, &options, source(), &io).unwrap_err().code(),
            "initialization_stopped_partial_files_preserved_for_inspection"
        );
        assert_eq!(*io.order.borrow(), expected[..=index]);
        assert_eq!(
            fs::read(root.join(&expected[index])).unwrap(),
            b"racing writer"
        );
        assert!(!root.join("state/.workspace.lock").exists());
        assert!(!root.join("dashboard.md").exists());
    }
    let mut io = FaultIo::new();
    io.partial_at = Some(1);
    assert_eq!(
        run(&root, &options, source(), &io).unwrap_err().code(),
        "initialization_stopped_partial_files_preserved_for_inspection"
    );
    assert_eq!(fs::read(root.join("state/agents.json")).unwrap().len(), 7);
    assert!(root.join("coven.json").is_file());
    assert!(!root.join("state/tasks.json").exists());
    assert!(!root.join("state/.workspace.lock").exists());
    assert!(
        init(
            &root,
            &Options {
                force: true,
                ..options
            }
        )
        .is_err()
    );
}

#[test]
fn staging_budget_is_incremental_and_render_failure_preserves_all_seeds() {
    let mut total = values::MAX_OUTPUT - 3;
    admit_file(&mut total, "goal.md", "abc").unwrap();
    assert_eq!(total, values::MAX_OUTPUT);
    assert_eq!(
        admit_file(&mut total, "goal.md", "x").unwrap_err().code(),
        "unsupported_init_output_profile"
    );
    let dir = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let example = assets.path().join("examples/custom");
    fs::create_dir_all(&example).unwrap();
    fs::write(example.join("example.json"), b"{}").unwrap();
    fs::write(example.join("prompt-context.md"), "x".repeat(MAX_TEXT / 2)).unwrap();
    let options = Options {
        agents: (0..70)
            .map(|i| format!("agent{i}"))
            .collect::<Vec<_>>()
            .join(","),
        example: "custom".into(),
        ..Options::default()
    };
    let root = dir.path().join("new");
    assert_eq!(
        run(&root, &options, assets.path(), &SystemIo)
            .unwrap_err()
            .code(),
        "unsupported_init_output_profile"
    );
    assert!(!root.exists());

    struct RenderFailure;
    impl InitIo for RenderFailure {
        fn dashboard(&self, root: &Path, guard: &WorkspaceGuard) -> Result<()> {
            guard.validate_for(root)?;
            assert!(WorkspaceGuard::acquire(root, Duration::ZERO).is_err());
            fs::write(root.join("goal.md"), b"\xff")?;
            task_delivery::refresh_dashboard(root, guard)
        }
    }
    assert!(run(&root, &blank(), source(), &RenderFailure).is_err());
    assert!(root.join("prompts/eric.start.txt").is_file());
    assert!(!root.join("dashboard.md").exists());
    assert_eq!(fs::read(root.join("goal.md")).unwrap(), b"\xff");
    task_delivery::validate_log(&fs::read(root.join("logs/events.jsonl")).unwrap()).unwrap();
}

#[test]
fn event_and_dashboard_errors_are_not_success_envelopes_or_rollbacks() {
    for event in [Some(0), Some(1), Some(2), None] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("new");
        let mut io = FaultIo::new();
        io.fail_event = event;
        io.fail_dashboard = event.is_none();
        assert!(run(&root, &Options::default(), source(), &io).is_err());
        assert!(root.join("prompts/eric.start.txt").is_file());
        assert!(root.join("state/.workspace.lock").is_file());
        assert_eq!(root.join("dashboard.md").exists(), event.is_none());
        let bytes = fs::read(root.join("logs/events.jsonl")).unwrap();
        if event.is_some() {
            assert!(bytes.ends_with(b"{partial"));
        } else {
            task_delivery::validate_log(&bytes).unwrap();
        }
        workspace::load(&root).unwrap();
    }
}

#[test]
fn root_mkdir_failure_has_partial_inspection_code_and_preserves_racing_bytes() {
    struct RootCollision;
    impl InitIo for RootCollision {
        fn mkdir_root(&self, root: &Path) -> Result<()> {
            fs::write(root, b"racing root file")?;
            fs::create_dir_all(root)?;
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new");
    assert_eq!(
        run(&root, &blank(), source(), &RootCollision)
            .unwrap_err()
            .code(),
        "initialization_stopped_partial_files_preserved_for_inspection"
    );
    assert_eq!(fs::read(root).unwrap(), b"racing root file");
}

#[test]
fn example_fallbacks_and_missing_optional_assets_use_python_defaults() {
    let assets = tempfile::tempdir().unwrap();
    let example = assets.path().join("examples/custom");
    fs::create_dir_all(&example).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new");
    let options = Options {
        example: "custom".into(),
        ..Options::default()
    };
    // Missing, null, false and empty strings all select the same fallback.
    for id in [None, Some("null"), Some("false"), Some("\"\"")] {
        for message in [None, Some("null"), Some("false"), Some("\"\"")] {
            let fields = [("id", id), ("seed_message", message)]
                .into_iter()
                .filter_map(|(key, value)| value.map(|value| format!("\"{key}\":{value}")))
                .collect::<Vec<_>>()
                .join(",");
            fs::write(example.join("example.json"), format!("{{{fields}}}")).unwrap();
            let prepared = prepare(&root, &options, assets.path()).unwrap();
            let file = |path: &str| {
                prepared
                    .files
                    .iter()
                    .find(|(p, _)| p == path)
                    .unwrap()
                    .1
                    .as_str()
            };
            let team =
                values::parse(file("coven.json").as_bytes(), Profile::LegacySnapshot).unwrap();
            assert_eq!(
                team.as_object().unwrap().get("example").unwrap().as_str(),
                Some("custom")
            );
            assert_eq!(file("state/tasks.json"), "[]\n");
            assert_eq!(
                file("goal.md"),
                "# Goal\n\nDescribe the unified coven goal here.\n"
            );
            assert_eq!(
                file("spec.md"),
                "# Spec\n\nDescribe the desired behavior here.\n"
            );
            for (path, content) in templates_init::structure(assets.path(), None).unwrap() {
                assert_eq!(file(&path), content);
            }
            assert_eq!(
                file("prompts/clippy.md"),
                templates::prompt("clippy", root.to_str().unwrap(), "")
            );
            assert_eq!(prepared.events.len(), 2);
            assert_eq!(
                prepared.success.lines().nth(1),
                Some("[INFO] Seeded example: custom")
            );
        }
    }
    assert!(!root.exists());
}

#[test]
fn example_text_normalizes_crlf_and_bare_cr_before_rendering() {
    let assets = tempfile::tempdir().unwrap();
    let example = assets.path().join("examples/custom");
    fs::create_dir_all(example.join("work")).unwrap();
    fs::create_dir_all(example.join("output")).unwrap();
    fs::write(
        example.join("example.json"),
        b"{\r\n\"id\": \"custom\"\r\n}",
    )
    .unwrap();
    for path in [
        "goal.md",
        "spec.md",
        "prompt-context.md",
        "work/README.md",
        "output/README.md",
    ] {
        fs::write(example.join(path), b"First\r\nSecond\rThird").unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new");
    let options = Options {
        example: "custom".into(),
        ..Options::default()
    };
    run(&root, &options, assets.path(), &SystemIo).unwrap();
    for path in ["goal.md", "spec.md", "work/README.md", "output/README.md"] {
        let expected = if path == "goal.md" {
            "First\nSecond\nThird\n"
        } else {
            "First\nSecond\nThird"
        };
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), expected);
    }
    assert_eq!(
        fs::read_to_string(root.join("prompts/clippy.md")).unwrap(),
        templates::prompt("clippy", root.to_str().unwrap(), "First\nSecond\nThird")
    );
    workspace::load(&root).unwrap();
}

#[test]
fn example_loading_precedes_opencode_and_record_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new");
    let options = Options {
        example: "missing-example".into(),
        name: Some("!!!".into()),
        opencode_command: "".into(),
        ..Options::default()
    };
    assert_eq!(
        run(&root, &options, source(), &SystemIo)
            .unwrap_err()
            .code(),
        "init_example_assets_unavailable"
    );
    assert!(!root.exists());
    let options = Options {
        example: "interactive-demo".into(),
        ..options
    };
    assert_eq!(
        run(&root, &options, source(), &SystemIo)
            .unwrap_err()
            .code(),
        "empty_init_opencode_command"
    );
    assert!(!root.exists());
}
