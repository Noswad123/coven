//! Reference-only policy capture. Inputs go directly to the verified module.
use super::{AGENTS, AgentSessionRef, AgentSessionRefKind, reference};
use serde_json::{Value, json};

fn selector(value: Option<AgentSessionRef>) -> Value {
    value.map_or(
        Value::Null,
        |value| json!({"kind": value.kind, "value": value.value}),
    )
}

fn capture(cases: &mut Vec<Value>, name: String, operation: &str, input: Value) {
    let text = |key: &str| input[key].as_str().unwrap();
    let optional = |key: &str| input[key].as_str().map(str::to_owned);
    let kind = || serde_json::from_value(input["kind"].clone()).unwrap();
    let output = match operation {
        "report" => selector(reference::session_ref_from_report(
            text("source"),
            text("agent"),
            optional("id"),
            optional("path"),
        )),
        "snapshot" => reference::session_ref_from_snapshot(
            text("source"),
            text("agent"),
            kind(),
            text("value"),
        )
        .map_or(Value::Null, |value| {
            json!({
                "source": value.source, "agent": value.agent,
                "session_ref": selector(Some(value.session_ref))
            })
        }),
        "reserved" => json!(reference::is_reserved_native_state_source(
            text("source"),
            text("agent")
        )),
        "normalize" => json!(reference::normalize_session_start_source(optional("value"))),
        "dedupe" => json!(reference::dedupe_key(
            text("source"),
            text("agent"),
            &AgentSessionRef {
                kind: kind(),
                value: text("value").into(),
            }
        )),
        _ => unreachable!(),
    };
    cases.push(json!({"name": name, "operation": operation, "input": input, "output": output}));
}

