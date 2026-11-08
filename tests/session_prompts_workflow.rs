//! Native-only private socket fixtures. No Python or live provider children.
#![cfg(unix)]
#[path = "support/session_prompt_fixture.rs"]
mod fixture;
use fixture::{REQUEST, RUNTIME, Server, frame};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command.env("PYTHONDONTWRITEBYTECODE", "1");
    command
}
fn report(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn prompt(root: &Path, socket: &Path, file: &Path, execute: bool) -> Command {
    let mut cmd = command();
    cmd.args(["session", "prompt"])
        .arg(root)
        .arg("--native-socket")
        .arg(socket)
        .args([
            "--expected-runtime",
            RUNTIME,
            "--session",
            "ses_fixture",
            "--request-id",
            REQUEST,
            "--prompt-file",
        ])
        .arg(file)
        .arg("--json");
    if execute {
        cmd.arg("--execute");
    }
    cmd
}
fn workspace() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    let file = temp.path().join("prompt");
    let output = command()
        .arg("init")
        .arg(&root)
        .args(["--blank", "--agents", "operator"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(
        &file,
        "Literal λ 🦊\r\n$(not a command); @not-an-attachment\n",
    )
    .unwrap();
    (temp, root, file)
}

#[test]
fn pure_preview_needs_no_files_or_endpoint() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("missing");
    let file = temp.path().join("absent");
    let value = report(
        prompt(&root, Path::new("/missing/socket"), &file, false)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(value["delivery"], "not_attempted");
    assert_eq!(value["authorization"], "none");
    assert!(value["prompt_sha256"].is_null());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn empty_and_nonexistent_workspace_previews_have_no_effects() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("absent");
    for workspace in ["", "/notexists"] {
        let value = report(
            prompt(
                Path::new(workspace),
                Path::new("/missing/socket"),
                &file,
                false,
            )
            .current_dir(temp.path())
            .output()
            .unwrap(),
            0,
        );
        assert_eq!(
            value["workspace"],
            if workspace.is_empty() {
                json!(temp.path())
            } else {
                json!(workspace)
            }
        );
        assert_eq!(value["outcome"], "preview");
        assert_eq!(value["delivery"], "not_attempted");
        assert_eq!(value["authorization"], "none");
        assert!(value["prompt_sha256"].is_null());
        assert!(value["prompt_bytes"].is_null());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}

#[test]
fn capabilities_are_workspace_independent() {
    let server = Server::normal();
    let value = report(
        command()
            .args(["session", "prompt-capabilities", "--native-socket"])
            .arg(&server.path)
            .arg("--json")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(value["runtime_id"], RUNTIME);
    assert_eq!(value["authorization"], "none");
    assert_eq!(server.methods(), ["capabilities"]);
}

#[test]
fn symlinked_immediate_parent_is_rejected_without_contact_for_all_spellings() {
    use std::os::unix::fs::symlink;
    let (temp, root, file) = workspace();
    let server = Server::normal();
    let alias = temp.path().join("alias");
    symlink(server.path.parent().unwrap(), &alias).unwrap();
    for separators in ["/", "//", "///"] {
        let socket = format!(
            "{}{separators}{}",
            alias.display(),
            server.path.file_name().unwrap().to_str().unwrap()
        );
        let value = report(
            command()
                .args(["session", "prompt-capabilities", "--native-socket"])
                .arg(&socket)
                .arg("--json")
                .output()
                .unwrap(),
            1,
        );
        assert_eq!(value["assessment"], "unavailable");
        assert_eq!(value["diagnostics"], json!(["unsafe_native_parent"]));
        assert!(server.methods().is_empty());

        let value = report(
            prompt(&root, Path::new(&socket), &file, true)
                .output()
                .unwrap(),
            1,
        );
        assert_eq!(value["outcome"], "unavailable");
        assert_eq!(value["delivery"], "unsent");
        assert_eq!(value["native_prompt_acceptance"], "unestablished");
        assert!(value["native_message_id"].is_null());
        assert_eq!(value["diagnostics"], json!(["unsafe_native_parent"]));
        assert!(server.methods().is_empty());
    }
}

#[test]
fn exact_admission_preserves_text_and_reports_only_digest() {
    fn snapshot(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut entries = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                entries.insert(path.clone(), Vec::new());
                entries.extend(snapshot(&path));
            } else {
                entries.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        entries
    }
    let (_temp, root, file) = workspace();
    // Match the original setup's already-created transport gate before snapshotting.
    std::fs::write(root.join("state/.runtime-transport.lock"), b"").unwrap();
    let before = snapshot(&root);
    let server = Server::normal();
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        0,
    );
    assert_eq!(value["delivery"], "accepted");
    assert_eq!(value["native_prompt_acceptance"], "accepted");
    assert_eq!(value["native_message_id"], "msg_fixture");
    assert_eq!(value["native_worker_acceptance"], "unestablished");
    assert_eq!(value["job_completion"], "unestablished");
    assert!(!value.to_string().contains("not a command"));
    assert_eq!(server.methods(), ["capabilities", "submit"]);
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests[1]["params"]["text"],
        std::fs::read_to_string(&file).unwrap()
    );
    assert_eq!(requests[1]["id"], REQUEST);
    assert_eq!(snapshot(&root), before);
}

#[test]
fn runtime_mismatch_and_unsupported_capability_never_submit() {
    let (_temp, root, file) = workspace();
    for case in ["runtime", "version", "contract", "max_prompt_bytes"] {
        let server = Server::new(move |_, response| {
            if case == "runtime" {
                response["runtime_id"] = json!(REQUEST);
                response["result"]["runtime_id"] = json!(REQUEST);
            } else if case == "contract" {
                response["result"][case] = json!("other");
            } else {
                response["result"][case] = json!(true);
            }
            Some(frame(response))
        });
        let value = report(
            prompt(&root, &server.path, &file, true).output().unwrap(),
            1,
        );
        assert_eq!(value["delivery"], "unsent");
        if case == "runtime" {
            assert_eq!(value["diagnostics"], json!(["native_runtime_mismatch"]));
        }
        assert_eq!(server.methods(), ["capabilities"]);
    }
}

#[test]
fn lost_bad_duplicate_and_safe_rejection_are_not_retried() {
    let (_temp, root, file) = workspace();
    for mode in 0..7 {
        let server = Server::new(move |request, response| {
            if request["method"] == "submit" {
                match mode {
                    0 => return None,
                    1 => response["result"]["prompt_bytes"] = json!(true),
                    2..=4 | 6 => {
                        response.as_object_mut().unwrap().remove("result");
                        let (code, delivery) = match mode {
                            2 => ("request_already_seen", "uncertain"),
                            3 => ("session_unavailable", "rejected"),
                            6 => ("admission_uncertain", "uncertain"),
                            _ => ("secret-peer-text", "rejected"),
                        };
                        response["error"] =
                            json!({"code":code,"delivery":delivery,"message":"sensitive"});
                    }
                    _ => return Some(vec![b'x'; 65537]),
                }
            }
            Some(frame(response))
        });
        let value = report(
            prompt(&root, &server.path, &file, true).output().unwrap(),
            if mode == 3 { 1 } else { 2 },
        );
        assert_eq!(
            value["delivery"],
            if mode == 3 { "rejected" } else { "uncertain" }
        );
        assert!(
            !value.to_string().contains("sensitive")
                && !value.to_string().contains("secret-peer-text")
        );
        assert_eq!(server.methods(), ["capabilities", "submit"]);
        assert_eq!(value["native_prompt_acceptance"], "unestablished");
    }
}

#[test]
fn whole_native_payload_validated_without_narrowing_ignored_bigints() {
    let (_temp, root, file) = workspace();
    for extension in [
        "9".repeat(4000),
        "1e999".into(),
        "{\"duplicate\":0,\"duplicate\":1}".into(),
    ] {
        let valid = extension.starts_with('9');
        let server = Server::new(move |_, response| {
            let raw = serde_json::to_string(response).unwrap();
            Some(format!("{{\"ignored\":{extension},{}\n", &raw[1..]).into_bytes())
        });
        let value = report(
            prompt(&root, &server.path, &file, true).output().unwrap(),
            if valid { 0 } else { 1 },
        );
        assert_eq!(value["delivery"], if valid { "accepted" } else { "unsent" });
    }
}

#[test]
fn strict_permissions_and_prompt_bounds_prevent_contact() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_temp, root, file) = workspace();
    let server = Server::normal();
    std::fs::set_permissions(
        server.path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_eq!(
        report(
            prompt(&root, &server.path, &file, true).output().unwrap(),
            1
        )["delivery"],
        "unsent"
    );
    std::fs::set_permissions(
        server.path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    for mode in [0o400, 0o660, 0o666] {
        std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            report(
                prompt(&root, &server.path, &file, true).output().unwrap(),
                1
            )["delivery"],
            "unsent"
        );
    }
    std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o600)).unwrap();
    for bytes in [
        vec![],
        vec![b'x'; 65537],
        vec![0xff],
        b" \t\n".to_vec(),
        b"bad\0text".to_vec(),
    ] {
        std::fs::write(&file, bytes).unwrap();
        assert_eq!(
            report(
                prompt(&root, &server.path, &file, true).output().unwrap(),
                1
            )["delivery"],
            "unsent"
        );
    }
    std::fs::remove_file(&file).unwrap();
    symlink(root.join("goal.md"), &file).unwrap();
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        1,
    );
    assert_eq!(value["delivery"], "unsent");
    assert!(server.methods().is_empty());
}

