use super::*;
use std::cell::Cell;

struct Fixed;
impl InitIo for Fixed {
    fn now(&self) -> Result<String> {
        Ok("2026-01-02T03:04:05+00:00".into())
    }
    fn uuid(&self) -> Result<String> {
        Ok("01234567-89ab-4cde-8012-3456789abcde".into())
    }
}

fn blank() -> Options {
    Options {
        blank: true,
        ..Options::default()
    }
}

#[test]
fn all_flags_semantics_and_no_authority_activation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("custom");
    let options = Options {
        name: Some(String::new()),
        agents: " Alice Smith , BOB ".into(),
        opencode_command: "opencode --flag".into(),
        multiplexer: "tmux".into(),
        example: "not-supported".into(),
        blank: true,
        force: true,
    };
    let output = arguments_with(&root, &options, &Fixed).unwrap();
    assert_eq!(output.exit_code, 0);
    let state = workspace::load(&root).unwrap();
    let team = state.team.value().as_object().unwrap();
    assert_eq!(team.get("name").unwrap().as_str(), Some("custom"));
    assert!(matches!(team.get("example"), Some(Value::Null)));
    assert_eq!(team.get("multiplexer").unwrap().as_str(), Some("tmux"));
    assert_eq!(state.team.agent_ids().unwrap(), ["alice-smith", "bob"]);
    assert!(state.tasks.as_array().unwrap().is_empty());
    let mut state_files = fs::read_dir(root.join("state"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    state_files.sort();
    assert_eq!(
        state_files,
        [".workspace.lock", "agents.json", "tasks.json"]
    );
    assert!(!root.join("spec.md").exists());
    for path in [
        "state/task-authority.json",
        "state/task-ledger",
        "state/actions",
        "state/lead.json",
        "state/execution",
        "state/control",
        "state/repositories",
        "state/groups",
        "state/runtime-launch-control.json",
        "execution",
        "control",
        "repositories",
        "groups",
    ] {
        assert!(!root.join(path).exists(), "{path}");
    }
    assert!(arguments_with(&root, &options, &Fixed).is_err());
}

#[test]
fn preserved_collisions_precede_flag_semantics() {
    for reserved in RESERVED {
        for directory in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let path = root.join(reserved);
            if directory {
                fs::create_dir(&path).unwrap();
            } else {
                fs::write(&path, b"evidence").unwrap();
            }
            let options = Options {
                agents: ",".into(),
                force: true,
                ..blank()
            };
            let error = arguments_with(root, &options, &Fixed).err().unwrap();
            assert_eq!(error.code(), "init_reserved_name_exists");
            if !directory {
                assert_eq!(fs::read(&path).unwrap(), b"evidence");
            }
            assert_eq!(fs::read_dir(root).unwrap().count(), 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn preserved_dangling_symlinks_and_root_non_utf8() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    for reserved in RESERVED {
        let temp = tempfile::tempdir().unwrap();
        symlink("absent", temp.path().join(reserved)).unwrap();
        assert_eq!(
            arguments_with(temp.path(), &blank(), &Fixed)
                .err()
                .unwrap()
                .code(),
            "init_reserved_name_exists"
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'x', 255]));
    assert!(arguments_with(&root, &blank(), &Fixed).is_err());
    assert!(!root.exists());
}

#[test]
fn deterministic_validation_no_root_or_lock() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing/child");
    for options in [
        Options {
            agents: "A,a".into(),
            ..blank()
        },
        Options {
            agents: "a,".into(),
            ..blank()
        },
        Options {
            multiplexer: "invalid".into(),
            ..blank()
        },
        Options {
            opencode_command: " \t".into(),
            ..blank()
        },
        Options {
            example: String::new(),
            ..Options::default()
        },
        Options {
            name: Some("x".repeat(MAX_TEMPLATE + 1)),
            ..blank()
        },
        Options {
            agents: (0..MAX_FILES)
                .map(|i| format!("a{i}"))
                .collect::<Vec<_>>()
                .join(","),
            ..blank()
        },
    ] {
        assert!(arguments_with(&root, &options, &Fixed).is_err());
        assert!(!temp.path().join("missing").exists());
    }
}

#[test]
fn source_context_exact_bytes_and_initial_event_shape() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("demo");
    let options = Options {
        agents: "Clippy,Other".into(),
        ..Options::default()
    };
    assert_eq!(
        arguments_with(&root, &options, &Fixed).unwrap().exit_code,
        0
    );
    let example = assets::init_example("interactive-demo").unwrap();
    assert_eq!(
        fs::read(root.join("spec.md")).unwrap(),
        example.spec.as_bytes()
    );
    assert_eq!(
        fs::read(root.join("work/README.md")).unwrap(),
        example.work.as_bytes()
    );
    let context = PureAssetContext {
        cli_command: "coven".into(),
        example_text: example.context,
    };
    assert_eq!(
        fs::read(root.join("prompts/clippy.md")).unwrap(),
        assets::roleprompt_text("clippy", root.to_str().unwrap(), &context).as_bytes()
    );
    let state = workspace::load(&root).unwrap();
    let tasks = state.tasks.as_array().unwrap();
    assert_eq!(
        tasks[0].as_object().unwrap().get("owner").unwrap().as_str(),
        Some("clippy")
    );
    assert!(matches!(
        tasks[1].as_object().unwrap().get("owner"),
        Some(Value::Null)
    ));
    let log = fs::read_to_string(root.join("logs/events.jsonl")).unwrap();
    let events = log
        .lines()
        .map(|line| values::parse(line.as_bytes(), Profile::LegacySnapshot).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), tasks.len() + 2);
    assert_eq!(
        events[0]
            .as_object()
            .unwrap()
            .entries()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>(),
        ["event_id", "ts", "type", "name", "agents", "example"]
    );
    assert_eq!(
        events[1].as_object().unwrap().get("type").unwrap().as_str(),
        Some("goal.seeded")
    );
    for (event, task) in events[2..].iter().zip(tasks) {
        assert_eq!(
            event
                .as_object()
                .unwrap()
                .get("task")
                .unwrap()
                .compact()
                .unwrap(),
            task.compact().unwrap()
        );
    }
    assert!(fs::read(root.join("coven.json")).unwrap().is_ascii());
}

