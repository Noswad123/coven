use super::*;
use serde_json::Value;

fn fact(args: &[&str]) -> ProcessFact {
    ProcessFact {
        pid: 42,
        name: "unrelated".into(),
        argv0: None,
        argv: Some(args.iter().map(|s| (*s).into()).collect()),
    }
}

fn provider(args: &[&str]) -> Option<&'static str> {
    classify(&fact(args)).map(|m| m.provider)
}

#[test]
fn all_families_and_exact_aliases() {
    let groups = [
        ("pi", "pi"),
        ("claude", "claude|claude-code"),
        ("codex", "codex"),
        ("gemini", "gemini"),
        ("cursor", "cursor|cursor-agent"),
        ("devin", "devin|devin-cli|devin cli"),
        ("agy", "agy|antigravity|antigravity-cli"),
        ("cline", "cline"),
        ("omp", "omp"),
        ("mastracode", "mastracode|mastra-code|mastra code"),
        ("opencode", "opencode|open-code"),
        ("copilot", "copilot|github-copilot|ghcs"),
        ("kimi", "kimi|kimi-code|kimi code"),
        ("kiro", "kiro|kiro-cli"),
        ("droid", "droid"),
        ("amp", "amp|amp-local"),
        ("grok", "grok|grok-build"),
        ("hermes", "hermes|hermes-agent"),
        ("kilo", "kilo|kilo-code|kilo code"),
        ("qodercli", "qodercli|qoderclicn|qoder|qodercn"),
        ("maki", "maki"),
        ("djinn", "djinn|djinn-ui"),
    ];
    assert_eq!(groups.len(), 22);
    for (canonical, aliases) in groups {
        for alias in aliases.split('|') {
            for literal in [
                alias.to_owned(),
                format!("/nonexistent/{}.JS", alias.to_ascii_uppercase()),
            ] {
                assert_eq!(provider(&[&literal]), Some(canonical), "{literal}");
                assert_eq!(provider(&["node", &literal]), Some(canonical));
            }
            assert_eq!(provider(&[&format!("not-{alias}")]), None);
        }
    }
}

#[test]
fn authoritative_fields_and_no_text_parsing() {
    let mut f = fact(&["codex"]);
    f.name = "claude".into();
    f.argv0 = Some("djinn".into());
    assert_eq!(classify(&f).unwrap().provider, "codex");
    for args in [
        vec!["unknown".into()],
        vec![String::new()],
        vec!["node".into(), "unknown".into(), "pi".into()],
    ] {
        f.argv = Some(args);
        assert_eq!(classify(&f), None);
    }
    for args in [None, Some(vec![])] {
        f.argv = args;
        assert_eq!(classify(&f).unwrap().provider, "djinn");
        f.argv0 = Some("unknown".into());
        assert_eq!(classify(&f), None);
        f.argv0 = None;
        assert_eq!(classify(&f).unwrap().provider, "claude");
        f.argv0 = Some("djinn".into());
    }
    for raw in [
        "'djinn'",
        "\"djinn\"",
        " djinn",
        "djinn ",
        "djinn --help",
        "DJİNN",
        "djinn\u{a0}",
        "C:\\bin\\djinn",
        "djinn.exe",
        "djinn.cmd",
        "djinn.bat",
        "djinn.ps1",
        "djinn.js.js",
        "djinn/",
    ] {
        assert_eq!(provider(&[raw]), None, "{raw}");
    }
}

#[test]
fn literal_wrappers_flags_and_first_operand_only() {
    for args in [
        vec![
            "node",
            "-r",
            "preload.js",
            "--loader",
            "loader.js",
            "--import",
            "import.js",
            "--require",
            "other.js",
            "--no-warnings",
            "codex.js",
        ],
        vec!["nodejs", "--", "codex"],
        vec!["bun", "--smol", "--preload", "preload", "codex.js"],
        vec!["python", "-u", "-W", "ignore", "-X", "dev", "codex"],
        vec!["python3", "-B", "codex"],
        vec!["sh", "-e", "codex"],
        vec!["bash", "-l", "codex"],
        vec!["zsh", "-f", "codex"],
        vec!["fish", "--no-config", "codex"],
    ] {
        assert_eq!(provider(&args), Some("codex"), "{args:?}");
    }
    for runtime in ["node", "bun", "python", "sh", "bash", "zsh", "fish"] {
        for flag in [
            "-e",
            "--eval",
            "-p",
            "--print",
            "-c",
            "-m",
            "--unknown",
            "--eval=codex",
            "-lc",
        ] {
            // -e is a safe shell option, but its first script is still not an agent.
            assert_eq!(provider(&[runtime, flag, "not-a-script", "codex"]), None);
        }
        assert_eq!(provider(&[runtime, "other.js", "codex"]), None);
        assert_eq!(provider(&[runtime, "'codex'"]), None);
    }
    for args in [
        vec!["node", "-r"],
        vec!["node", "-r", "--eval", "codex"],
        vec!["node", "-r", "", "codex"],
        vec!["bun", "run", "codex"],
        vec!["npm", "exec", "codex"],
        vec!["env", "codex"],
        vec!["python3.12", "codex"],
        vec!["node", "--require=preload", "codex"],
    ] {
        assert_eq!(provider(&args), None, "{args:?}");
    }
}

