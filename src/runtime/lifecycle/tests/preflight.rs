use super::*;
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                files.insert(relative, None);
                visit(root, &path, files);
            } else {
                files.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn aggregate_launch_staging_refuses_restart_and_single_agent_start_before_effects() {
    for multiplexer in ["tmux", "herdr"] {
        let root = fixture(multiplexer);
        let agents: Vec<_> = (0..12).map(|index| format!("agent{index}")).collect();
        fs::write(
            root.path().join("coven.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "Test", "agents": agents, "multiplexer": multiplexer,
                "opencode_command": format!("opencode {}", "x".repeat(900_000)),
            }))
            .unwrap(),
        )
        .unwrap();
        // The reader still admits the team and each individual script fits its
        // existing asset profile; only the retained batch exceeds 8 MiB.
        let state = workspace::load(root.path()).unwrap();
        assert_eq!(state.team.agent_ids().unwrap().len(), 12);
        let before = snapshot(root.path());
        let fake = Fake::new(root.path(), vec![]);
        let (result, foreground) = run(
            root.path(),
            RuntimeAction::Restart,
            &fake,
            multiplexer == "tmux",
            false,
            false,
        );
        assert_eq!(
            result.unwrap_err().code(),
            "unsupported_runtime_staging_profile"
        );
        assert_eq!(snapshot(root.path()), before);
        assert!(foreground.calls.borrow().is_empty());
        let result = agent_change_using(
            root.path(),
            &["agent0".into()],
            AgentAction::Start,
            |r, _| {
                Ok(transport::Backend::injected(
                    r,
                    multiplexer == "tmux",
                    &fake,
                ))
            },
        );
        assert_eq!(
            result.unwrap_err().code(),
            "unsupported_runtime_staging_profile"
        );
        assert_eq!(snapshot(root.path()), before);
        assert!(fake.calls.borrow().is_empty());
        assert!(!root.path().join("state").exists());
        assert!(!root.path().join("logs").exists());
    }
}