struct Fault {
    count: Cell<usize>,
    fail: usize,
}
impl InitIo for Fault {
    fn boundary(&self, _: &str, root: &Path) -> Result<()> {
        assert!(!root.join("coven.json").exists(), "config must be last");
        let count = self.count.get() + 1;
        self.count.set(count);
        if count == self.fail {
            Err(Error("injected_init_fault"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn config_last_fault_every_boundary_and_no_stale_reset() {
    let temp = tempfile::tempdir().unwrap();
    let probe = Fault {
        count: Cell::new(0),
        fail: usize::MAX,
    };
    assert_eq!(
        arguments_with(&temp.path().join("probe"), &blank(), &probe)
            .unwrap()
            .exit_code,
        0
    );
    for fail in 1..=probe.count.get() {
        let root = temp.path().join(format!("fault-{fail}"));
        let io = Fault {
            count: Cell::new(0),
            fail,
        };
        let result = arguments_with(&root, &blank(), &io).unwrap();
        assert_eq!(result.exit_code, 1, "boundary {fail}");
        assert!(result.stdout.is_empty());
        assert!(!root.join("coven.json").exists());
        if root.join("state").exists() {
            assert!(arguments_with(&root, &blank(), &Fixed).is_err());
        }
    }
}

struct Uncertain;
impl InitIo for Uncertain {
    fn marker(&self, reservation: &bootstrap::Reservation, bytes: &[u8]) -> Result<Delivery> {
        reservation.publish_marker(bytes)?;
        Ok(Delivery::DurabilityUncertain)
    }
}

#[test]
fn known_marker_uncertainty_is_not_ordinary_failure() {
    let temp = tempfile::tempdir().unwrap();
    let result = arguments_with(temp.path(), &blank(), &Uncertain).unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stdout.starts_with("[INFO] Coven workspace:"));
    assert_eq!(result.warnings.len(), 1);
    assert!(workspace::load(temp.path()).is_ok());
}

#[test]
fn role_count_and_stage_bounds() {
    let mut prepared = Prepared::new();
    for index in 0..MAX_FILES {
        prepared
            .push(format!("file-{index}"), vec![], MAX_TEMPLATE)
            .unwrap();
    }
    assert!(prepared.push("extra".into(), vec![], MAX_TEMPLATE).is_err());
    let mut prepared = Prepared::new();
    for index in 0..32 {
        prepared
            .push(
                format!("file-{index}"),
                vec![b'x'; MAX_TEMPLATE],
                MAX_TEMPLATE,
            )
            .unwrap();
    }
    assert!(
        prepared
            .push("extra".into(), vec![b'x'], MAX_TEMPLATE)
            .is_err()
    );
}

#[test]
fn native_reservation_one_winner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("race");
    fs::create_dir(&root).unwrap();
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| arguments_with(&root, &blank(), &Fixed));
        let second = scope.spawn(|| arguments_with(&root, &blank(), &Fixed));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        results
            .iter()
            .filter(|r| r.as_ref().is_ok_and(|r| r.exit_code == 0))
            .count(),
        1
    );
    assert!(workspace::load(&root).is_ok());
}

struct Collision {
    at: &'static str,
    relative: &'static str,
    fired: Cell<bool>,
}
impl InitIo for Collision {
    fn boundary(&self, operation: &str, root: &Path) -> Result<()> {
        if operation == self.at && !self.fired.replace(true) {
            fs::write(root.join(self.relative), b"concurrent evidence")?;
        }
        Ok(())
    }
}

#[test]
fn preserved_concurrent_collisions_never_overwrite() {
    for (at, relative) in [
        ("reserve", "goal.md"),
        ("open", "state/agents.json"),
        ("publish-marker", "coven.json"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let io = Collision {
            at,
            relative,
            fired: Cell::new(false),
        };
        let result = arguments_with(temp.path(), &blank(), &io).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert_eq!(
            fs::read(temp.path().join(relative)).unwrap(),
            b"concurrent evidence"
        );
    }
}

#[test]
fn ascii_snapshot_and_literal_unicode_history_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let options = Options {
        name: Some("caf\u{e9}\nname".into()),
        ..blank()
    };
    let prepared = prepare(temp.path(), &options, &Fixed).unwrap();
    let config = &prepared
        .files
        .iter()
        .find(|file| file.relative == "coven.json")
        .unwrap()
        .bytes;
    assert!(config.is_ascii());
    assert!(
        std::str::from_utf8(config)
            .unwrap()
            .contains("caf\\u00e9\\nname")
    );
    let events = &prepared
        .files
        .iter()
        .find(|file| file.relative == "logs/events.jsonl")
        .unwrap()
        .bytes;
    assert!(
        std::str::from_utf8(events)
            .unwrap()
            .contains("caf\u{e9}\\nname")
    );
    assert!(!temp.path().join("state").exists());
}

#[test]
fn encoded_json_and_output_caps_are_pre_effects() {
    let mut prepared = Prepared::new();
    // Literal Unicode fits the value profile but ASCII persistence expansion does not.
    let value = Value::Array(vec![s(&"\u{ffff}".repeat(MAX_TEMPLATE / 3)); 5]);
    assert!(prepared.json("oversized.json", &value).is_err());
    assert!(prepared.files.is_empty());
    assert!(human(&"\u{ffff}".repeat(values::MAX_OUTPUT / 6 + 1)).is_err());
}

#[cfg(unix)]
#[test]
fn dashboard_private_mode_and_unrelated_files_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("authored"), b"do not touch").unwrap();
    assert_eq!(
        arguments_with(temp.path(), &blank(), &Fixed)
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(
        fs::metadata(temp.path().join("dashboard.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    assert_eq!(
        fs::read(temp.path().join("authored")).unwrap(),
        b"do not touch"
    );
}

#[test]
fn root_non_directory_precedes_invalid_flags() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("file");
    fs::write(&root, b"evidence").unwrap();
    let options = Options {
        agents: ",".into(),
        ..blank()
    };
    assert!(arguments_with(&root, &options, &Fixed).is_err());
    assert_eq!(fs::read(&root).unwrap(), b"evidence");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn defaults_and_independent_metadata_calls() {
    struct Counting {
        times: Cell<usize>,
        ids: Cell<usize>,
    }
    impl InitIo for Counting {
        fn now(&self) -> Result<String> {
            let index = self.times.get();
            self.times.set(index + 1);
            Ok(format!("2026-01-02T03:04:05.{index:06}+00:00"))
        }
        fn uuid(&self) -> Result<String> {
            let index = self.ids.get();
            self.ids.set(index + 1);
            Ok(format!("01234567-89ab-4cde-8012-{index:012}"))
        }
    }
    let defaults = Options::default();
    assert!(defaults.name.is_none());
    assert_eq!(defaults.agents, "Clippy,Srikanth,Jamal,Kris,Eric");
    assert_eq!(defaults.opencode_command, "oc");
    assert_eq!(defaults.multiplexer, "herdr");
    assert_eq!(defaults.example, "interactive-demo");
    assert!(!defaults.blank && !defaults.force);
    let temp = tempfile::tempdir().unwrap();
    let clock = Counting {
        times: Cell::new(0),
        ids: Cell::new(0),
    };
    let prepared = prepare(temp.path(), &defaults, &clock).unwrap();
    assert_eq!(clock.times.get(), 21); // Team + two per task + one per event.
    assert_eq!(clock.ids.get(), 8);
    assert_eq!(prepared.files.len(), 29);
    assert!(!temp.path().join("state").exists());
}

#[test]
fn output_escapes_root_before_effects_and_keeps_three_info_lines() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("caf\u{e9}\n\u{1b}");
    let prepared = prepare(&root, &blank(), &Fixed).unwrap();
    assert!(!root.exists());
    assert!(prepared.stdout.is_ascii());
    assert_eq!(prepared.stdout.lines().count(), 3);
    assert!(prepared.stdout.contains("caf\\u00e9\\n\\u001b"));
    assert!(!prepared.stdout.contains('\u{1b}'));
}

#[test]
fn replaced_reservation_identity_refuses_before_next_file() {
    struct ReplaceState;
    impl InitIo for ReplaceState {
        fn boundary(&self, operation: &str, root: &Path) -> Result<()> {
            if operation == "open" {
                fs::rename(root.join("state"), root.join("saved-state"))?;
                fs::create_dir(root.join("state"))?;
            }
            Ok(())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let result = arguments_with(temp.path(), &blank(), &ReplaceState).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert!(temp.path().join("saved-state/.workspace.lock").exists());
    assert_eq!(fs::read_dir(temp.path().join("state")).unwrap().count(), 0);
    assert!(!temp.path().join("coven.json").exists());
}

#[test]
fn command_validation_precedence_is_process_isolated_and_has_no_effects() {
    const CASE: &str = "COVEN_INIT_COMMAND_VALIDATION_CASE";
    if let Ok(case) = std::env::var(CASE) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("missing/child");
        let options = Options {
            opencode_command: if case == "spaces" { " \t\n" } else { "" }.into(),
            ..blank()
        };
        let error = arguments_with(&root, &options, &Fixed).err().unwrap();
        assert_eq!(
            error.code(),
            if case == "environment" {
                "unsupported_multiplexer"
            } else {
                "invalid_init_command"
            }
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        return;
    }
    for case in ["empty", "spaces", "environment"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "init::tests::command_validation_precedence_is_process_isolated_and_has_no_effects",
                "--nocapture",
            ])
            .env(CASE, case)
            .env_remove("COVEN_COMMAND")
            .env_remove("COVEN_MULTIPLEXER");
        if case == "environment" {
            command.env("COVEN_MULTIPLEXER", "unsupported");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn raw_input_caps_refuse_before_clock_or_root_creation() {
    struct NoClock;
    impl InitIo for NoClock {
        fn now(&self) -> Result<String> {
            panic!("oversized input must be refused before cloning team metadata")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing/child");
    for options in [
        Options {
            agents: "x".repeat(MAX_JSON + 1),
            ..blank()
        },
        Options {
            agents: "x".repeat(values::MAX_STRING + 1),
            ..blank()
        },
        Options {
            multiplexer: "x".repeat(MAX_TEMPLATE + 1),
            ..blank()
        },
        Options {
            example: "x".repeat(MAX_TEMPLATE + 1),
            ..Options::default()
        },
    ] {
        assert!(arguments_with(&root, &options, &NoClock).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    assert!(prepare(Path::new(&"x".repeat(MAX_TEMPLATE + 1)), &blank(), &NoClock).is_err());
    for options in [
        Options {
            name: Some("x".repeat(values::MAX_STRING + 1)),
            ..blank()
        },
        Options {
            opencode_command: "x".repeat(values::MAX_STRING + 1),
            ..blank()
        },
    ] {
        assert_eq!(
            arguments_with(&root, &options, &Fixed)
                .err()
                .unwrap()
                .code(),
            "unsupported_init_input_profile"
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}

#[test]
fn actual_partial_writes_and_file_sync_failures_preserve_exact_evidence() {
    struct PartialIo {
        writes: Cell<usize>,
        fail: usize,
        at_sync: bool,
    }
    impl InitIo for PartialIo {
        fn now(&self) -> Result<String> {
            Fixed.now()
        }
        fn uuid(&self) -> Result<String> {
            Fixed.uuid()
        }
        fn write(&self, file: &mut fs::File, bytes: &[u8]) -> Result<()> {
            let index = self.writes.get();
            self.writes.set(index + 1);
            if index == self.fail && !self.at_sync {
                file.write_all(&bytes[..bytes.len() / 2])?;
                return Err(Error("injected_partial_write"));
            }
            Ok(file.write_all(bytes)?)
        }
        fn sync_file(&self, file: &fs::File) -> Result<()> {
            if self.at_sync && self.writes.get() == self.fail + 1 {
                return Err(Error("injected_file_sync_failure"));
            }
            Ok(file.sync_all()?)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let options = Options::default();
    for at_sync in [false, true] {
        let probe = prepare(&temp.path().join("probe"), &options, &Fixed).unwrap();
        let count = probe
            .files
            .iter()
            .filter(|file| file.relative != "coven.json")
            .count();
        for fail in 0..count {
            let root = temp.path().join(format!("fault-{at_sync}-{fail}"));
            let staged = prepare(&root, &options, &Fixed).unwrap();
            let io = PartialIo {
                writes: Cell::new(0),
                fail,
                at_sync,
            };
            let result = arguments_with(&root, &options, &io).unwrap();
            assert_eq!(result.exit_code, 1);
            assert!(result.stdout.is_empty());
            assert!(
                result.warnings[0]
                    .contains(&format!("root={}", human(root.to_str().unwrap()).unwrap()))
            );
            assert!(result.warnings[0].contains("partial_file=true"));
            let files = staged
                .files
                .iter()
                .filter(|file| file.relative != "coven.json")
                .collect::<Vec<_>>();
            let previous = fail
                .checked_sub(1)
                .map_or("none", |index| files[index].relative.as_str());
            assert!(result.warnings[0].contains(&format!(
                "target={} last_completed={previous}",
                files[fail].relative
            )));
            assert!(result.warnings[0].contains(if at_sync {
                "phase=sync-file"
            } else {
                "phase=write-file"
            }));
            assert!(result.warnings[0].contains(if at_sync {
                "injected_file_sync_failure"
            } else {
                "injected_partial_write"
            }));
            assert!(!root.join("coven.json").exists());
            for (index, file) in staged
                .files
                .iter()
                .filter(|file| file.relative != "coven.json")
                .enumerate()
            {
                let path = root.join(&file.relative);
                if index > fail {
                    assert!(!path.exists());
                    continue;
                }
                let expected = if index == fail && !at_sync {
                    &file.bytes[..file.bytes.len() / 2]
                } else {
                    &file.bytes
                };
                assert_eq!(
                    fs::read(&path).unwrap(),
                    expected,
                    "{at_sync}/{fail}/{}",
                    file.relative
                );
            }
            assert!(root.join("state/.workspace.lock").exists());
            assert_eq!(
                arguments_with(&root, &blank(), &Fixed)
                    .err()
                    .unwrap()
                    .code(),
                "init_reserved_name_exists"
            );
        }
    }
}

#[test]
fn generated_basename_profile_is_checked_after_normalization_before_effects() {
    let temp = tempfile::tempdir().unwrap();
    for label in ["a".repeat(245), format!("  {}  ", "A".repeat(245))] {
        let root = temp.path().join(format!("valid-{}", label.len()));
        let options = Options {
            agents: label,
            ..Options::default()
        };
        let prepared = prepare(&root, &options, &Fixed).unwrap();
        let result = arguments_with(&root, &options, &Fixed).unwrap();
        assert_eq!(result.exit_code, 0);
        for file in prepared.files {
            assert_eq!(fs::read(root.join(file.relative)).unwrap(), file.bytes);
        }
        assert!(
            fs::metadata(root.join(format!("prompts/{}.start.txt", "a".repeat(245))))
                .unwrap()
                .len()
                > 0
        );
    }
    for label in [
        "a".repeat(246),
        format!("  {} ! {}  ", "A".repeat(244), "B"),
    ] {
        let root = temp.path().join("missing/child");
        let error = arguments_with(
            &root,
            &Options {
                agents: label,
                ..blank()
            },
            &Fixed,
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "unsupported_init_filename_profile");
        assert!(!temp.path().join("missing").exists());
    }
}

#[test]
fn directory_syncs_stop_at_changed_creation_chain() {
    struct Syncs(std::cell::RefCell<Vec<std::path::PathBuf>>);
    impl InitIo for Syncs {
        fn sync_directory(&self, path: &Path) -> Result<()> {
            self.0.borrow_mut().push(path.into());
            Ok(platform::sync_directory(path)?)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    for tail in ["existing", "new-parent/new-child/root"] {
        let root = temp.path().join(tail);
        if tail == "existing" {
            fs::create_dir(&root).unwrap();
        }
        let io = Syncs(std::cell::RefCell::new(vec![]));
        assert_eq!(arguments_with(&root, &blank(), &io).unwrap().exit_code, 0);
        let mut expected = DIRECTORIES
            .iter()
            .copied()
            .chain(std::iter::once("state"))
            .map(|dir| root.join(dir))
            .collect::<Vec<_>>();
        expected.push(root.clone());
        if tail != "existing" {
            expected.extend([
                root.parent().unwrap().to_owned(),
                temp.path().join("new-parent"),
                temp.path().to_owned(),
            ]);
        }
        assert_eq!(*io.0.borrow(), expected);
    }
}

#[test]
fn concurrent_root_creator_and_reservation_winner_have_operation_scoped_sync() {
    use std::{cell::RefCell, path::PathBuf};

    struct Winner {
        syncs: RefCell<Vec<PathBuf>>,
        committed: Cell<bool>,
    }
    impl InitIo for Winner {
        fn now(&self) -> Result<String> {
            Fixed.now()
        }
        fn uuid(&self) -> Result<String> {
            Fixed.uuid()
        }
        fn sync_directory(&self, path: &Path) -> Result<()> {
            self.syncs.borrow_mut().push(path.into());
            Ok(platform::sync_directory(path)?)
        }
        fn marker(&self, reservation: &bootstrap::Reservation, bytes: &[u8]) -> Result<Delivery> {
            let delivery = reservation.publish_marker(bytes)?;
            assert_eq!(delivery, Delivery::Committed);
            self.committed.set(true);
            Ok(delivery)
        }
    }
    struct Creator {
        syncs: RefCell<Vec<PathBuf>>,
        winner: Winner,
        result: RefCell<Option<TaskOutput>>,
        expected: RefCell<Option<Prepared>>,
    }
    impl InitIo for Creator {
        fn boundary(&self, operation: &str, root: &Path) -> Result<()> {
            if operation == "reserve" {
                // A created the missing chain but has not reserved state or
                // synced its directory entries. Run B to completion at this barrier.
                assert!(root.parent().unwrap().is_dir());
                assert_eq!(fs::read_dir(root)?.count(), 0);
                assert!(self.syncs.borrow().is_empty());
                let options = Options {
                    name: Some("reservation-winner".into()),
                    ..Options::default()
                };
                *self.expected.borrow_mut() = Some(prepare(root, &options, &Fixed)?);
                let result = arguments_with(root, &options, &self.winner)?;
                assert_eq!(result.exit_code, 0);
                assert!(workspace::load(root).is_ok());
                *self.result.borrow_mut() = Some(result);
            }
            Ok(())
        }
        fn sync_directory(&self, path: &Path) -> Result<()> {
            self.syncs.borrow_mut().push(path.into());
            Ok(platform::sync_directory(path)?)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("a/b/workspace");
    assert!(!temp.path().join("a").exists());
    let creator = Creator {
        syncs: RefCell::new(vec![]),
        winner: Winner {
            syncs: RefCell::new(vec![]),
            committed: Cell::new(false),
        },
        result: RefCell::new(None),
        expected: RefCell::new(None),
    };
    let loser = arguments_with(
        &root,
        &Options {
            name: Some("root-creator".into()),
            ..blank()
        },
        &creator,
    )
    .unwrap();
    let winner = creator.result.borrow_mut().take().unwrap();
    assert_eq!([loser.exit_code, winner.exit_code], [1, 0]);
    assert_eq!(
        [&loser, &winner]
            .iter()
            .filter(|output| output.exit_code == 0)
            .count(),
        1
    );
    assert!(loser.stdout.is_empty());
    assert!(loser.warnings[0].contains("phase=reserve-state"));
    assert!(loser.warnings[0].contains("last_completed=none partial_file=false"));
    assert!(winner.warnings.is_empty());
    assert!(creator.winner.committed.get());
    assert!(
        creator.syncs.borrow().is_empty(),
        "A never reaches creation-chain syncing after losing reservation"
    );
    let expected_syncs = DIRECTORIES
        .iter()
        .copied()
        .chain(std::iter::once("state"))
        .map(|directory| root.join(directory))
        .chain(std::iter::once(root.clone()))
        .collect::<Vec<_>>();
    assert_eq!(*creator.winner.syncs.borrow(), expected_syncs);
    // This characterizes operation scope, not power-loss durability: B does
    // not sync the outer entries A created, and A has no late sync on failure.
    for path in [
        temp.path(),
        temp.path().join("a").as_path(),
        root.parent().unwrap(),
    ] {
        assert!(
            !creator
                .winner
                .syncs
                .borrow()
                .iter()
                .any(|synced| synced == path)
        );
    }
    for file in creator.expected.borrow_mut().take().unwrap().files {
        assert_eq!(
            fs::read(root.join(&file.relative)).unwrap(),
            file.bytes,
            "{}",
            file.relative
        );
    }
    assert!(root.join("state/.workspace.lock").is_file());
    let state = workspace::load(&root).unwrap();
    assert_eq!(
        state
            .team
            .value()
            .as_object()
            .unwrap()
            .get("name")
            .unwrap()
            .as_str(),
        Some("reservation-winner")
    );
    assert_eq!(state.tasks.as_array().unwrap().len(), 6);
}

#[cfg(unix)]
#[test]
fn managed_directory_collisions_after_recheck_are_never_adopted() {
    use std::os::unix::fs::symlink;
    for kind in ["directory", "file", "dangling"] {
        struct Collide {
            kind: &'static str,
            directory: &'static str,
            fired: Cell<bool>,
        }
        impl InitIo for Collide {
            fn boundary(&self, operation: &str, root: &Path) -> Result<()> {
                if operation == "mkdir-managed" && !self.fired.replace(true) {
                    let target = root.join(self.directory);
                    match self.kind {
                        "directory" => {
                            fs::create_dir(&target)?;
                            fs::write(target.join("foreign"), b"foreign evidence")?;
                        }
                        "file" => fs::write(target, b"foreign evidence")?,
                        _ => symlink("missing-target", target)?,
                    }
                }
                Ok(())
            }
        }
        for directory in DIRECTORIES {
            let temp = tempfile::tempdir().unwrap();
            let io = Collide {
                kind,
                directory,
                fired: Cell::new(false),
            };
            let result = arguments_with(temp.path(), &blank(), &io).unwrap();
            assert_eq!(result.exit_code, 1);
            assert!(result.stdout.is_empty());
            assert!(result.warnings[0].contains(&format!(
                "phase=create-directory target={directory} last_completed=none partial_file=false"
            )));
            assert!(!temp.path().join("coven.json").exists());
            assert_eq!(fs::read_dir(temp.path().join("state")).unwrap().count(), 1);
            match kind {
                "directory" => {
                    assert_eq!(
                        fs::read(temp.path().join(directory).join("foreign")).unwrap(),
                        b"foreign evidence"
                    );
                    assert_eq!(
                        fs::read_dir(temp.path().join(directory)).unwrap().count(),
                        1
                    );
                }
                "file" => assert_eq!(
                    fs::read(temp.path().join(directory)).unwrap(),
                    b"foreign evidence"
                ),
                _ => assert_eq!(
                    fs::read_link(temp.path().join(directory)).unwrap(),
                    Path::new("missing-target")
                ),
            }
        }
    }
}

#[test]
fn progress_is_bounded_escaped_and_does_not_claim_a_foreign_file() {
    let long_root = format!("/{}", "a/".repeat(2000));
    let prepared = prepare(Path::new(&long_root), &blank(), &Fixed).unwrap();
    assert_eq!(prepared.diagnostic_root.len(), 2051);
    assert!(prepared.diagnostic_root.ends_with("..."));
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("line\n\u{1b}");
    fs::create_dir(&root).unwrap();
    let io = Collision {
        at: "open",
        relative: "state/agents.json",
        fired: Cell::new(false),
    };
    let result = arguments_with(&root, &blank(), &io).unwrap();
    let message = &result.warnings[0];
    assert!(message.contains("line\\n\\u001b"));
    assert!(message.contains(
        "phase=open-file target=state/agents.json last_completed=none partial_file=false"
    ));
    assert!(message.is_ascii());
    assert_eq!(message.lines().count(), 1);
    assert!(message.len() < 4096);
    assert_eq!(
        fs::read(root.join("state/agents.json")).unwrap(),
        b"concurrent evidence"
    );
}

#[cfg(unix)]
#[path = "overlap.rs"]
mod overlap;
