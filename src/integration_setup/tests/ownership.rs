use super::*;

fn custom_install() -> Fixture {
    let mut f = Fixture::new();
    let executable = f.dir.path().join("custom-bin");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    f.request.executable = Some(executable);
    let state = f.request.state_root.as_ref().unwrap();
    fs::create_dir(state).unwrap();
    fs::set_permissions(state, fs::Permissions::from_mode(0o700)).unwrap();
    f.write(b"# exact original\nx = 1\n", 0o640);
    f.run().unwrap();
    f
}

#[test]
fn omitted_pins_inspect_and_disable_recorded_custom_install() {
    let mut f = custom_install();
    let receipt = f.receipt();
    let post = fs::read(f.config()).unwrap();
    f.request.executable = None;
    f.request.state_root = None;
    for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
        f.request.operation = operation;
        f.request.apply = false;
        let output = f.run().unwrap();
        assert_eq!(output["executable"], json!(receipt.executable));
        assert_eq!(output["state_root"], json!(receipt.state_root));
        assert_eq!(output["command"][0], json!(receipt.executable));
        assert_eq!(output["command"][4], json!(receipt.state_root));
        assert_eq!(output["report_command"][5], json!(receipt.state_root));
        assert_eq!(fs::read(f.config()).unwrap(), post);
    }
    f.request.apply = true;
    f.run().unwrap();
    assert_eq!(
        fs::read(f.config()).unwrap(),
        receipt.preimage.unwrap().bytes
    );
    assert_eq!(f.receipt().stage, Stage::Inactive);
    assert_eq!(fs::read_dir(receipt.state_root).unwrap().count(), 0);
}

#[test]
fn changed_explicit_pins_conflict_for_plan_enable_and_disable() {
    let mut f = custom_install();
    let receipt = f.receipt();
    let post = fs::read(f.config()).unwrap();
    let journal = fs::read(f.dir.path().join(RECEIPT)).unwrap();
    for change_executable in [false, true] {
        f.request.executable = None;
        f.request.state_root = None;
        if change_executable {
            f.request.executable = Some(f.dir.path().join("different-bin"));
        } else {
            f.request.state_root = Some(f.dir.path().join("different-root"));
        }
        for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
            f.request.operation = operation;
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_spec_conflict"
            );
            assert_eq!(fs::read(f.config()).unwrap(), post);
            assert_eq!(fs::read(f.dir.path().join(RECEIPT)).unwrap(), journal);
        }
    }
    assert_eq!(fs::read_dir(receipt.state_root).unwrap().count(), 0);
}

#[test]
fn unsafe_recorded_root_is_diagnostic_for_active_plan_but_does_not_block_undo() {
    let mut f = custom_install();
    let receipt = f.receipt();
    fs::set_permissions(&receipt.state_root, fs::Permissions::from_mode(0o755)).unwrap();
    let before = fs::metadata(&receipt.state_root).unwrap();
    f.request.executable = None;
    f.request.state_root = None;
    f.request.operation = Operation::Plan;
    let output = f.run().unwrap();
    assert_eq!(output["runtime_ready"], false);
    assert_eq!(
        output["runtime_readiness_error"],
        "integration_setup_unsafe_state_root"
    );
    assert_eq!(output["state_root"], json!(receipt.state_root));
    assert!(!output["warnings"].as_array().unwrap().is_empty());
    f.request.operation = Operation::Enable;
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_unsafe_state_root"
    );
    f.request.operation = Operation::Disable;
    for apply in [false, true] {
        f.request.apply = apply;
        let output = f.run().unwrap();
        assert!(output["runtime_ready"].is_null());
        assert_eq!(output["state_root"], json!(receipt.state_root));
    }
    assert_eq!(
        fs::read(f.config()).unwrap(),
        receipt.preimage.unwrap().bytes
    );
    let after = fs::metadata(&receipt.state_root).unwrap();
    assert_eq!(
        (after.ino(), after.mode(), after.ctime(), after.ctime_nsec()),
        (
            before.ino(),
            before.mode(),
            before.ctime(),
            before.ctime_nsec()
        )
    );
    assert_eq!(fs::read_dir(receipt.state_root).unwrap().count(), 0);
}

