use super::*;
use crate::values::Value;

#[cfg(unix)]
#[path = "retirement.rs"]
mod retirement;

const RUNTIME: &str = "6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0";
const ID: &str = "832492cf-947d-4ba4-8fa6-9c43a347a81b";

fn request() -> Request {
    Request {
        workspace: PathBuf::from("/missing/workspace"),
        native_socket: "/missing/socket".into(),
        expected_runtime: RUNTIME.into(),
        session: "ses_fixture".into(),
        request_id: ID.into(),
        prompt_file: "/missing/prompt".into(),
        execute: false,
        timeout: 3.0,
    }
}
fn envelope(result: JsonValue) -> JsonValue {
    json!({"version":1,"id":ID,"runtime_id":RUNTIME,"result":result})
}
fn capability() -> JsonValue {
    envelope(
        json!({"type":"capabilities","contract":NATIVE_CONTRACT,"version":1,
        "runtime_id":RUNTIME,"max_prompt_bytes":LIMIT}),
    )
}
fn decode(value: &JsonValue) -> WireResult<Value> {
    protocol::decode(&serde_json::to_vec(value).unwrap(), ID)
}

#[test]
fn pure_preview_and_literal_validation() {
    let mut r = request();
    let report = preview(&r).unwrap();
    assert_eq!(report["delivery"], "not_attempted");
    assert!(report["prompt_sha256"].is_null());
    assert_eq!(prompt_arguments(&r, true).unwrap().exit_code, 0);
    for id in ["00000000-0000-0000-0000-000000000000", RUNTIME] {
        assert!(protocol::uuid(id));
    }
    for id in [
        "6CD93c2e-7146-4dca-8d2f-9c7afcfe85f0",
        "{6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0}",
        "no",
    ] {
        assert!(!protocol::uuid(id));
    }
    for session in ["ses_", "ses_x/y", "ses_\n", "folder", "ses_é"] {
        assert!(!protocol::identifier(session, "ses_"));
    }
    for value in ["relative", "/x/../y", "/x/", "/x\u{85}", "/x\0"] {
        r.prompt_file = value.into();
        assert!(preview(&r).is_err());
    }
    for timeout in [0.0, -1.0, 30.01, f64::NAN, f64::INFINITY] {
        assert!(arguments("/socket", timeout).is_err());
    }
}

#[test]
fn capability_strict_types_correlation_and_open_extensions() {
    assert_eq!(
        protocol::capability(&decode(&capability()).unwrap()).unwrap(),
        RUNTIME
    );
    for (key, value) in [
        ("version", json!(true)),
        ("version", json!(1.0)),
        ("max_prompt_bytes", json!(true)),
        ("contract", json!("other")),
        ("runtime_id", json!(ID)),
    ] {
        let mut v = capability();
        v["result"][key] = value;
        assert!(protocol::capability(&decode(&v).unwrap()).is_err());
    }
    for (key, value) in [
        ("id", json!(RUNTIME)),
        ("version", json!(true)),
        ("runtime_id", json!("bad")),
    ] {
        let mut v = capability();
        v[key] = value;
        assert!(decode(&v).is_err());
    }
    let raw = serde_json::to_string(&capability()).unwrap();
    let extended = format!("{{\"ignored\":{},{}", "9".repeat(4000), &raw[1..]);
    assert!(protocol::capability(&protocol::decode(extended.as_bytes(), ID).unwrap()).is_ok());
    for extension in [
        "NaN".to_owned(),
        "1e999".to_owned(),
        "9".repeat(4301),
        "{\"x\":1,\"x\":2}".into(),
        "\"\\ud800\"".into(),
    ] {
        let extended = format!("{{\"ignored\":{extension},{}", &raw[1..]);
        assert!(protocol::decode(extended.as_bytes(), ID).is_err());
    }
}

