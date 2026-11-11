//! Coven-owned, pure Linux executable hints, not evidence of interactive activity.
//!
//! Alias facts were checked against Kitsune `src/detect/mod.rs:180-205`, commit
//! 01d7cef6e98e2ccab216cd669d569c0731abfbce. The frozen comparison and provenance
//! live in `tests/fixtures/kitsune_process_policy/reference.json` (see also
//! `tests/kitsune_process_policy_reference.rs`). No resolver or ranking code is
//! ported. This module does not inspect files, PATH, environment, or live processes.
//!
//! Nonempty structured argv is authoritative, including an empty first operand.
//! Otherwise argv0, if present, masks name. Only POSIX `/` separates basenames;
//! ASCII case folds for executable/script aliases, without trimming, quote peeling,
//! Unicode normalization, or Windows suffix stripping. Each alias also accepts
//! exactly one `.js` suffix. Pi's package suffix is case-sensitive and lexical.
//! The collector owns raw-schema limits (256 processes, 128 arguments, 4 KiB per
//! argument, 64 KiB total text per process) and retains all matching records.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessFact {
    pub pid: u32,
    pub name: String,
    pub argv0: Option<String>,
    pub argv: Option<Vec<String>>,
}

/// Display-safe values only: no paths or raw arguments escape the classifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Match {
    pub provider: &'static str,
    pub pid: u32,
    pub rule: &'static str,
}

fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

fn alias(value: &str) -> Option<&'static str> {
    let name = basename(value).to_ascii_lowercase();
    let name = name.strip_suffix(".js").unwrap_or(&name);
    Some(match name {
        "pi" => "pi",
        "claude" | "claude-code" => "claude",
        "codex" => "codex",
        "gemini" => "gemini",
        "cursor" | "cursor-agent" => "cursor",
        "devin" | "devin-cli" | "devin cli" => "devin",
        "agy" | "antigravity" | "antigravity-cli" => "agy",
        "cline" => "cline",
        "omp" => "omp",
        "mastracode" | "mastra-code" | "mastra code" => "mastracode",
        "opencode" | "open-code" => "opencode",
        "copilot" | "github-copilot" | "ghcs" => "copilot",
        "kimi" | "kimi-code" | "kimi code" => "kimi",
        "kiro" | "kiro-cli" => "kiro",
        "droid" => "droid",
        "amp" | "amp-local" => "amp",
        "grok" | "grok-build" => "grok",
        "hermes" | "hermes-agent" => "hermes",
        "kilo" | "kilo-code" | "kilo code" => "kilo",
        "qodercli" | "qoderclicn" | "qoder" | "qodercn" => "qodercli",
        "maki" => "maki",
        "djinn" | "djinn-ui" => "djinn",
        _ => return None,
    })
}

pub(crate) fn classify(fact: &ProcessFact) -> Option<Match> {
    let argv = fact.argv.as_deref().filter(|args| !args.is_empty());
    let executable = argv.map_or_else(
        || fact.argv0.as_deref().unwrap_or(&fact.name),
        |args| args[0].as_str(),
    );
    if let Some(provider) = alias(executable) {
        return Some(Match {
            provider,
            pid: fact.pid,
            rule: "literal-executable",
        });
    }
    let runtime = basename(executable).to_ascii_lowercase();
    let args = argv?;
    let mut index = 1;
    // Never scan past an unknown flag or a non-agent first script operand.
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            index += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        let (single, paired): (&[&str], &[&str]) = match runtime.as_str() {
            "node" | "nodejs" => (
                &["--no-warnings", "--trace-warnings", "--enable-source-maps"],
                &["-r", "--require", "--loader", "--import"],
            ),
            "bun" => (&["--smol"], &["-r", "--require", "--preload"]),
            "python" | "python3" => (&["-u", "-B", "-E", "-I", "-s", "-S"], &["-W", "-X"]),
            "sh" | "bash" | "zsh" => (&["-l", "-e", "-u", "-x", "-f"], &[]),
            "fish" => (&["--no-config", "-N"], &[]),
            _ => return None,
        };
        if single.contains(&arg.as_str()) {
            index += 1;
        } else if paired.contains(&arg.as_str()) {
            let value = args.get(index + 1)?;
            if value.is_empty() || value.starts_with('-') {
                return None;
            }
            index += 2;
        } else {
            return None;
        }
    }
    if !matches!(
        runtime.as_str(),
        "node" | "nodejs" | "bun" | "python" | "python3" | "sh" | "bash" | "zsh" | "fish"
    ) {
        return None;
    }
    let script = args.get(index)?;
    let (provider, rule) = if matches!(runtime.as_str(), "node" | "nodejs" | "bun")
        && (script == "node_modules/@earendil-works/pi-coding-agent/dist/cli.js"
            || script.ends_with("/node_modules/@earendil-works/pi-coding-agent/dist/cli.js"))
    {
        ("pi", "literal-pi-package-script")
    } else {
        (alias(script)?, "literal-wrapper-script")
    };
    Some(Match {
        provider,
        pid: fact.pid,
        rule,
    })
}

#[cfg(test)]
#[path = "agent_processes/tests.rs"]
mod tests;
