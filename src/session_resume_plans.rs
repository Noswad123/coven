//! Pure inspection of legacy resume arguments, never execution or authorization.
use crate::{Result, error::require, session_support, task_mutation::TaskOutput};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Id,
    Path,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub runtime: String,
    pub source: String,
    pub session_kind: SessionKind,
    pub session: String,
    pub executable: Option<String>,
}

const LIMITATIONS: [&str; 4] = [
    "Inspection only: saved reports are not execution inputs and grant no execution capability.",
    "Source labels are caller-supplied and unauthenticated.",
    "Provider CLI versions and session existence are unverified.",
    "Executable resolution, delivery, native lifecycle and readiness are not established by this report.",
];

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(char::is_control)
        && !value.trim_start().starts_with('-')
}

// No public constructor or deserialization: every plan passes the same validation.
struct Plan {
    argv: Vec<String>,
}

impl Plan {
    fn validated(request: &Request) -> Result<Option<Self>> {
        require(valid_text(&request.source, 512), "invalid_resume_source")?;
        require(valid_text(&request.runtime, 512), "invalid_resume_runtime")?;
        let session_limit = match request.session_kind {
            SessionKind::Id => 512,
            SessionKind::Path => 4096,
        };
        require(
            valid_text(&request.session, session_limit)
                && (request.session_kind != SessionKind::Path || request.session.starts_with('/')),
            "invalid_session_selector",
        )?;
        if let Some(executable) = &request.executable {
            require(
                valid_text(executable, 4096)
                    && executable.starts_with('/')
                    && !executable.ends_with('/')
                    && !executable.split('/').any(|component| component == ".."),
                "invalid_executable_path",
            )?;
        }

        use SessionKind::{Id, Path};
        let (binary, flag, joined) = match (
            request.source.as_str(),
            request.runtime.as_str(),
            request.session_kind,
        ) {
            ("kitsune:claude", "claude", Id) => ("claude", "--resume", false),
            ("kitsune:codex", "codex", Id) => ("codex", "resume", false),
            ("kitsune:copilot", "copilot", Id) => ("copilot", "--resume=", true),
            ("kitsune:devin", "devin", Id) => ("devin", "--resume", false),
            ("kitsune:droid", "droid", Id) => ("droid", "--resume", false),
            ("kitsune:djinn", "djinn", Id) => ("djinn", "-s", false),
            ("kitsune:kimi", "kimi", Id) => ("kimi", "--session", false),
            ("kitsune:mastracode", "mastracode", Id) => ("mastracode", "--thread", false),
            ("kitsune:pi", "pi", Id | Path) => ("pi", "--session", false),
            ("kitsune:omp", "omp", Id | Path) => ("omp", "--resume=", true),
            ("kitsune:hermes", "hermes", Id) => ("hermes", "--resume", false),
            ("kitsune:opencode", "opencode", Id) => ("opencode", "--session", false),
            ("kitsune:qodercli", "qodercli", Id) => ("qodercli", "--resume", false),
            ("kitsune:kilo", "kilo", Id) => ("kilo", "--session", false),
            ("kitsune:cursor", "cursor", Id) => ("cursor-agent", "--resume", false),
            ("kitsune:grok", "grok", Id) => ("grok", "--resume", false),
            _ => return Ok(None),
        };
        let mut argv = vec![request.executable.as_deref().unwrap_or(binary).to_owned()];
        if joined {
            argv.push(format!("{flag}{}", request.session));
        } else {
            argv.push(flag.to_owned());
            argv.push(request.session.clone());
        }
        Ok(Some(Self { argv }))
    }
}