#[test]
fn parent_retarget_preserving_leaf_inode_is_unsent() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};
    let (_temp, root, file) = workspace();
    let selected = Arc::new(Mutex::new(std::path::PathBuf::new()));
    let captured = selected.clone();
    let server = Server::new(move |request, response| {
        assert_eq!(request["method"], "capabilities");
        let socket = captured.lock().unwrap();
        let parent = socket.parent().unwrap();
        let old = parent.join("original");
        // Move the original parent aside, recreate it, and preserve the leaf inode.
        let moved = parent.with_extension("moved");
        std::fs::rename(parent, &moved).unwrap();
        std::fs::create_dir(parent).unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(moved.join(socket.file_name().unwrap()), &*socket).unwrap();
        std::fs::rename(&moved, &old).unwrap();
        Some(frame(response))
    });
    *selected.lock().unwrap() = server.path.clone();
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        1,
    );
    assert_eq!(value["delivery"], "unsent");
    assert_eq!(value["diagnostics"], json!(["native_endpoint_changed"]));
    assert_eq!(server.methods(), ["capabilities"]);
}

#[test]
fn response_budget_is_shared_across_both_exchanges() {
    let (_temp, root, file) = workspace();
    let server = Server::new(|request, response| {
        let count = if request["method"] == "capabilities" {
            15
        } else {
            2
        };
        response["ignored"] = json!(vec!["x".repeat(4000); count]);
        Some(frame(response))
    });
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        2,
    );
    assert_eq!(value["delivery"], "uncertain");
    assert_eq!(value["diagnostics"], json!(["response_limit"]));
    assert_eq!(server.methods(), ["capabilities", "submit"]);
}