pub(super) fn main() {
    assert_eq!(
        std::env::args_os().count(),
        3,
        "usage: kitsune-resume-reference --native-policy OUTPUT.json"
    );
    let mut cases = Vec::new();
    let sources: Vec<_> = AGENTS
        .iter()
        .map(|agent| format!("kitsune:{agent}"))
        .chain(["kitsune:unknown".into(), "custom:claude".into()])
        .collect();
    for source in &sources {
        for agent in AGENTS.into_iter().chain(["unknown", "custom-agent"]) {
            pair(
                &mut cases,
                &format!("matrix/{source}/{agent}"),
                source,
                agent,
            );
        }
    }
    for agent in AGENTS {
        let source = format!("kitsune:{agent}");
        for (label, source, agent) in [
            ("source-space", format!(" {source} "), agent.to_owned()),
            (
                "source-unicode-space",
                format!("\u{2003}{source}\u{2003}"),
                agent.to_owned(),
            ),
            ("source-case", source.to_uppercase(), agent.to_owned()),
            ("agent-space", source.clone(), format!(" {agent} ")),
            (
                "agent-unicode-space",
                source.clone(),
                format!("\u{2003}{agent}\u{2003}"),
            ),
            ("agent-case", source.clone(), agent.to_uppercase()),
        ] {
            pair(
                &mut cases,
                &format!("variant/{label}/{agent}"),
                &source,
                &agent,
            );
        }
        for (kind, label, limit) in [
            (AgentSessionRefKind::Id, "id", 512),
            (AgentSessionRefKind::Path, "path", 4096),
        ] {
            let prefix = if label == "path" { "/" } else { "" };
            let values = [
                ("empty", String::new()),
                ("blank", "   ".into()),
                ("unicode-blank", "\u{2003}".into()),
                ("leading-hyphen", "--help".into()),
                ("space-before-hyphen", "  --help".into()),
                ("unicode-space-before-hyphen", "\u{2003}--help".into()),
                ("newline", format!("{prefix}a\nb")),
                ("nul", format!("{prefix}a\0b")),
                ("tab", format!("{prefix}a\tb")),
                ("del", format!("{prefix}a\u{7f}b")),
                ("c1-control", format!("{prefix}a\u{85}b")),
                ("relative", "relative/session".into()),
                ("space-before-absolute", " /saved/session".into()),
                ("unicode", format!("{prefix}s\u{e9}ance-\u{1f98a}")),
                ("unicode-format", format!("{prefix}a\u{202e}b")),
                (
                    "literal-metacharacters",
                    format!("{prefix}a; $(NEVER) `cmd` 'x' \"x\" \\ * ? | & > <"),
                ),
                ("surrounding-spaces", format!("{prefix} saved session ")),
                ("path-components", "/a/../b//./session".into()),
                ("hyphen-component", "/--help".into()),
                (
                    "below-limit",
                    format!("{prefix}{}", "a".repeat(limit - prefix.len() - 1)),
                ),
                (
                    "at-limit",
                    format!("{prefix}{}", "a".repeat(limit - prefix.len())),
                ),
                (
                    "over-limit",
                    format!("{prefix}{}", "a".repeat(limit - prefix.len() + 1)),
                ),
                (
                    "unicode-at-limit",
                    if label == "id" {
                        "\u{e9}".repeat(256)
                    } else {
                        format!("/{}a", "\u{e9}".repeat(2047))
                    },
                ),
                (
                    "unicode-over-limit",
                    if label == "id" {
                        format!("{}a", "\u{e9}".repeat(256))
                    } else {
                        format!("/{}aa", "\u{e9}".repeat(2047))
                    },
                ),
            ];
            for (edge, value) in values {
                let name = format!("edge/{agent}/{label}/{edge}");
                capture(
                    &mut cases,
                    format!("{name}/snapshot"),
                    "snapshot",
                    json!({"source": source, "agent": agent, "kind": kind, "value": value}),
                );
                if label == "id" {
                    capture(
                        &mut cases,
                        format!("{name}/report-with-path"),
                        "report",
                        json!({"source": source, "agent": agent, "id": value, "path": "/saved/session"}),
                    );
                    capture(
                        &mut cases,
                        format!("{name}/report"),
                        "report",
                        json!({"source": source, "agent": agent, "id": value, "path": null}),
                    );
                } else {
                    for (label, id) in [("without-id", None), ("with-id", Some("saved-id"))] {
                        capture(
                            &mut cases,
                            format!("{name}/report-{label}"),
                            "report",
                            json!({"source": source, "agent": agent, "id": id, "path": value}),
                        );
                    }
                }
            }
        }
    }
    for value in ["startup", "resume", "clear", "compact", "new", "fork"] {
        for (label, value) in [
            ("exact", value.to_owned()),
            ("ascii-trim", format!(" \t{value}\n")),
            ("unicode-trim", format!("\u{2003}{value}\u{a0}")),
            ("case", value.to_uppercase()),
        ] {
            capture(
                &mut cases,
                format!("normalize/{label}/{value}"),
                "normalize",
                json!({"value": value}),
            );
        }
    }
    for (label, value) in [
        ("missing", None),
        ("empty", Some("")),
        ("blank", Some(" \u{2003}")),
        ("unknown", Some("other")),
        ("internal-space", Some("start up")),
        ("nul", Some("resume\0")),
    ] {
        capture(
            &mut cases,
            format!("normalize/{label}"),
            "normalize",
            json!({"value": value}),
        );
    }
    // Deliberately raw: dedupe is not validation or authorization.
    for (label, source, agent, kind, value) in [
        ("base", "kitsune:pi", "pi", "id", "/a/../b//session"),
        ("source", "custom:pi", "pi", "id", "/a/../b//session"),
        ("agent", "kitsune:pi", "unknown", "id", "/a/../b//session"),
        ("kind", "kitsune:pi", "pi", "path", "/a/../b//session"),
        ("value", "kitsune:pi", "pi", "id", "/b/session"),
        ("path-literal", "kitsune:pi", "pi", "path", "/b/session"),
        ("blank", "custom:pi", "unknown", "id", " "),
        ("control", "custom:pi", "unknown", "id", "a\0b"),
    ] {
        capture(
            &mut cases,
            format!("dedupe/{label}"),
            "dedupe",
            json!({"source": source, "agent": agent, "kind": kind, "value": value}),
        );
    }
    let fixture = json!({
        "reference": {
            "repository": "kitsune", "commit": "01d7cef6e98e2ccab216cd669d569c0731abfbce",
            "source": "src/agent_resume.rs", "blob": "0c3b51bd19d73aa0ede31d6775cac35395447273",
            "capture": "unchanged module copied to OUT_DIR after git blob byte verification",
            "harness": "tools/harnesses/kitsune-resume-reference", "mode": "--native-policy",
            "dependencies": {"serde": "1.0.228", "serde_json": "1.0.150", "schemars": "1.2.1"},
            "platform": "unix", "schema_version": 1
        }, "cases": cases
    });
    std::fs::write(
        std::env::args_os().nth(2).unwrap(),
        format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap()),
    )
    .unwrap();
}

fn pair(cases: &mut Vec<Value>, name: &str, source: &str, agent: &str) {
    for (label, id, path) in [
        ("both", Some("saved-id"), Some("/saved/session")),
        ("id-only", Some("saved-id"), None),
        ("path-only", None, Some("/saved/session")),
        ("missing", None, None),
    ] {
        capture(
            cases,
            format!("{name}/report/{label}"),
            "report",
            json!({"source": source, "agent": agent, "id": id, "path": path}),
        );
    }
    for (kind, value) in [("id", "saved-id"), ("path", "/saved/session")] {
        capture(
            cases,
            format!("{name}/snapshot/{kind}"),
            "snapshot",
            json!({"source": source, "agent": agent, "kind": kind, "value": value}),
        );
    }
    capture(
        cases,
        format!("{name}/reserved"),
        "reserved",
        json!({"source": source, "agent": agent}),
    );
}
