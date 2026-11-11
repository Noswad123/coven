use super::*;
use observation_socket::{Endpoint, Reply};

fn reply(result: Value) -> Reply {
    Reply {
        payload: serde_json::to_vec(&json!({"id":"test","result":result})).unwrap(),
        request_id: "test".into(),
        endpoint: Endpoint {
            selected: "/unused".into(),
            resolved: "/unused".into(),
            device: 1,
            inode: 2,
        },
    }
}

fn snapshot(panes: usize) -> Value {
    json!({"type":"session_snapshot","snapshot":{"version":"kit-test","protocol":18,
        "process_observation":{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"},
        "panes":(0..panes).map(|i| json!({"pane_id":format!("p{i}"),"terminal_id":format!("t{i}"),"workspace_id":"w","tab_id":"tab"})).collect::<Vec<_>>()}})
}

fn processes(rows: Value) -> Reply {
    reply(
        json!({"type":"pane_process_info","process_info":{"pane_id":"p0","foreground_processes":rows}}),
    )
}

#[test]
fn validation_is_bounded_and_expectation_requires_selection() {
    let mut request = Request {
        socket: "/tmp/test.sock".into(),
        pane: None,
        expected_terminal: None,
        timeout: 3.0,
        max_response_bytes: 1048576,
    };
    assert!(validate(&request).is_ok());
    for timeout in [0.0, -1.0, 30.1, f64::NAN, f64::INFINITY] {
        request.timeout = timeout;
        assert!(validate(&request).is_err());
    }
    request.timeout = 30.0;
    for budget in [0, 1023, 8388609] {
        request.max_response_bytes = budget;
        assert!(validate(&request).is_err());
    }
    request.max_response_bytes = 1024;
    request.expected_terminal = Some("t0".into());
    assert!(validate(&request).is_err());
    request.pane = Some("p0".into());
    assert!(validate(&request).is_ok());
}

#[test]
fn snapshot_is_neutral_and_does_not_narrow_extensions() {
    let mut value = snapshot(1);
    value["snapshot"]["agents"] = json!([false,{"agent_session":true}]);
    value["snapshot"]["panes"][0]["agent"] = json!({"malformed":"ignored"});
    let mut wire = reply(value);
    let text = String::from_utf8(wire.payload).unwrap().replace(
        "\"protocol\":18",
        "\"ignored\":99999999999999999999999999999999999999,\"protocol\":18",
    );
    wire.payload = text.into_bytes();
    let decoded = schema::snapshot(&wire).unwrap();
    assert_eq!(decoded.protocol, 18);
    assert_eq!(
        decoded.selected(None).unwrap()[0]
            .as_object()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn descriptor_is_mandatory_and_linux_unclassified_only() {
    for (key, value) in [
        ("version", json!(2)),
        ("version", json!(true)),
        ("platform", json!("windows")),
        ("platform", json!("macos")),
        ("scope", json!("selected_agent")),
        ("selection", json!("ranked")),
    ] {
        let mut s = snapshot(0);
        s["snapshot"]["process_observation"][key] = value;
        assert_eq!(
            schema::snapshot(&reply(s)).err().unwrap().code,
            "process_observation_unsupported"
        );
    }
    let mut s = snapshot(0);
    s["snapshot"]
        .as_object_mut()
        .unwrap()
        .remove("process_observation");
    assert_eq!(
        schema::snapshot(&reply(s)).err().unwrap().code,
        "process_observation_unsupported"
    );
}

#[test]
fn cohort_limit_and_duplicate_locators() {
    assert_eq!(
        schema::snapshot(&reply(snapshot(64)))
            .unwrap()
            .selected(None)
            .unwrap()
            .len(),
        64
    );
    let s = schema::snapshot(&reply(snapshot(65))).unwrap();
    assert_eq!(s.selected(None).unwrap_err().code, "pane_limit");
    assert_eq!(s.selected(Some("p64")).unwrap().len(), 1);
    assert_eq!(
        schema::snapshot(&reply(snapshot(1025))).err().unwrap().code,
        "pane_limit"
    );
    for key in ["pane_id", "terminal_id"] {
        let mut s = snapshot(2);
        s["snapshot"]["panes"][1][key] = s["snapshot"]["panes"][0][key].clone();
        assert!(schema::snapshot(&reply(s)).is_err());
    }
}

#[test]
fn process_caps_types_and_text_are_enforced() {
    let row = json!({"pid":1,"name":"node","argv0":null,"argv":["node","/x/claude.js","legal secret"],"cmdline":null,"cwd":null,"tty":{"opaque":"ignored"}});
    assert_eq!(
        schema::processes(&processes(json!([row.clone()])), "p0")
            .unwrap()
            .len(),
        1
    );
    for (key, value) in [
        ("pid", json!(0)),
        ("pid", json!(4294967296u64)),
        ("pid", json!(1.0)),
        ("name", json!(null)),
        ("argv0", json!(false)),
        ("argv", json!(true)),
        ("argv", json!(["a\u{0}b"])),
        ("argv", json!(["x".repeat(4097)])),
        ("argv", json!(["é".repeat(2049)])),
        ("argv", json!(vec!["x"; 129])),
        ("argv", json!(vec!["x".repeat(4096); 16])),
        ("cmdline", json!(false)),
        ("cwd", json!(17)),
    ] {
        let mut bad = row.clone();
        bad[key] = value;
        assert!(
            schema::processes(&processes(json!([bad])), "p0").is_err(),
            "{key}"
        );
    }
    let rows: Vec<_> = (1..=256)
        .map(|pid| json!({"pid":pid,"name":"claude"}))
        .collect();
    assert_eq!(
        schema::processes(&processes(json!(rows)), "p0")
            .unwrap()
            .len(),
        256
    );
    let rows: Vec<_> = (1..=257)
        .map(|pid| json!({"pid":pid,"name":"claude"}))
        .collect();
    assert_eq!(
        schema::processes(&processes(json!(rows)), "p0")
            .unwrap_err()
            .code,
        "process_limit"
    );
    assert!(schema::processes(&processes(json!([row.clone(), row])), "p0").is_err());
}

#[test]
fn missing_process_list_is_empty_but_wrong_pane_is_rejected() {
    let r = reply(json!({"type":"pane_process_info","process_info":{"pane_id":"p0"}}));
    assert!(schema::processes(&r, "p0").unwrap().is_empty());
    assert!(schema::processes(&r, "p1").is_err());
}

#[test]
fn malformed_envelopes_and_known_fields_fail_closed() {
    for payload in [
        b"{\"id\":\"test\",\"id\":\"test\"}".as_slice(),
        b"{",
        b"{\"id\":NaN}",
        b"\xff",
    ] {
        let mut r = reply(snapshot(0));
        r.payload = payload.to_vec();
        assert!(schema::snapshot(&r).is_err());
    }
    let mut r = reply(snapshot(0));
    r.request_id = "wrong".into();
    assert!(schema::snapshot(&r).is_err());
    let mut s = snapshot(1);
    s["snapshot"]["panes"][0]["pane_id"] = json!(false);
    assert!(schema::snapshot(&reply(s)).is_err());
}

#[test]
fn caller_cancellation_does_not_install_a_guard_or_read() {
    struct Interrupted;
    impl Cancel for Interrupted {
        fn interrupted(&self) -> bool {
            true
        }
    }
    let request = Request {
        socket: "/tmp/not-opened.sock".into(),
        pane: None,
        expected_terminal: None,
        timeout: 3.0,
        max_response_bytes: 1024,
    };
    let report = inspect_cancel(&request, &Interrupted).unwrap();
    assert_eq!(report["diagnostics"], json!(["interrupted"]));
    assert_eq!(report["targets"], json!([]));
    assert!(report["target_count"].is_null());
}