pub fn arguments(request: &Request, json_output: bool) -> Result<TaskOutput> {
    let plan = Plan::validated(request)?;
    let outcome = if plan.is_some() {
        "planned"
    } else {
        "unsupported"
    };
    let exit_code = if plan.is_some() { 0 } else { 2 };
    let mut report = json!({
        "contract": "coven.session-resume-plan.v1",
        "schema_version": 1,
        "runtime": request.runtime,
        "source": request.source,
        "session_kind": match request.session_kind { SessionKind::Id => "id", SessionKind::Path => "path" },
        "session_value": request.session,
        "outcome": outcome,
        "reference_trust": "caller_supplied_unverified",
        "executable_resolution": if request.executable.is_some() { "unchecked_absolute_override" } else { "not_attempted" },
        "delivery": "not_attempted",
        "authorization": "none",
        "native_lifecycle": "unestablished",
        "readiness": "unestablished",
        "limitations": LIMITATIONS,
    });
    if let Some(plan) = &plan {
        report["argv"] = json!(plan.argv);
    }
    let mut output = if json_output {
        session_support::output(&report, false)?
    } else {
        let mut stdout = format!(
            "Resume argument inspection: {outcome}. No execution capability or authorization; delivery not attempted."
        );
        if let Some(plan) = &plan {
            let argv = crate::output::ascii_json(&serde_json::to_string(&plan.argv)?)?;
            stdout.push_str(&format!("\nargv (JSON data): {argv}"));
        }
        for limitation in LIMITATIONS {
            stdout.push('\n');
            stdout.push_str(limitation);
        }
        TaskOutput {
            stdout,
            warnings: Vec::new(),
            exit_code,
        }
    };
    output.exit_code = exit_code;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn request(runtime: &str, kind: SessionKind, session: &str) -> Request {
        Request {
            runtime: runtime.into(),
            source: format!("kitsune:{runtime}"),
            session_kind: kind,
            session: session.into(),
            executable: None,
        }
    }

    fn report(request: &Request, code: u8) -> Value {
        let output = arguments(request, true).unwrap();
        assert_eq!(output.exit_code, code);
        assert!(output.warnings.is_empty());
        serde_json::from_str(&output.stdout).unwrap()
    }

    fn invalid(request: &Request, code: &str) {
        for json in [false, true] {
            assert_eq!(arguments(request, json).err().unwrap().code(), code);
        }
    }

    #[test]
    fn full_legacy_mapping_and_exact_pairs() {
        let cases = [
            ("claude", vec!["claude", "--resume", "saved"]),
            ("codex", vec!["codex", "resume", "saved"]),
            ("copilot", vec!["copilot", "--resume=saved"]),
            ("devin", vec!["devin", "--resume", "saved"]),
            ("droid", vec!["droid", "--resume", "saved"]),
            ("djinn", vec!["djinn", "-s", "saved"]),
            ("kimi", vec!["kimi", "--session", "saved"]),
            ("mastracode", vec!["mastracode", "--thread", "saved"]),
            ("pi", vec!["pi", "--session", "saved"]),
            ("omp", vec!["omp", "--resume=saved"]),
            ("hermes", vec!["hermes", "--resume", "saved"]),
            ("opencode", vec!["opencode", "--session", "saved"]),
            ("qodercli", vec!["qodercli", "--resume", "saved"]),
            ("kilo", vec!["kilo", "--session", "saved"]),
            ("cursor", vec!["cursor-agent", "--resume", "saved"]),
            ("grok", vec!["grok", "--resume", "saved"]),
        ];
        for (runtime, expected) in &cases {
            let req = request(runtime, SessionKind::Id, "saved");
            let value = report(&req, 0);
            assert_eq!(value["argv"], json!(expected), "{runtime}");
            assert_eq!(value["outcome"], "planned");
            for (other, _) in &cases {
                if other != runtime {
                    let mut mismatch = req.clone();
                    mismatch.source = format!("kitsune:{other}");
                    assert!(report(&mismatch, 2).get("argv").is_none());
                }
            }
            let path = request(runtime, SessionKind::Path, "/saved/session.jsonl");
            if *runtime == "pi" {
                assert_eq!(
                    report(&path, 0)["argv"],
                    json!(["pi", "--session", "/saved/session.jsonl"])
                );
            } else if *runtime == "omp" {
                assert_eq!(
                    report(&path, 0)["argv"],
                    json!(["omp", "--resume=/saved/session.jsonl"])
                );
            } else {
                assert!(report(&path, 2).get("argv").is_none());
            }
        }
    }

    #[test]
    fn invalid_fields_are_rejected_even_for_unknown_pairs() {
        let bad = [
            "",
            " ",
            "\u{2003}",
            "-option",
            "  -option",
            "a\n",
            "a\0",
            "a\u{85}",
        ];
        for text in bad {
            let mut req = request("unknown", SessionKind::Id, "saved");
            req.source = text.into();
            invalid(&req, "invalid_resume_source");
            req.source = "custom:unknown".into();
            req.runtime = text.into();
            invalid(&req, "invalid_resume_runtime");
            req.runtime = "unknown".into();
            req.session = text.into();
            invalid(&req, "invalid_session_selector");
            req.session_kind = SessionKind::Path;
            invalid(&req, "invalid_session_selector");
        }
        for path in ["relative", " /absolute", "C:\\session", "/bad\npath"] {
            invalid(
                &request("pi", SessionKind::Path, path),
                "invalid_session_selector",
            );
        }
    }

    #[test]
    fn bounds_measure_utf8_bytes_without_normalizing() {
        let at_limit = "\u{e9}".repeat(256);
        let mut req = request("claude", SessionKind::Id, &at_limit);
        assert_eq!(report(&req, 0)["session_value"], at_limit);
        req.session.push('a');
        invalid(&req, "invalid_session_selector");
        req.session = "saved".into();
        for source in [true, false] {
            if source {
                req.source = at_limit.clone();
            } else {
                req.source = "kitsune:claude".into();
                req.runtime = at_limit.clone();
            }
            report(&req, 2);
            if source {
                req.source.push('a');
            } else {
                req.runtime.push('a');
            }
            invalid(
                &req,
                if source {
                    "invalid_resume_source"
                } else {
                    "invalid_resume_runtime"
                },
            );
        }
        let path = format!("/{}a", "\u{e9}".repeat(2047));
        assert_eq!(path.len(), 4096);
        let mut req = request("pi", SessionKind::Path, &path);
        report(&req, 0);
        req.session.push('a');
        invalid(&req, "invalid_session_selector");
        req.session = "/session".into();
        req.executable = Some(path);
        report(&req, 0);
        req.executable.as_mut().unwrap().push('a');
        invalid(&req, "invalid_executable_path");
    }

    #[test]
    fn selectors_are_literal_data_and_human_argv_is_escaped_json() {
        let session = "  saved; $(touch /tmp/nope) `cmd` 'quoted' \"x\" \\ \u{202e}  ";
        for runtime in ["claude", "copilot", "omp"] {
            let req = request(runtime, SessionKind::Id, session);
            let value = report(&req, 0);
            let expected = if runtime == "claude" {
                session.to_owned()
            } else {
                format!("--resume={session}")
            };
            assert_eq!(
                value["argv"].as_array().unwrap().last().unwrap(),
                &json!(expected)
            );
            assert_eq!(value["session_value"], session);
            let human = arguments(&req, false).unwrap();
            assert!(human.stdout.is_ascii());
            let encoded = human
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix("argv (JSON data): "))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(encoded).unwrap(),
                value["argv"]
            );
        }
        let req = request("pi", SessionKind::Path, "/a/../b//session ");
        assert_eq!(report(&req, 0)["argv"][2], req.session);
    }

    #[test]
    fn executable_override_is_lexical_and_validated_before_mapping() {
        for executable in [
            "",
            " ",
            "/",
            "//",
            "relative",
            "-option",
            " /bin/tool",
            "/bin/",
            "/a/../tool",
            "/a/..",
            "/a\ntool",
            "/a\u{85}tool",
        ] {
            let mut req = request("unknown", SessionKind::Id, "saved");
            req.executable = Some(executable.into());
            invalid(&req, "invalid_executable_path");
        }
        for executable in [
            "/nonexistent/cursor-agent",
            "/a//./literal;$(cmd)",
            "/a/..tool",
        ] {
            let mut req = request("cursor", SessionKind::Id, "saved");
            req.executable = Some(executable.into());
            let value = report(&req, 0);
            assert_eq!(value["argv"], json!([executable, "--resume", "saved"]));
            assert_eq!(
                value["executable_resolution"],
                "unchecked_absolute_override"
            );
            req.runtime = "unknown".into();
            assert!(report(&req, 2).get("argv").is_none());
        }
    }

    #[test]
    fn report_contract_and_unsupported_have_no_execution_authority() {
        for runtime in ["claude", "unknown", "Claude", " claude", "claude "] {
            let req = request(runtime, SessionKind::Id, "saved");
            let supported = runtime == "claude";
            let value = report(&req, if supported { 0 } else { 2 });
            assert_eq!(value["contract"], "coven.session-resume-plan.v1");
            assert_eq!(value["schema_version"], 1);
            assert_eq!(value["runtime"], runtime);
            assert_eq!(value["source"], req.source);
            assert_eq!(value["session_kind"], "id");
            assert_eq!(value["session_value"], "saved");
            assert_eq!(
                value["outcome"],
                if supported { "planned" } else { "unsupported" }
            );
            assert_eq!(value.get("argv").is_some(), supported);
            assert!(value.get("dedupe_key").is_none());
            for (key, expected) in [
                ("reference_trust", "caller_supplied_unverified"),
                ("executable_resolution", "not_attempted"),
                ("delivery", "not_attempted"),
                ("authorization", "none"),
                ("native_lifecycle", "unestablished"),
                ("readiness", "unestablished"),
            ] {
                assert_eq!(value[key], expected);
            }
            assert_eq!(value["limitations"], json!(LIMITATIONS));
            assert_eq!(
                arguments(&req, false).unwrap().exit_code,
                if supported { 0 } else { 2 }
            );
        }
        let mut custom = request("claude", SessionKind::Id, "saved");
        custom.source = "custom:claude".into();
        assert!(report(&custom, 2).get("argv").is_none());
    }
}