fn accepted() -> JsonValue {
    envelope(
        json!({"type":"prompt_accepted","contract":NATIVE_CONTRACT,"session_id":"ses_fixture",
        "message_id":"msg_fixture","prompt_sha256":"digest","prompt_bytes":12}),
    )
}
#[test]
fn receipt_and_safe_errors_never_echo_peer_text() {
    let r = request();
    assert_eq!(
        protocol::receipt(&decode(&accepted()).unwrap(), &r, "digest", 12).unwrap(),
        "msg_fixture"
    );
    for (key, value) in [
        ("type", json!("ok")),
        ("contract", json!("other")),
        ("session_id", json!("ses_other")),
        ("message_id", json!("pane")),
        ("prompt_sha256", json!("other")),
        ("prompt_bytes", json!(true)),
        ("prompt_bytes", json!(12.0)),
    ] {
        let mut v = accepted();
        v["result"][key] = value;
        assert_eq!(
            protocol::receipt(&decode(&v).unwrap(), &r, "digest", 12)
                .unwrap_err()
                .delivery,
            Delivery::Uncertain
        );
    }
    for code in [
        "runtime_mismatch",
        "request_capacity",
        "session_unavailable",
        "request_unavailable",
        "request_already_seen",
        "admission_uncertain",
        "secret peer message",
    ] {
        let expected = if matches!(code, "request_already_seen" | "admission_uncertain") {
            "uncertain"
        } else {
            "rejected"
        };
        let v = json!({"version":1,"id":ID,"runtime_id":RUNTIME,"error":{"code":code,"delivery":expected,"message":"private text"}});
        let error = protocol::receipt(&decode(&v).unwrap(), &r, "digest", 12).unwrap_err();
        assert!(!error.code.contains("private") && !error.code.contains("secret"));
        assert_eq!(
            error.delivery,
            if expected == "uncertain" || code == "secret peer message" {
                Delivery::Uncertain
            } else {
                Delivery::Rejected
            }
        );
    }
}

#[test]
fn interrupted_render_preserves_an_accounted_accepted_receipt() {
    let mut report = preview(&request()).unwrap();
    report["outcome"] = json!("accepted");
    report["delivery"] = json!("accepted");
    report["native_prompt_acceptance"] = json!("accepted");
    report["native_message_id"] = json!("msg_fixture");
    report["interrupted"] = json!(true);
    report["diagnostics"] = json!(["interrupted"]);
    let output = render(&report, true, false).unwrap();
    assert_eq!(output.exit_code, 130);
    assert!(!output.stdout.ends_with('\n'));
    let value: JsonValue = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(value["delivery"], "accepted");
    assert_eq!(value["native_message_id"], "msg_fixture");
    assert_eq!(value["native_prompt_acceptance"], "accepted");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    };

    #[test]
    fn endpoint_exact_modes_symlinks_and_parent_inode_fence() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("private");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = parent.join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let address = socket.to_str().unwrap();
        let selected = endpoint::Endpoint::select(address).unwrap();
        for mode in [0o400, 0o660, 0o1600] {
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(endpoint::Endpoint::select(address).is_err());
        }
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(endpoint::Endpoint::select(address).is_err());
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let alias = temp.path().join("alias");
        symlink(&parent, &alias).unwrap();
        for separators in ["/", "//", "///"] {
            let address = format!("{}{separators}socket", alias.display());
            assert_eq!(
                endpoint::Endpoint::select(&address).unwrap_err().code,
                "unsafe_native_parent"
            );
            let address = format!("{}{separators}socket", parent.display());
            assert!(endpoint::Endpoint::select(&address).is_ok());
        }
        let old = temp.path().join("old");
        std::fs::rename(&parent, &old).unwrap();
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Even preserving the socket inode cannot preserve its selected parent identity.
        std::fs::rename(old.join("socket"), &socket).unwrap();
        assert!(selected.fence().is_err());
    }

    #[test]
    fn bounded_regular_utf8_prompt_preserves_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("prompt");
        let address = file.to_str().unwrap();
        for raw in [
            b"".as_slice(),
            b" \t\n",
            b"\xff",
            b"secret\0text",
            b"secret\x7ftext",
            &vec![b'x'; LIMIT + 1],
        ] {
            std::fs::write(&file, raw).unwrap();
            assert!(endpoint::prompt_file(address).is_err());
        }
        for text in [
            "Literal λ 🦊\r\n$(not a command); @not-an-attachment\n".to_owned(),
            "x".repeat(LIMIT),
        ] {
            std::fs::write(&file, &text).unwrap();
            assert_eq!(endpoint::prompt_file(address).unwrap(), text);
        }
        let alias = temp.path().join("alias");
        symlink(&file, &alias).unwrap();
        assert!(endpoint::prompt_file(alias.to_str().unwrap()).is_err());
        assert!(endpoint::prompt_file(temp.path().to_str().unwrap()).is_err());
    }
}