#[test]
fn interruption_is_confined_to_child_and_never_retries() {
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;
    let (_temp, root, file) = workspace();
    for method in ["capabilities", "submit"] {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = Server::new(move |request, response| {
            if request["method"] == method {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                None
            } else {
                Some(frame(response))
            }
        });
        let child = prompt(&root, &server.path, &file, true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let output = child.wait_with_output().unwrap();
        release_tx.send(()).unwrap();
        let value = report(output, 130);
        assert_eq!(value["interrupted"], true);
        assert_eq!(
            value["delivery"],
            if method == "submit" {
                "uncertain"
            } else {
                "unsent"
            }
        );
        assert_eq!(
            server.methods().len(),
            if method == "submit" { 2 } else { 1 }
        );
    }
}

#[test]
fn transport_gate_is_held_and_workspace_lock_is_free_during_io() {
    use std::os::fd::AsRawFd;
    let (_temp, root, file) = workspace();
    let captured = root.clone();
    let server = Server::new(move |request, response| {
        if request["method"] == "submit" {
            let gate = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(captured.join("state/.runtime-transport.lock"))
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(gate.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                -1
            );
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::WouldBlock
            );
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(captured.join("state/.workspace.lock"))
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
        }
        Some(frame(response))
    });
    report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        0,
    );
}

