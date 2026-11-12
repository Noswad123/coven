#![cfg(unix)]
//! Public CLI privacy admission; no native providers or foreign UID operations.
#[path = "support/physical_temp.rs"]
mod physical_temp;

use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{MetadataExt, PermissionsExt, symlink},
        net::UnixListener,
    },
    path::Path,
    process::{Command, Output, Stdio},
};

fn serve(workspace: &Path, socket: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_coven"))
        .args(["agent", "serve"])
        .arg(workspace)
        .arg("--socket")
        .arg(socket)
        .args(["--duration", "1"])
        .output()
        .unwrap()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cli_unsafe_root_refuses_before_state_namespace_creation() {
    for mode in [0o720, 0o702, 0o777] {
        let base = physical_temp::tempdir();
        let root = base.path().join("workspace");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("coven.json"), b"marker").unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(mode)).unwrap();
        let socket = base.path().join("receiver.sock");
        let output = serve(&root, &socket);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("agent_unsafe_parent"));
        assert!(!root.join("state").exists());
        assert!(!socket.exists());
        assert_eq!(fs::metadata(&root).unwrap().mode() & 0o7777, mode);
        assert_eq!(fs::read(root.join("coven.json")).unwrap(), b"marker");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cli_unsafe_state_and_alias_paths_are_not_repaired_or_followed() {
    for fixture in ["state-mode", "state-file", "state-alias", "root-alias"] {
        let base = physical_temp::tempdir();
        let root = base.path().join("workspace");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("coven.json"), b"marker").unwrap();
        let state = root.join("state");
        let foreign = base.path().join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("sentinel"), b"unchanged").unwrap();
        let selected = match fixture {
            "state-mode" => {
                fs::create_dir(&state).unwrap();
                fs::set_permissions(&state, fs::Permissions::from_mode(0o770)).unwrap();
                root.clone()
            }
            "state-file" => {
                fs::write(&state, b"not-a-directory").unwrap();
                root.clone()
            }
            "state-alias" => {
                symlink(&foreign, &state).unwrap();
                root.clone()
            }
            "root-alias" => {
                let alias = base.path().join("alias");
                symlink(&root, &alias).unwrap();
                alias
            }
            _ => unreachable!(),
        };
        // Preserve the malicious spelling: canonicalizing here would hide the bug.
        let socket = base.path().join("receiver.sock");
        let output = serve(&selected, &socket);
        assert!(!output.status.success(), "fixture={fixture}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("agent_delivery_uncertain"));
        assert!(!socket.exists(), "fixture={fixture}");
        assert!(!state.join(".workspace.lock").exists());
        assert!(!state.join(".agent-management.receiver.lock").exists());
        assert!(!state.join("agent-management.json").exists());
        assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
        if fixture == "state-mode" {
            assert_eq!(fs::metadata(&state).unwrap().mode() & 0o7777, 0o770);
        }
        if fixture == "state-file" {
            assert_eq!(fs::read(&state).unwrap(), b"not-a-directory");
        }
        if fixture == "root-alias" {
            assert!(!state.exists());
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cli_socket_privacy_refusal_never_connects_or_leaks_request() {
    for fixture in [
        "parent-mode",
        "leaf-mode",
        "parent-alias",
        "leaf-alias",
        "hardlink",
    ] {
        let base = physical_temp::tempdir();
        let parent = base.path().join("private");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = parent.join("receiver.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let selected = match fixture {
            "parent-mode" => {
                fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
                socket.clone()
            }
            "leaf-mode" => {
                fs::set_permissions(&socket, fs::Permissions::from_mode(0o660)).unwrap();
                socket.clone()
            }
            "parent-alias" => {
                let alias = base.path().join("alias");
                symlink(&parent, &alias).unwrap();
                alias.join("receiver.sock")
            }
            "leaf-alias" => {
                let alias = parent.join("alias.sock");
                symlink(&socket, &alias).unwrap();
                alias
            }
            "hardlink" => {
                fs::hard_link(&socket, parent.join("other.sock")).unwrap();
                socket.clone()
            }
            _ => unreachable!(),
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_coven"))
            .args(["agent", "report", "--socket"])
            .arg(&selected)
            .args(["--provider", "claude", "--event", "Stop"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(
            br#"{"session_id":"PRIVATE-SID","hook_event_name":"Stop","prompt":"PRIVATE-PROMPT"}"#,
        ).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success(), "fixture={fixture}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("agent_delivery_uncertain"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE"));
        assert_eq!(
            listener.accept().err().unwrap().kind(),
            std::io::ErrorKind::WouldBlock,
            "fixture={fixture}"
        );
        assert_eq!(
            fs::metadata(&socket).unwrap().nlink(),
            if fixture == "hardlink" { 2 } else { 1 }
        );
        assert_eq!(
            fs::metadata(&socket).unwrap().mode() & 0o7777,
            if fixture == "leaf-mode" { 0o660 } else { 0o600 }
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn cli_owned_directory_acl_is_refused_before_private_file_creation() {
    for role in ["root", "state"] {
        let base = physical_temp::tempdir();
        let root = base.path().join("workspace");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("coven.json"), b"marker").unwrap();
        let selected = if role == "state" {
            let state = root.join("state");
            fs::create_dir(&state).unwrap();
            state
        } else {
            root.clone()
        };
        let ace = format!("user:{} allow read", unsafe { libc::geteuid() });
        assert!(
            Command::new("/bin/chmod")
                .args(["+a", &ace])
                .arg(&selected)
                .status()
                .unwrap()
                .success()
        );
        let socket = base.path().join("receiver.sock");
        let output = serve(&root, &socket);
        assert!(!output.status.success(), "role={role}");
        assert!(!socket.exists());
        assert!(!root.join("state/.workspace.lock").exists());
        assert!(!root.join("state/.agent-management.receiver.lock").exists());
        assert!(!root.join("state/agent-management.json").exists());
        if role == "root" {
            assert!(!root.join("state").exists());
        }
        assert!(
            Command::new("/bin/chmod")
                .arg("-N")
                .arg(&selected)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn unsupported_unix_receiver_reports_known_platform_error() {
    let base = physical_temp::tempdir();
    fs::write(base.path().join("coven.json"), b"marker").unwrap();
    let output = serve(base.path(), &base.path().join("receiver.sock"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent_unsafe_parent"));
    assert!(!base.path().join("state").exists());
}
