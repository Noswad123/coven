//! Actual native CLI handshake: no workspace, configuration, or runtime needed.
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn command(cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command
        .env_clear()
        .current_dir(cwd)
        .args(["doctor", "status"]);
    command
}

fn report(command: &mut Command) -> Value {
    let output = command.arg("--json").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(output.stdout.ends_with(b"\n"));
    assert_eq!(output.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let versions: &[u32] = if cfg!(any(target_os = "linux", target_os = "macos")) {
        &[1]
    } else {
        &[]
    };
    assert_eq!(
        value,
        json!({
            "contract": "coven.doctor-status.v1",
            "version": env!("CARGO_PKG_VERSION"),
            "executable": Path::new(env!("CARGO_BIN_EXE_coven")).canonicalize().unwrap(),
            "capabilities": {
                "integration_runtime": {"versions": versions},
                "native_event_reporting": {"versions": versions}
            },
            "defaults": value["defaults"],
            "diagnostics": value["diagnostics"]
        })
    );
    assert_eq!(value["defaults"].as_object().unwrap().len(), 1);
    value
}

#[test]
fn missing_and_invalid_environment_is_successful_metadata() {
    let temp = tempfile::tempdir().unwrap();
    for (home, xdg, diagnostic) in [
        (None, None, "integration_home_required"),
        (None, Some(""), "integration_home_required"),
        (Some(""), None, "integration_home_required"),
        (Some("relative"), None, "integration_invalid_path"),
        (Some("relative"), Some(""), "integration_invalid_path"),
        (None, Some("relative"), "integration_invalid_path"),
    ] {
        let mut cmd = command(temp.path());
        if let Some(home) = home {
            cmd.env("HOME", home);
        }
        if let Some(xdg) = xdg {
            cmd.env("XDG_STATE_HOME", xdg);
        }
        let value = report(&mut cmd);
        assert_eq!(value["defaults"]["integration_state_root"], Value::Null);
        assert_eq!(value["diagnostics"], json!([diagnostic]));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn default_root_policy_is_reused_without_creating_anything() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("absent-home");
    let xdg = temp.path().join("absent-state");
    for empty_xdg in [false, true] {
        let mut cmd = command(temp.path());
        cmd.env("HOME", &home);
        if empty_xdg {
            cmd.env("XDG_STATE_HOME", "");
        }
        let value = report(&mut cmd);
        assert_eq!(
            value["defaults"]["integration_state_root"],
            json!(home.join(".local/state/coven/integration"))
        );
        assert_eq!(value["diagnostics"], json!([]));
    }
    let value = report(command(temp.path()).env("XDG_STATE_HOME", &xdg));
    assert_eq!(
        value["defaults"]["integration_state_root"],
        json!(xdg.join("coven/integration"))
    );
    assert_eq!(value["diagnostics"], json!([]));
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn human_output_displays_the_same_facts_without_json_or_logs() {
    let temp = tempfile::tempdir().unwrap();
    let output = command(temp.path()).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let human = String::from_utf8(output.stdout).unwrap();
    for fact in [
        "coven.doctor-status.v1",
        env!("CARGO_PKG_VERSION"),
        Path::new(env!("CARGO_BIN_EXE_coven"))
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap(),
        "Integration runtime versions:",
        "Native event reporting versions:",
        "Default integration state root: unavailable",
        "integration_home_required",
    ] {
        assert!(human.contains(fact), "{human}");
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn selector_symlink_reports_native_identity_not_launcher() {
    let temp = tempfile::tempdir().unwrap();
    let selector = temp.path().join("dev-selector");
    let wrapper = temp.path().join("wrapper");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_coven"), &selector).unwrap();
    std::os::unix::fs::symlink(&selector, &wrapper).unwrap();
    let mut cmd = Command::new(&wrapper);
    cmd.env_clear()
        .current_dir(temp.path())
        .args(["doctor", "status"]);
    report(&mut cmd);
    let mut shell = Command::new("/bin/sh");
    shell
        .env_clear()
        .current_dir(temp.path())
        .args(["-c", "exec \"$@\"", "doctor-wrapper"])
        .arg(&wrapper)
        .args(["doctor", "status"]);
    report(&mut shell);
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn active_kitsune_environment_is_irrelevant_and_never_connects() {
    use std::os::unix::net::UnixListener;

    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("kit.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let baseline = report(&mut command(temp.path()));
    let mut cmd = command(temp.path());
    cmd.env("KITSUNE_SOCKET_PATH", &socket)
        .env("KITSUNE_AGENT_MANAGEMENT", "external")
        .env("KITSUNE_INTEGRATION_SOURCE", "coven")
        .env(
            "KITSUNE_INTEGRATION_REPORT_COMMAND",
            serde_json::to_string(&[
                env!("CARGO_BIN_EXE_coven"),
                "agent",
                "report-native",
                "--from-kitsune",
            ])
            .unwrap(),
        )
        .env("KITSUNE_PANE_ID", "pane:active")
        .env("KITSUNE_PID", std::process::id().to_string())
        .env("KITSUNE_CONFIG_PATH", temp.path().join("missing-config"))
        .env("COVEN_SOCKET", &socket)
        .env("RUST_LOG", "trace");
    assert_eq!(report(&mut cmd), baseline);
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}
