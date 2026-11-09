//! Development oracle: argv is obtained only by executing the unchanged reference.
use serde::Serialize;
use std::{env, fs};

#[allow(dead_code)]
mod reference {
    include!(concat!(env!("OUT_DIR"), "/agent_resume.rs"));
}
use reference::{AgentSessionRef, AgentSessionRefKind};

const AGENTS: [&str; 16] = [
    "claude",
    "codex",
    "copilot",
    "devin",
    "droid",
    "djinn",
    "kimi",
    "mastracode",
    "pi",
    "omp",
    "hermes",
    "opencode",
    "qodercli",
    "kilo",
    "cursor",
    "grok",
];

#[derive(Serialize)]
struct Case {
    name: String,
    source: String,
    agent: String,
    kind: AgentSessionRefKind,
    value: String,
    constructor_valid: bool,
    raw_argv: Option<Vec<String>>,
    validated_argv: Option<Vec<String>>,
    disposition: &'static str,
}

fn capture(
    name: String,
    source: &str,
    agent: &str,
    kind: AgentSessionRefKind,
    value: &str,
) -> Case {
    let constructed = match kind {
        AgentSessionRefKind::Id => AgentSessionRef::id(value),
        AgentSessionRefKind::Path => AgentSessionRef::path(value),
    };
    let raw = AgentSessionRef {
        kind,
        value: value.into(),
    };
    let raw_argv = reference::plan(source, agent, &raw).map(|plan| plan.argv);
    let validated_argv = constructed
        .as_ref()
        .and_then(|selector| reference::plan(source, agent, selector))
        .map(|plan| plan.argv);
    // This is the intentional Coven policy delta, not a second argv mapping.
    let reject =
        constructed.is_none() || value.trim().is_empty() || value.trim_start().starts_with('-');
    Case {
        name,
        source: source.into(),
        agent: agent.into(),
        kind,
        value: value.into(),
        constructor_valid: constructed.is_some(),
        raw_argv,
        validated_argv,
        disposition: if reject {
            "reject_invalid_session_selector"
        } else {
            "parity"
        },
    }
}

fn main() {
    let output = env::args_os()
        .nth(1)
        .expect("usage: kitsune-resume-reference OUTPUT.json");
    assert_eq!(
        env::args_os().count(),
        2,
        "exactly one output path required"
    );
    let sources: Vec<String> = AGENTS
        .iter()
        .map(|agent| format!("kitsune:{agent}"))
        .chain(["kitsune:unknown".into(), "custom:claude".into()])
        .collect();
    let agents: Vec<&str> = AGENTS
        .into_iter()
        .chain(["unknown", "custom-agent"])
        .collect();
    let mut cases = Vec::new();
    for source in &sources {
        for agent in &agents {
            for (kind, label, value) in [
                (AgentSessionRefKind::Id, "id", "saved-session"),
                (AgentSessionRefKind::Path, "path", "/saved/session.jsonl"),
            ] {
                cases.push(capture(
                    format!("matrix/{source}/{agent}/{label}"),
                    source,
                    agent,
                    kind,
                    value,
                ));
            }
        }
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
            ("newline", format!("{prefix}secret\nvalue")),
            ("nul", format!("{prefix}secret\0value")),
            ("tab", format!("{prefix}secret\tvalue")),
            ("del", format!("{prefix}secret\u{7f}value")),
            ("c1-control", format!("{prefix}secret\u{85}value")),
            ("relative", "relative/session.jsonl".into()),
            ("space-before-absolute", " /saved/session".into()),
            ("unicode", format!("{prefix}s\u{e9}ance-\u{1f98a}")),
            ("unicode-format", format!("{prefix}saved\u{202e}session")),
            (
                "literal-metacharacters",
                format!("{prefix}saved; $(touch NEVER) `cmd` 'quoted' \"x\" \\ * ? | & > <"),
            ),
            ("surrounding-spaces", format!("{prefix} saved session ")),
            ("path-components", "/a/../b//session".into()),
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
            for agent in &AGENTS {
                let source = format!("kitsune:{agent}");
                cases.push(capture(
                    format!("edge/{agent}/{label}/{edge}"),
                    &source,
                    agent,
                    kind,
                    &value,
                ));
            }
            for (source, agent) in [("kitsune:unknown", "unknown"), ("custom:claude", "claude")] {
                cases.push(capture(
                    format!("edge/{source}/{label}/{edge}"),
                    source,
                    agent,
                    kind,
                    &value,
                ));
            }
        }
    }
    let fixture = serde_json::json!({
        "reference": {
            "repository": "kitsune",
            "commit": "01d7cef6e98e2ccab216cd669d569c0731abfbce",
            "source": "src/agent_resume.rs",
            "blob": "0c3b51bd19d73aa0ede31d6775cac35395447273",
            "capture": "unchanged module copied to OUT_DIR after git blob byte verification",
            "harness": "tools/harnesses/kitsune-resume-reference",
            "dependencies": {"serde": "1.0.228", "serde_json": "1.0.150", "schemars": "1.2.1"},
            "platform": "unix",
            "schema_version": 1
        },
        "cases": cases
    });
    fs::write(
        output,
        format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap()),
    )
    .unwrap();
}
