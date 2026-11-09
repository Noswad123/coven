use coven::session_resume_plans::{Request, SessionKind, arguments};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    source: String,
    agent: String,
    kind: String,
    value: String,
    constructor_valid: bool,
    raw_argv: Option<Vec<String>>,
    validated_argv: Option<Vec<String>>,
    disposition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    reference: Value,
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/kitsune_resume/reference.json")).unwrap()
}

#[test]
fn frozen_reference_provenance_coverage_and_constructor_bypass() {
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("fixtures/kitsune_resume/reference.json"))
        ),
        "ddfcbcef2aad3e6e7f46634d7c50afa64ae4f8fe351837bd4f6cdc1aba4ef9fe",
        "frozen corpus changed; recapture independently and review policy dispositions"
    );
    let fixture = fixture();
    let metadata = fixture.reference;
    assert_eq!(
        metadata["commit"],
        "01d7cef6e98e2ccab216cd669d569c0731abfbce"
    );
    assert_eq!(metadata["blob"], "0c3b51bd19d73aa0ede31d6775cac35395447273");
    assert_eq!(metadata["source"], "src/agent_resume.rs");
    assert_eq!(metadata["repository"], "kitsune");
    assert_eq!(metadata["schema_version"], 1);
    assert_eq!(metadata["platform"], "unix");
    assert_eq!(
        metadata["harness"],
        "tools/harnesses/kitsune-resume-reference"
    );
    assert_eq!(
        metadata["capture"],
        "unchanged module copied to OUT_DIR after git blob byte verification"
    );
    assert_eq!(
        metadata["dependencies"],
        serde_json::json!({"serde": "1.0.228", "serde_json": "1.0.150", "schemars": "1.2.1"})
    );
    let names: HashSet<_> = fixture
        .cases
        .iter()
        .map(|case| case.name.as_str())
        .collect();
    assert_eq!(names.len(), fixture.cases.len(), "duplicate case names");
    assert_eq!(fixture.cases.len(), 1512);
    let sources: Vec<_> = AGENTS
        .iter()
        .map(|agent| format!("kitsune:{agent}"))
        .chain(["kitsune:unknown".into(), "custom:claude".into()])
        .collect();
    for source in &sources {
        for agent in AGENTS.into_iter().chain(["unknown", "custom-agent"]) {
            for kind in ["id", "path"] {
                let name = format!("matrix/{source}/{agent}/{kind}");
                let case = fixture
                    .cases
                    .iter()
                    .find(|case| case.name == name)
                    .expect("matrix cell");
                assert_eq!(
                    (&case.source, case.agent.as_str(), case.kind.as_str()),
                    (source, agent, kind)
                );
                assert!(case.constructor_valid);
                assert_eq!(case.disposition, "parity");
            }
        }
    }
    for agent in AGENTS
        .into_iter()
        .chain(["kitsune:unknown", "custom:claude"])
    {
        for kind in ["id", "path"] {
            for edge in [
                "empty",
                "blank",
                "unicode-blank",
                "leading-hyphen",
                "space-before-hyphen",
                "unicode-space-before-hyphen",
                "newline",
                "nul",
                "tab",
                "del",
                "c1-control",
                "relative",
                "space-before-absolute",
                "unicode",
                "unicode-format",
                "literal-metacharacters",
                "surrounding-spaces",
                "path-components",
                "hyphen-component",
                "below-limit",
                "at-limit",
                "over-limit",
                "unicode-at-limit",
                "unicode-over-limit",
            ] {
                assert!(
                    names.contains(format!("edge/{agent}/{kind}/{edge}").as_str()),
                    "missing edge {agent}/{kind}/{edge}"
                );
            }
        }
    }
    for case in &fixture.cases {
        if case.constructor_valid {
            assert_eq!(case.raw_argv, case.validated_argv, "{}", case.name);
        } else {
            assert!(case.validated_argv.is_none(), "{}", case.name);
            assert_eq!(case.disposition, "reject_invalid_session_selector");
        }
        let hardening = !case.constructor_valid
            || case.value.trim().is_empty()
            || case.value.trim_start().starts_with('-');
        assert_eq!(
            case.disposition,
            if hardening {
                "reject_invalid_session_selector"
            } else {
                "parity"
            },
            "{}",
            case.name
        );
    }
    for (name, constructor_valid) in [
        ("edge/claude/id/empty", false),
        ("edge/claude/id/newline", false),
        ("edge/claude/id/over-limit", false),
        ("edge/pi/path/relative", false),
        ("edge/omp/path/over-limit", false),
        ("edge/claude/id/blank", true),
        ("edge/claude/id/space-before-hyphen", true),
    ] {
        let case = fixture.cases.iter().find(|case| case.name == name).unwrap();
        assert_eq!(case.constructor_valid, constructor_valid, "{name}");
        assert!(
            case.raw_argv.is_some(),
            "raw planner bypass not captured: {name}"
        );
        assert_eq!(case.disposition, "reject_invalid_session_selector");
    }
    for (kind, limit) in [("id", 512), ("path", 4096)] {
        for edge in ["at-limit", "unicode-at-limit"] {
            let name = format!("edge/pi/{kind}/{edge}");
            let case = fixture.cases.iter().find(|case| case.name == name).unwrap();
            assert_eq!(case.value.len(), limit);
            assert!(case.constructor_valid);
        }
        for edge in ["over-limit", "unicode-over-limit"] {
            let name = format!("edge/pi/{kind}/{edge}");
            let case = fixture.cases.iter().find(|case| case.name == name).unwrap();
            assert_eq!(case.value.len(), limit + 1);
            assert!(!case.constructor_valid);
        }
    }
}

#[test]
fn coven_matches_frozen_reference_except_explicit_selector_hardening() {
    for case in fixture().cases {
        let request = Request {
            runtime: case.agent,
            source: case.source,
            session_kind: match case.kind.as_str() {
                "id" => SessionKind::Id,
                "path" => SessionKind::Path,
                _ => panic!("invalid fixture kind"),
            },
            session: case.value,
            executable: None,
        };
        if case.disposition == "reject_invalid_session_selector" {
            for json in [true, false] {
                let error = arguments(&request, json)
                    .err()
                    .unwrap_or_else(|| panic!("expected selector rejection: {}", case.name));
                assert_eq!(error.code(), "invalid_session_selector", "{}", case.name);
                assert_eq!(
                    error.to_string(),
                    "invalid_session_selector",
                    "{}: must not disclose raw selector",
                    case.name
                );
                assert_eq!(
                    format!("{error:?}"),
                    "Error(\"invalid_session_selector\")",
                    "{}",
                    case.name
                );
            }
            continue;
        }
        assert_eq!(case.disposition, "parity", "{}", case.name);
        let output =
            arguments(&request, true).unwrap_or_else(|error| panic!("{}: {error}", case.name));
        let report: Value = serde_json::from_str(&output.stdout).unwrap();
        assert!(output.warnings.is_empty(), "{}", case.name);
        match case.validated_argv {
            Some(argv) => {
                assert_eq!(output.exit_code, 0, "{}", case.name);
                assert_eq!(report["outcome"], "planned", "{}", case.name);
                assert_eq!(report["argv"], serde_json::json!(argv), "{}", case.name);
            }
            None => {
                assert_eq!(output.exit_code, 2, "{}", case.name);
                assert_eq!(report["outcome"], "unsupported", "{}", case.name);
                assert!(report.get("argv").is_none(), "{}", case.name);
            }
        }
    }
}
