use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    sync::{Arc, Mutex},
    thread,
};

#[test]
fn navigation_result_is_closed_and_nullable_fields_must_be_present() {
    let v = json!({"contract":"coven.agent-navigation.v1","server_epoch":"a".repeat(32),
        "observation_id":"b".repeat(64),"association_revision":null,"provenance":"user_asserted",
        "native_hosting":"unestablished","valid":false,"applied":false,"target":null});
    validate_control_result(&v).unwrap();
    for key in v.as_object().unwrap().keys() {
        let mut bad = v.clone();
        bad.as_object_mut().unwrap().remove(key);
        bad["provider_payload"] = json!("PRIVATE");
        assert!(validate_control_result(&bad).is_err(), "{key}");
    }
}

#[test]
fn inspect_checks_capability_exact_id_and_live_generation_without_fallback() {
    for (version, availability, exact) in [
        (0, "live", true),
        (1, "exited", true),
        (1, "detached", true),
        (1, "live", false),
        (1, "live", true),
    ] {
        let root = tempfile::Builder::new().prefix("cn").tempdir().unwrap();
        let path = root.path().join("k");
        let listener = UnixListener::bind(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let methods = Arc::new(Mutex::new(Vec::new()));
        let seen = methods.clone();
        let worker = thread::spawn(move || {
            for _ in 0..if version == 1 { 2 } else { 1 } {
                let (mut socket, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(socket.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                seen.lock()
                    .unwrap()
                    .push(request["method"].as_str().unwrap().to_owned());
                let result = if request["method"] == "session.snapshot" {
                    json!({"snapshot":{"terminal_navigation":{"version":version}}})
                } else {
                    assert_eq!(request["params"], json!({"terminal_id":"term"}));
                    json!({"type":"terminal_inspected","terminal_id":if exact {"term"} else {"other"},
                        "runtime_generation":"generation","availability":availability,"workspace_id":"w","tab_id":"t","pane_id":"p"})
                };
                writeln!(socket, "{}", json!({"id":request["id"],"result":result})).unwrap();
            }
        });
        let endpoint = crate::observation_socket::select(path.to_str().unwrap()).unwrap();
        assert_eq!(
            unix::inspect(&endpoint, "term").is_ok(),
            version == 1 && availability == "live" && exact
        );
        worker.join().unwrap();
        assert!(
            methods
                .lock()
                .unwrap()
                .iter()
                .all(|m| matches!(m.as_str(), "session.snapshot" | "terminal.inspect"))
        );
    }
}
