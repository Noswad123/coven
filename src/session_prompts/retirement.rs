//! Independent admission/transport checks; no reference implementation execution.
use super::*;

#[path = "../../tests/support/session_prompt_fixture.rs"]
mod fixture;

use crate::platform::observation_socket::SocketConstructionGuard;

struct AfterAckGuard(std::marker::PhantomData<std::rc::Rc<()>>);

impl AfterAckGuard {
    fn new(hook: impl FnMut(&JsonValue) + 'static) -> Self {
        AFTER_ACK.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "nested after-ack hook");
            *slot = Some(Box::new(hook));
        });
        Self(std::marker::PhantomData)
    }
}

impl Drop for AfterAckGuard {
    fn drop(&mut self) {
        AFTER_ACK.with(|slot| *slot.borrow_mut() = None);
    }
}

fn run_isolated(name: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            name,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

fn executable_request(root: &Path, file: &Path, server: &fixture::Server) -> Request {
    let mut request = request();
    request.workspace = root.to_path_buf();
    request.prompt_file = file.to_str().unwrap().into();
    request.native_socket = server.path.to_str().unwrap().into();
    request.execute = true;
    request
}

#[test]
fn authority_refusals_never_construct_a_native_socket() {
    run_isolated("session_prompts::tests::retirement::authority_socket_poison_child");
}

#[test]
#[ignore = "process-isolated helper invoked by authority_refusals_never_construct_a_native_socket"]
fn authority_socket_poison_child() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("coven.json"),
        br#"{"name":"Session","agents":["alpha"],"multiplexer":"herdr"}"#,
    )
    .unwrap();
    let file = root.join("prompt");
    std::fs::write(&file, "hello").unwrap();
    let server = fixture::Server::normal();
    let request = executable_request(root, &file, &server);
    // A valid workspace reaches and trips the actual constructor, not an API wrapper.
    {
        let poison = SocketConstructionGuard::new(true);
        assert!(std::panic::catch_unwind(|| prompt_arguments(&request, true)).is_err());
        assert_eq!(poison.count(), 1);
    }
    assert!(server.methods().is_empty());
    {
        let counter = SocketConstructionGuard::new(false);
        assert_eq!(prompt_arguments(&request, true).unwrap().exit_code, 0);
        assert_eq!(counter.count(), 2);
    }
    assert_eq!(server.methods(), ["capabilities", "submit"]);
    server.requests.lock().unwrap().clear();

    std::fs::create_dir_all(root.join("state/actions")).unwrap();
    std::fs::write(root.join("state/actions/orphan.json"), "{}\n").unwrap();
    {
        let poison = SocketConstructionGuard::new(true);
        assert_eq!(
            prompt_arguments(&request, true).err().unwrap().code(),
            "action_orphan_bundles"
        );
        assert_eq!(poison.count(), 0);
        assert!(server.methods().is_empty());
    }
    std::fs::remove_file(root.join("state/actions/orphan.json")).unwrap();
    std::fs::remove_dir(root.join("state/actions")).unwrap();
    let location = crate::platform::workspace_location(root).unwrap();
    std::fs::write(
        root.join("state/runtime-launch-control.json"),
        serde_json::to_vec(&json!({
            "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),
            "epoch_id":"b".repeat(32),
            "location":{"root":location.root,"device":location.device,"inode":location.inode},
            "head":null,"claims":{}
        }))
        .unwrap(),
    )
    .unwrap();
    let poison = SocketConstructionGuard::new(true);
    assert_eq!(
        prompt_arguments(&request, true).err().unwrap().code(),
        "legacy_runtime_disabled"
    );
    assert_eq!(poison.count(), 0);
    assert!(server.methods().is_empty());
}

#[test]
fn interruption_after_real_ack_accounting_preserves_accepted_fact() {
    run_isolated("session_prompts::tests::retirement::after_ack_sigint_child");
}