#[test]
fn enabled_invalid_and_orphan_launch_authority_block_before_contact() {
    for mode in ["enabled", "invalid", "orphan"] {
        let (_temp, root, file) = workspace();
        let server = Server::normal();
        match mode {
            "enabled" => {
                let output = command()
                    .args(["action", "enable-runtime-launch"])
                    .arg(&root)
                    .arg("--json")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            "invalid" => {
                std::fs::write(root.join("state/runtime-launch-control.json"), "{}\n").unwrap();
            }
            _ => {
                std::fs::create_dir_all(root.join("state/actions")).unwrap();
                std::fs::write(root.join("state/actions/orphan.json"), "{}\n").unwrap();
            }
        }
        let output = prompt(&root, &server.path, &file, true).output().unwrap();
        assert!(
            !output.status.success(),
            "authority {mode} admitted: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(server.methods().is_empty());
    }
}

#[test]
fn maximum_prompt_is_accepted_without_disclosing_text() {
    let (_temp, root, file) = workspace();
    std::fs::write(&file, vec![b'x'; 65536]).unwrap();
    let server = Server::normal();
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        0,
    );
    assert_eq!(value["prompt_bytes"], 65536);
    assert_eq!(server.methods(), ["capabilities", "submit"]);
    assert_eq!(
        server.requests.lock().unwrap()[1]["params"]["text"]
            .as_str()
            .unwrap()
            .len(),
        65536
    );
    assert!(!value.to_string().contains(&"x".repeat(100)));
}

#[test]
fn all_original_malformed_ack_cases_are_uncertain_without_retry() {
    let (_temp, root, file) = workspace();
    for case in 0..8 {
        let server = Server::new(move |request, response| {
            if request["method"] == "submit" {
                match case {
                    0 => response["id"] = json!(RUNTIME),
                    1 => response["version"] = json!(true),
                    2 => response["runtime_id"] = json!(REQUEST),
                    3 => response["result"]["session_id"] = json!("ses_other"),
                    4 => response["result"]["prompt_sha256"] = json!("0".repeat(64)),
                    5 => response["result"]["prompt_bytes"] = json!(true),
                    6 => response["result"]["message_id"] = json!("w1:p1"),
                    _ => response["error"] = json!({"code":[],"delivery":[]}),
                }
            }
            Some(frame(response))
        });
        let value = report(
            prompt(&root, &server.path, &file, true).output().unwrap(),
            2,
        );
        assert_eq!(value["delivery"], "uncertain", "case {case}");
        assert_eq!(value["native_prompt_acceptance"], "unestablished");
        assert!(value["native_message_id"].is_null());
        assert_eq!(server.methods(), ["capabilities", "submit"]);
    }
}

#[test]
fn leaf_socket_replacement_after_capabilities_contacts_neither_submit_endpoint() {
    use std::sync::{Arc, Mutex};
    let (_temp, root, file) = workspace();
    let replacement = Server::normal();
    let replacement_path = replacement.path.clone();
    let selected = Arc::new(Mutex::new(std::path::PathBuf::new()));
    let captured = selected.clone();
    let server = Server::new(move |request, response| {
        assert_eq!(request["method"], "capabilities");
        let socket = captured.lock().unwrap();
        std::fs::rename(&*socket, socket.with_extension("old")).unwrap();
        std::fs::rename(&replacement_path, &*socket).unwrap();
        Some(frame(response))
    });
    *selected.lock().unwrap() = server.path.clone();
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        1,
    );
    assert_eq!(value["delivery"], "unsent");
    assert_eq!(server.methods(), ["capabilities"]);
    assert!(replacement.methods().is_empty());
}

#[test]
fn original_invalid_argument_cases_refuse_without_endpoint_contact() {
    let (_temp, root, file) = workspace();
    let server = Server::normal();
    for (flag, value) in [
        ("--native-socket", "relative"),
        ("--expected-runtime", "w1:p1"),
        ("--request-id", "not-a-uuid"),
        ("--session", "folder"),
        ("--prompt-file", "/x/../y"),
        ("--timeout", "nan"),
    ] {
        let original = prompt(&root, &server.path, &file, true);
        let mut args: Vec<_> = original.get_args().map(|arg| arg.to_os_string()).collect();
        if let Some(index) = args.iter().position(|arg| arg == flag) {
            args[index + 1] = value.into();
        } else {
            args.extend([flag.into(), value.into()]);
        }
        let output = command().args(args).output().unwrap();
        assert!(!output.status.success(), "{flag}={value}");
        assert!(server.methods().is_empty());
    }
}

#[test]
fn literal_receipt_contract_keeps_prompt_and_peer_extensions_private() {
    let (_temp, root, file) = workspace();
    std::fs::write(&file, b"hello").unwrap();
    let server = Server::new(|request, response| {
        if request["method"] == "submit" {
            assert_eq!(request["id"], "832492cf-947d-4ba4-8fa6-9c43a347a81b");
            assert_eq!(
                request["params"],
                json!({
                    "expected_runtime_id":"6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0",
                    "session_id":"ses_fixture", "text":"hello"
                })
            );
            return Some(concat!(
                "{\"version\":1,\"id\":\"832492cf-947d-4ba4-8fa6-9c43a347a81b\",",
                "\"runtime_id\":\"6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0\",",
                "\"private\":\"private-peer-extension\",\"result\":{",
                "\"type\":\"prompt_accepted\",\"contract\":\"djinn.interactive-prompt.v1\",",
                "\"session_id\":\"ses_fixture\",\"message_id\":\"msg_literal\",",
                "\"prompt_sha256\":\"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824\",",
                "\"prompt_bytes\":5,\"private\":\"private-receipt-extension\"}}\n"
            ).as_bytes().to_vec());
        }
        Some(frame(response))
    });
    let value = report(
        prompt(&root, &server.path, &file, true).output().unwrap(),
        0,
    );
    assert_eq!(value["delivery"], "accepted");
    assert_eq!(value["native_prompt_acceptance"], "accepted");
    assert_eq!(value["native_message_id"], "msg_literal");
    assert_eq!(value["prompt_bytes"], 5);
    assert_eq!(
        value["prompt_sha256"],
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(value["native_worker_acceptance"], "unestablished");
    assert_eq!(value["job_completion"], "unestablished");
    for private in [
        "hello",
        "private-peer-extension",
        "private-receipt-extension",
    ] {
        assert!(!value.to_string().contains(private));
    }
    assert_eq!(server.methods(), ["capabilities", "submit"]);
}