#[test]
fn deleted_recorded_executable_does_not_block_undo_or_bypass_user_edit_fence() {
    for explicit_obsolete_pin in [false, true] {
        let mut f = custom_install();
        let receipt = f.receipt();
        let post = fs::read(f.config()).unwrap();
        fs::remove_file(&receipt.executable).unwrap();
        f.request.executable = explicit_obsolete_pin.then(|| receipt.executable.clone());
        f.request.state_root = None;
        f.request.operation = Operation::Plan;
        assert_eq!(f.run().unwrap()["runtime_ready"], false);
        f.request.operation = Operation::Enable;
        assert!(f.run().is_err());
        f.request.operation = Operation::Disable;
        f.request.apply = false;
        assert_eq!(f.run().unwrap()["executable"], json!(receipt.executable));
        let mut edited = post.clone();
        edited.extend_from_slice(b"\n# later user edit\n");
        f.write(&edited, 0o640);
        let journal = fs::read(f.dir.path().join(RECEIPT)).unwrap();
        for apply in [false, true] {
            f.request.apply = apply;
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_config_edited"
            );
            assert_eq!(fs::read(f.config()).unwrap(), edited);
            assert_eq!(fs::read(f.dir.path().join(RECEIPT)).unwrap(), journal);
        }
        // Restoring the exact known postimage makes guarded undo possible again.
        f.write(&post, 0o640);
        f.run().unwrap();
        assert_eq!(
            fs::read(f.config()).unwrap(),
            receipt.preimage.unwrap().bytes
        );
        assert!(!receipt.executable.exists());
        assert_eq!(fs::read_dir(receipt.state_root).unwrap().count(), 0);
    }
}

#[test]
fn unowned_disable_never_selects_or_validates_runtime_defaults() {
    let mut f = Fixture::new();
    f.write(b"# unmanaged config\n[invalid toml", 0o640);
    f.request.operation = Operation::Disable;
    f.request.executable = Some(f.dir.path().join("missing-bin"));
    f.request.state_root = Some(f.dir.path().to_owned());
    fs::set_permissions(f.dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    for apply in [false, true] {
        f.request.apply = apply;
        let output = f.run().unwrap();
        assert_eq!(output["noop"], true);
        assert!(output["executable"].is_null());
        assert!(output["state_root"].is_null());
        assert!(output["command"].is_null());
        assert!(output["report_command"].is_null());
        assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn managed_pins_ignore_defaults_child() {
    let Some(config) = std::env::var_os("COVEN_SETUP_MANAGED_CONFIG") else {
        return;
    };
    let config = PathBuf::from(config);
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(config.parent().unwrap().join(RECEIPT)).unwrap()).unwrap();
    let mut request = Request {
        operation: Operation::Plan,
        kitsune_config: None,
        executable: None,
        state_root: None,
        apply: false,
    };
    for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
        request.operation = operation;
        let output: serde_json::Value =
            serde_json::from_str(&arguments(&request, true).unwrap().stdout).unwrap();
        assert_eq!(output["executable"], json!(receipt.executable));
        assert_eq!(output["state_root"], json!(receipt.state_root));
    }
    request.apply = true;
    arguments(&request, true).unwrap();
    assert_eq!(fs::read(config).unwrap(), receipt.preimage.unwrap().bytes);
}

#[test]
fn managed_pins_ignore_unrelated_unsafe_defaults_and_missing_home() {
    for no_home in [false, true] {
        let f = custom_install();
        let default_base = f.dir.path().join("d");
        let default_root = default_base.join("coven/integration");
        fs::create_dir_all(&default_root).unwrap();
        fs::set_permissions(&default_root, fs::Permissions::from_mode(0o755)).unwrap();
        let before = fs::metadata(&default_root).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("KITSUNE_CONFIG_PATH", f.config())
            .env("COVEN_SETUP_MANAGED_CONFIG", f.config())
            .args([
                "--exact",
                "integration_setup::tests::ownership::managed_pins_ignore_defaults_child",
                "--nocapture",
            ]);
        if !no_home {
            command
                .env("HOME", f.dir.path())
                .env("XDG_STATE_HOME", &default_base);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let after = fs::metadata(&default_root).unwrap();
        assert_eq!(
            (after.ino(), after.mode(), after.ctime(), after.ctime_nsec()),
            (
                before.ino(),
                before.mode(),
                before.ctime(),
                before.ctime_nsec()
            )
        );
        assert_eq!(fs::read_dir(default_root).unwrap().count(), 0);
    }
}