#[test]
fn lexical_paths_package_and_display_safe_modes() {
    let package = "node_modules/@earendil-works/pi-coding-agent/dist/cli.js";
    for script in [package.to_owned(), format!("/does/not/exist/{package}")] {
        let m = classify(&fact(&["node", &script])).unwrap();
        assert_eq!(m.rule, "literal-pi-package-script");
        assert_eq!(m.provider, "pi");
    }
    for script in [
        format!("x{package}"),
        format!("{package}.bak"),
        package.replace("dist/", "dist/../dist/"),
        package.replace("cli.js", "index.js"),
        package.to_ascii_uppercase(),
    ] {
        assert_eq!(provider(&["node", &script]), None, "{script}");
    }
    assert_eq!(provider(&[package]), None);
    assert_eq!(provider(&["/missing/relabelled-binary"]), None);
    assert_eq!(provider(&["/missing/codex"]), Some("codex"));
    for mode in [
        "--help",
        "--version",
        "serve",
        "config",
        "maintenance",
        "claude",
    ] {
        let m = classify(&fact(&["/missing/djinn", mode])).unwrap();
        assert_eq!(
            serde_json::to_value(m).unwrap(),
            serde_json::json!({"provider":"djinn","pid":42,"rule":"literal-executable"})
        );
        assert_eq!(provider(&["node", "djinn.js", mode]), Some("djinn"));
    }
}

#[test]
fn frozen_comparison_ledger_not_parity_or_job_selection() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/kitsune_process_policy/reference.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 209);
    let mut ledger = std::collections::BTreeMap::<&str, usize>::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let category = if name.starts_with("tree/") || name == "root-absent" {
            "no-group-leader-or-ranking"
        } else if matches!(
            name,
            "wrapper/controlled-symlink" | "wrapper/controlled-missing-path"
        ) {
            "no-filesystem-resolver"
        } else if name.starts_with("fallback/") {
            "structured-argv-authority-no-cmdline-or-trimming"
        } else if matches!(
            name,
            "suffix/.exe"
                | "suffix/.cmd"
                | "suffix/.bat"
                | "suffix/.ps1"
                | "wrapper/cmd-command"
                | "wrapper/cmd-keep"
                | "wrapper/powershell-file"
                | "wrapper/pwsh-command"
                | "wrapper/pwsh-positional"
                | "wrapper/powershell-encoded"
                | "wrapper/powershell-nonagent-command"
        ) {
            "no-windows-rules"
        } else if name == "wrapper/shell-login-combined" {
            "small-explicit-wrapper-flag-allowlist"
        } else {
            "comparable-literal-evidence"
        };
        *ledger.entry(category).or_default() += 1;
        let matches: Vec<_> = case["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| {
                classify(&ProcessFact {
                    pid: entry["pid"].as_u64().unwrap() as u32,
                    name: entry["name"].as_str().unwrap().into(),
                    argv0: entry["argv0"].as_str().map(str::to_owned),
                    argv: entry["argv"]
                        .as_array()
                        .map(|args| args.iter().map(|a| a.as_str().unwrap().into()).collect()),
                })
            })
            .collect();
        if category == "comparable-literal-evidence" {
            let expected = case["identify_agent_in_job"]["agent"].as_str();
            assert_eq!(matches.first().map(|m| m.provider), expected, "{name}");
            assert!(matches.len() <= 1, "{name}");
        }
        if category == "no-windows-rules" || category == "no-filesystem-resolver" {
            assert!(matches.is_empty(), "{name}");
        }
        if name.starts_with("tree/root-agent-descendant-other/") {
            assert_eq!(matches.len(), 2, "retain ambiguity: {name}");
        }
    }
    assert_eq!(ledger.values().sum::<usize>(), 209);
    assert_eq!(ledger.len(), 6);
    assert!(ledger.values().all(|count| *count > 0));
}