#[test]
fn bad_workspace_configuration_creates_no_first_lock_or_event() {
    for config in [None, Some("broken [")] {
        let root = fixture("tmux");
        match config {
            None => fs::remove_file(root.path().join("coven.json")).unwrap(),
            Some(config) => fs::write(root.path().join("coven.toml"), config).unwrap(),
        }
        let before = snapshot(root.path());
        let fake = Fake::new(root.path(), vec![]);
        for action in [
            RuntimeAction::Plan,
            RuntimeAction::Up,
            RuntimeAction::Restart,
        ] {
            assert!(
                run(root.path(), action, &fake, true, false, false)
                    .0
                    .is_err()
            );
            assert_eq!(snapshot(root.path()), before);
            assert!(super::super::lifecycle(root.path(), action).is_err());
            assert_eq!(snapshot(root.path()), before);
        }
        for action in [AgentAction::Start, AgentAction::Stop, AgentAction::Restart] {
            assert!(
                agent_change_using(root.path(), &["Beta".into()], action, |r, _| Ok(
                    transport::Backend::injected(r, true, &fake)
                ))
                .is_err()
            );
            assert_eq!(snapshot(root.path()), before);
        }
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn invalid_batch_and_unsupported_scripts_create_no_first_locks() {
    let root = fixture("tmux");
    let fake = Fake::new(root.path(), vec![]);
    let before = snapshot(root.path());
    assert_eq!(
        agent_change_using(
            root.path(),
            &["Beta".into(), "unknown".into()],
            AgentAction::Restart,
            |r, _| Ok(transport::Backend::injected(r, true, &fake))
        )
        .unwrap_err()
        .code(),
        "unknown_agent"
    );
    assert_eq!(snapshot(root.path()), before);
    // Valid persisted input whose normalized command exceeds the existing asset
    // profile, rather than introducing a new runtime refusal profile.
    fs::write(
        root.path().join("coven.json"),
        serde_json::to_vec(&serde_json::json!({
            "name":"Test", "agents":["Beta","alpha"], "multiplexer":"tmux",
            "opencode_command":format!("oc {}", "\u{7f}".repeat(180_000))
        }))
        .unwrap(),
    )
    .unwrap();
    let before = snapshot(root.path());
    assert_eq!(
        run(root.path(), RuntimeAction::Up, &fake, true, false, false)
            .0
            .unwrap_err()
            .code(),
        "unsupported_refresh_template_profile"
    );
    assert_eq!(snapshot(root.path()), before);
    assert_eq!(
        agent_change_using(
            root.path(),
            &["Beta".into()],
            AgentAction::Start,
            |r, _| Ok(transport::Backend::injected(r, true, &fake))
        )
        .unwrap_err()
        .code(),
        "unsupported_refresh_template_profile"
    );
    assert_eq!(snapshot(root.path()), before);
}

#[test]
fn nul_launch_commands_reject_before_existing_runtime_restart_or_first_locks() {
    for multiplexer in ["tmux", "herdr"] {
        for command in ["opencode\0", "oc --flag\0"] {
            let root = fixture(multiplexer);
            fs::write(
                root.path().join("coven.json"),
                serde_json::to_vec(&serde_json::json!({
                    "name": "Test", "agents": ["Beta", "alpha"],
                    "multiplexer": multiplexer, "opencode_command": command,
                }))
                .unwrap(),
            )
            .unwrap();
            // The general reader still accepts this persisted team; refusal is
            // launch-specific and must precede even lookup of the existing runtime.
            let state = workspace::load(root.path()).unwrap();
            assert_eq!(
                transport::field(state.team.value(), "opencode_command"),
                Some(command)
            );
            let before = snapshot(root.path());
            let existing = if multiplexer == "tmux" {
                ok()
            } else {
                json(
                    serde_json::json!({"workspaces": [{"label": "coven-test", "workspace_id": "w1"}]}),
                )
            };
            let fake = Fake::new(root.path(), vec![existing]);
            let (result, foreground) = run(
                root.path(),
                RuntimeAction::Restart,
                &fake,
                multiplexer == "tmux",
                false,
                false,
            );
            assert!(result.is_err(), "{multiplexer}: {command:?}");
            assert!(
                fake.calls.borrow().is_empty(),
                "no lookup, kill/close, or creation"
            );
            assert!(foreground.calls.borrow().is_empty());
            assert_eq!(snapshot(root.path()), before);
            assert!(!root.path().join("state").exists(), "no first lock files");
            assert!(!root.path().join("logs").exists(), "no requested event");
        }
    }
}

#[cfg(unix)]
#[test]
fn bad_monitor_environment_creates_no_first_lock_or_requested_event() {
    const CHILD: &str = "COVEN_LIFECYCLE_BAD_MONITOR_ENV_TEST";
    if std::env::var_os(CHILD).is_none() {
        use std::os::unix::ffi::OsStringExt;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runtime::lifecycle::tests::preflight::bad_monitor_environment_creates_no_first_lock_or_requested_event", "--nocapture"])
            .env(CHILD, "1").env("COVEN_COMMAND", std::ffi::OsString::from_vec(vec![0xff]))
            .env_remove("COVEN_MULTIPLEXER").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = fixture("tmux");
    let before = snapshot(root.path());
    let fake = Fake::new(root.path(), vec![]);
    for action in [RuntimeAction::Up, RuntimeAction::Restart] {
        assert_eq!(
            run(root.path(), action, &fake, true, false, false)
                .0
                .unwrap_err()
                .code(),
            "unsupported_refresh_command_environment_profile"
        );
        assert_eq!(snapshot(root.path()), before);
    }
}

#[test]
fn late_bad_configuration_is_reloaded_before_workspace_lock_or_event() {
    let root = fixture("tmux");
    let fake = Fake::new(root.path(), vec![]);
    let foreground = Foreground {
        root: root.path().to_owned(),
        calls: RefCell::new(Vec::new()),
    };
    let mut before = None;
    let result = lifecycle_admitting(
        root.path(),
        RuntimeAction::Up,
        |r, _| Ok(transport::Backend::injected(r, true, &fake)),
        &foreground,
        (false, false),
        false,
        |root| {
            let gate = TransportGuard::shared(root, TIMEOUT)?;
            fs::write(root.join("coven.toml"), "broken [").unwrap();
            before = Some(snapshot(root));
            Ok(gate)
        },
    );
    assert_eq!(result.unwrap_err().code(), "invalid_workspace_toml");
    assert_eq!(snapshot(root.path()), before.unwrap());
    assert!(!root.path().join("state/.workspace.lock").exists());
    assert!(fake.calls.borrow().is_empty());
}
