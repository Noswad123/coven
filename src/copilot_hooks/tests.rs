use super::{native, policy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

fn install(v: &Value, consent: bool) -> crate::Result<(Vec<u8>, bool)> {
    policy::install(
        Some(&serde_json::to_vec(v).unwrap()),
        Path::new("/root"),
        "/coven",
        "/socket",
        consent,
    )
}

#[test]
fn native_direct_schema_preserves_custom_types_and_opaque_data() {
    let v = json!({"disableAllHooks":true, "sandbox":{"enabled":true},
        "trust":{"command":"kitsune-agent-state"}, "hooks":{
        "SessionStart":[{"type":"prompt","prompt":"kitsune-agent-state coven-managed:"},
            {"type":"http","url":"https://example.test", "headers":{"command":"kitsune-agent-state"}},
            {"exec":"/custom", "args":["literal"], "env":{"command":"kitsune-agent-state"},
                "metadata":{"command":"coven-managed:copilot-hooks:v1"}}],
        "sessionStart":[{"bash":"custom camelCase handler", "timeout":2.5}]}});
    let (post, migrated) = install(&v, false).unwrap();
    assert!(!migrated);
    let post: Value = serde_json::from_slice(&post).unwrap();
    for key in ["disableAllHooks", "sandbox", "trust"] {
        assert_eq!(post[key], v[key]);
    }
    assert_eq!(post["hooks"]["sessionStart"], v["hooks"]["sessionStart"]);
    assert_eq!(
        &post["hooks"]["SessionStart"].as_array().unwrap()[..3],
        v["hooks"]["SessionStart"].as_array().unwrap()
    );
    assert_eq!(post["hooks"]["SessionStart"][3]["timeoutSec"], 10);
    assert!(post["hooks"]["SessionStart"][3].get("hooks").is_none());
}

#[test]
fn malformed_known_execution_fields_are_refused() {
    for h in [
        json!(null),
        json!({"hooks":[]}),
        json!({"bash":12}),
        json!({"bash":""}),
        json!({"bash":"ok","type":null}),
        json!({"type":"unknown","bash":"ok"}),
        json!({"bash":"ok","timeoutSec":"10"}),
        json!({"bash":"ok","timeout":0}),
        json!({"bash":"ok","cwd":false}),
        json!({"bash":"ok","env":{"A":false}}),
        json!({"bash":"ok","matcher":[]}),
        json!({"exec":"x","args":[1]}),
        json!({"exec":"x","bash":"y"}),
        json!({"bash":"x","args":[]}),
        json!({"type":"http","url":123}),
        json!({"type":"http","url":"file:/secret"}),
        json!({"type":"http","url":"http://localhost:8080","allowedEnvVars":["TOKEN"]}),
        json!({"type":"http","url":"http://localhost:8080","allowedEnvVars":[]}),
        json!({"type":"http","url":"https://x","headers":{"X":1}}),
        json!({"type":"prompt","prompt":false}),
    ] {
        let raw = serde_json::to_vec(&json!({"hooks":{"SessionStart":[h]}})).unwrap();
        assert!(native::parse(&raw).is_err(), "accepted {h}");
    }
    for raw in [
        br#"{"hooks":null}"#.as_slice(),
        br#"{"disableAllHooks":"false"}"#,
        br#"{"hooks":{"SessionStart":{}}}"#,
        br#"{"hooks":{},"hooks":{}}"#,
        b"{ /* native JSONC deliberately refused */ }",
    ] {
        assert!(native::parse(raw).is_err());
    }
}

#[test]
fn http_environment_expansion_requires_https_even_for_localhost() {
    let h = json!({"type":"http","url":"https://example.test","allowedEnvVars":["TOKEN"]});
    assert!(install(&json!({"hooks":{"SessionStart":[h]}}), false).is_ok());
}

#[test]
fn migration_requires_exact_v2_handler_and_consent() {
    let exact = json!({"type":"command","bash":"bash '/root/hooks/kitsune-agent-state.sh'", "timeoutSec":10});
    let value = |h: Value| json!({"hooks":{"SessionStart":[h]}});
    assert_eq!(
        install(&value(exact.clone()), false).unwrap_err().code(),
        "copilot_hooks_migration_consent_required"
    );
    assert!(install(&value(exact.clone()), true).unwrap().1);
    for (key, v) in [
        ("powershell", json!("custom platform hook")),
        ("command", json!("custom fallback")),
        ("env", json!({"IMPORTANT":"yes"})),
        ("matcher", json!("*")),
        ("timeoutSec", json!(20)),
    ] {
        let mut mixed = exact.clone();
        mixed[key] = v;
        assert_eq!(
            install(&value(mixed), true).unwrap_err().code(),
            "copilot_hooks_ambiguous_kitsune"
        );
    }
    for h in [
        json!({"bash":"echo kitsune-agent-state"}),
        json!({"command":"bash '/root/hooks/kitsune-agent-state.sh'"}),
        json!({"powershell":"& '/root/hooks/kitsune-agent-state.ps1'"}),
        json!({"exec":"bash", "args":["/root/hooks/kitsune-agent-state.sh"]}),
        json!({"bash":"bash '/different/hooks/kitsune-agent-state.sh'"}),
    ] {
        assert_eq!(
            install(&value(h), true).unwrap_err().code(),
            "copilot_hooks_ambiguous_kitsune"
        );
    }
    let wrong_event = json!({"hooks":{"sessionStart":[exact]}});
    assert_eq!(
        install(&wrong_event, true).unwrap_err().code(),
        "copilot_hooks_ambiguous_kitsune"
    );
}

#[test]
fn ownership_signals_are_not_ownership_authority() {
    for h in [
        json!({"bash":"echo coven-managed:copilot-hooks:v1"}),
        json!({"exec":"/echo", "args":["coven-managed:claude-hooks:v1"]}),
    ] {
        assert_eq!(
            install(&json!({"hooks":{"SessionStart":[h]}}), true)
                .unwrap_err()
                .code(),
            "copilot_hooks_unreceipted_ownership"
        );
    }
}

#[test]
fn pinned_asset_is_v2_and_license_is_shipped() {
    let asset = include_bytes!("../../tests/fixtures/copilot_hooks/kitsune-agent-state.sh");
    assert_eq!(format!("{:x}", Sha256::digest(asset)), policy::KIT_SHA);
    assert!(
        std::str::from_utf8(asset)
            .unwrap()
            .contains("KITSUNE_INTEGRATION_VERSION=2")
    );
    assert!(
        include_str!("../../tests/fixtures/copilot_hooks/KITSUNE_LICENSE")
            .contains("Version 2.0, January 2004")
    );
}
