use super::*;
use serde_json::{Value, json};

fn inventory() -> Value {
    json!({"version":"fixture","protocol":18,"command_submission":{"version":1},
        "panes":[{"pane_id":"w1:p1","terminal_id":"terminal-1","workspace_id":"workspace-1","tab_id":"tab-1"}]})
}
fn reply(snapshot: Value) -> Reply {
    Reply {
        payload: serde_json::to_vec(
            &json!({"id":"correlated","result":{"type":"session_snapshot","snapshot":snapshot}}),
        )
        .unwrap(),
        request_id: "correlated".into(),
        endpoint: Endpoint {
            selected: "/fixture/api.sock".into(),
            resolved: "/fixture/api.sock".into(),
            device: 1,
            inode: 2,
        },
    }
}
fn hint() -> Value {
    json!({"source":"kitsune:pi","agent":"pi","kind":"id","value":"saved"})
}

#[test]
fn absent_is_no_hint_but_present_must_validate_and_unknown_extensions_are_not_narrowed() {
    assert!(parse(&reply(inventory())).unwrap().is_empty());
    let mut inv = inventory();
    inv["panes"][0]["agent_session"] = hint();
    let mut response = reply(inv);
    let raw = std::str::from_utf8(&response.payload).unwrap();
    response.payload = format!("{{\"unknown\":{},{}", "9".repeat(80), &raw[1..]).into_bytes();
    assert_eq!(parse(&response).unwrap().len(), 1);
    for malformed in [
        Value::Null,
        json!({}),
        json!([]),
        json!({"source":"kitsune:pi","agent":"pi","kind":"path","value":"relative"}),
    ] {
        let mut inv = inventory();
        inv["panes"][0]["agent_session"] = malformed;
        assert!(parse(&reply(inv)).is_err());
    }
}

#[test]
fn response_reference_locator_and_pane_bounds_are_enforced() {
    let mut response = reply(inventory());
    response.payload = vec![b' '; MAX_BYTES + 1];
    assert_eq!(parse(&response).unwrap_err().code, "response_limit");
    let mut response = reply(inventory());
    response.payload = format!("{}0{}", "[".repeat(33), "]".repeat(33)).into_bytes();
    assert_eq!(parse(&response).unwrap_err().code, "nesting_limit");
    let mut response = reply(inventory());
    response.payload = br#"{"id":"correlated","id":"correlated"}"#.to_vec();
    assert!(parse(&response).is_err());
    for (field, limit) in [("source", 512), ("agent", 512), ("value", 512)] {
        let mut reference = hint();
        reference[field] = json!("x".repeat(limit));
        let mut inv = inventory();
        inv["panes"][0]["agent_session"] = reference.clone();
        assert_eq!(parse(&reply(inv)).unwrap().len(), 1);
        reference[field] = json!("x".repeat(limit + 1));
        let mut inv = inventory();
        inv["panes"][0]["agent_session"] = reference;
        assert!(parse(&reply(inv)).is_err());
    }
    for length in [4096, 4097] {
        let mut reference = hint();
        reference["kind"] = json!("path");
        reference["value"] = json!(format!("/{}", "x".repeat(length - 1)));
        let mut inv = inventory();
        inv["panes"][0]["agent_session"] = reference;
        assert_eq!(parse(&reply(inv)).is_ok(), length == 4096);
    }
    for length in [1024, 1025] {
        let mut inv = inventory();
        inv["panes"] = json!(
            (0..length)
                .map(
                    |i| json!({"pane_id":format!("pane-{i}"),"terminal_id":format!("terminal-{i}"),
            "workspace_id":"workspace-1","tab_id":"tab-1"})
                )
                .collect::<Vec<_>>()
        );
        assert_eq!(parse(&reply(inv)).is_ok(), length == 1024);
    }
    for field in ["pane_id", "terminal_id"] {
        let mut inv = inventory();
        let mut duplicate = inv["panes"][0].clone();
        duplicate["pane_id"] = json!("second");
        duplicate["terminal_id"] = json!("second-terminal");
        duplicate[field] = inv["panes"][0][field].clone();
        inv["panes"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&reply(inv)).is_err());
    }
    for (length, valid) in [(4096, true), (4097, false)] {
        let mut inv = inventory();
        inv["panes"][0]["pane_id"] = json!("x".repeat(length));
        assert_eq!(parse(&reply(inv)).is_ok(), valid);
    }
}

#[cfg(unix)]
#[allow(dead_code)]
use crate::interactive_session_fixture as fixture;

#[cfg(unix)]
#[test]
fn read_rejects_endpoint_replacement_and_multiple_frames() {
    use std::{
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::{Arc, Mutex},
    };
    let path = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let selected = path.clone();
    let server = fixture::Server::new(move |_, req| {
        assert_eq!(req["method"], "session.snapshot");
        let path = selected.lock().unwrap().clone().unwrap();
        std::fs::rename(&path, path.with_extension("old")).unwrap();
        let _replacement = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        fixture::snapshot(req, inventory())
    });
    *path.lock().unwrap() = Some(server.path.clone());
    assert_eq!(
        read(server.path.to_str().unwrap(), &wire::NoCancel)
            .unwrap_err()
            .code,
        "endpoint_replaced"
    );
    let server = fixture::Server::new(|_, req| {
        let mut frames = fixture::snapshot(req, inventory());
        frames.extend(fixture::snapshot(req, inventory()));
        frames
    });
    assert_eq!(
        read(server.path.to_str().unwrap(), &wire::NoCancel)
            .unwrap_err()
            .code,
        "trailing_data"
    );
    let server = fixture::Server::new(|_, _| vec![b' '; MAX_BYTES + 1]);
    assert_eq!(
        read(server.path.to_str().unwrap(), &wire::NoCancel)
            .unwrap_err()
            .code,
        "response_limit"
    );
}