#[test]
#[ignore = "process-isolated helper invoked by interruption_after_real_ack_accounting_preserves_accepted_fact"]
fn after_ack_sigint_child() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(
        root.join("coven.json"),
        br#"{"name":"Session","agents":["alpha"],"multiplexer":"herdr"}"#,
    )
    .unwrap();
    let file = root.join("prompt");
    std::fs::write(&file, "hello").unwrap();
    let server = fixture::Server::normal();
    let request = executable_request(root, &file, &server);
    let fired = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = fired.clone();
    let _hook = AfterAckGuard::new(move |report| {
        assert_eq!(observed.replace(observed.get() + 1), 0);
        for field in ["outcome", "delivery", "native_prompt_acceptance"] {
            assert_eq!(report[field], "accepted");
        }
        assert_eq!(report["native_message_id"], "msg_fixture");
        assert_eq!(report["prompt_bytes"], 5);
        assert_eq!(
            report["prompt_sha256"],
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            crate::transport_gate::TransportGuard::exclusive(
                Path::new(report["workspace"].as_str().unwrap()),
                Duration::ZERO
            )
            .err()
            .unwrap()
            .code(),
            "runtime_transport_gate_busy"
        );
        assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
    });
    let counter = SocketConstructionGuard::new(false);
    let output = prompt_arguments(&request, true).unwrap();
    assert_eq!(output.exit_code, 130);
    let value: JsonValue = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(fired.get(), 1);
    assert_eq!(counter.count(), 2);
    assert_eq!(value["outcome"], "accepted");
    assert_eq!(value["delivery"], "accepted");
    assert_eq!(value["native_prompt_acceptance"], "accepted");
    assert_eq!(value["native_message_id"], "msg_fixture");
    assert_eq!(value["prompt_bytes"], 5);
    assert_eq!(
        value["prompt_sha256"],
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(value["interrupted"], true);
    assert_eq!(value["diagnostics"], json!(["interrupted"]));
    assert_eq!(value["native_worker_acceptance"], "unestablished");
    assert_eq!(value["job_completion"], "unestablished");
    assert_eq!(server.methods(), ["capabilities", "submit"]);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests[1]["id"], ID);
    assert_eq!(requests[1]["params"]["text"], "hello");
    assert!(!output.stdout.contains("hello"));
    drop(requests);
    // The gate is released only after the interrupted accepted report is rendered.
    assert!(crate::transport_gate::TransportGuard::exclusive(root, Duration::ZERO).is_ok());
}

#[test]
fn expired_clock_and_pre_send_cancel_leave_both_methods_unsent() {
    struct Cancelled;
    impl Cancel for Cancelled {
        fn interrupted(&self) -> bool {
            true
        }
    }
    let server = fixture::Server::normal();
    let endpoint = endpoint::Endpoint::select(server.path.to_str().unwrap()).unwrap();
    for method in [protocol::Method::Capabilities, protocol::Method::Submit] {
        for interrupted in [false, true] {
            let no_cancel = crate::observation_socket::NoCancel;
            let cancel: &dyn Cancel = if interrupted { &Cancelled } else { &no_cancel };
            let deadline = if interrupted {
                Instant::now() + Duration::from_secs(3)
            } else {
                Instant::now() - Duration::from_secs(1)
            };
            let error = exchange(
                &endpoint,
                method,
                &json!({}),
                ID,
                deadline,
                &mut ReadBudget::new(65536),
                cancel,
            )
            .unwrap_err();
            assert_eq!(error.delivery, Delivery::Unsent);
            assert_eq!(error.interrupted, interrupted);
            assert_eq!(
                error.code,
                if interrupted {
                    "interrupted"
                } else {
                    "timeout"
                }
            );
            assert!(server.methods().is_empty());
        }
    }
}

#[test]
fn wrong_workspace_gate_fails_admission_validation() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir(other.path().join("state")).unwrap();
    let gate = crate::transport_gate::TransportGuard::shared(other.path(), Duration::ZERO).unwrap();
    assert_eq!(
        crate::session_support::validate_admission(temp.path(), &gate)
            .unwrap_err()
            .code(),
        "invalid_transport_gate_owner"
    );
}

#[test]
fn replacement_after_completed_capability_exchange_is_unsent() {
    let server = fixture::Server::normal();
    let replacement = fixture::Server::normal();
    let endpoint = endpoint::Endpoint::select(server.path.to_str().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut budget = ReadBudget::new(65536);
    let cancel = crate::observation_socket::NoCancel;
    let capability = exchange(
        &endpoint,
        protocol::Method::Capabilities,
        &json!({}),
        fixture::REQUEST,
        deadline,
        &mut budget,
        &cancel,
    )
    .unwrap();
    assert_eq!(protocol::capability(&capability).unwrap(), fixture::RUNTIME);
    std::fs::rename(&server.path, server.path.with_extension("old")).unwrap();
    std::fs::rename(&replacement.path, &server.path).unwrap();
    let error = exchange(
        &endpoint,
        protocol::Method::Submit,
        &json!({"expected_runtime_id":RUNTIME,"session_id":"ses_fixture","text":"hello"}),
        ID,
        deadline,
        &mut budget,
        &cancel,
    )
    .unwrap_err();
    assert_eq!(error.delivery, Delivery::Unsent);
    assert_eq!(error.code, "native_endpoint_changed");
    assert_eq!(server.methods(), ["capabilities"]);
    assert!(replacement.methods().is_empty());
}
