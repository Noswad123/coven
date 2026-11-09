//! Native CLI inspection tests: no provider, Python, socket server, or live API.
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

const ID_CASES: &[(&str, &[&str])] = &[
    ("claude", &["claude", "--resume", "saved"]),
    ("codex", &["codex", "resume", "saved"]),
    ("copilot", &["copilot", "--resume=saved"]),
    ("devin", &["devin", "--resume", "saved"]),
    ("droid", &["droid", "--resume", "saved"]),
    ("djinn", &["djinn", "-s", "saved"]),
    ("kimi", &["kimi", "--session", "saved"]),
    ("mastracode", &["mastracode", "--thread", "saved"]),
    ("pi", &["pi", "--session", "saved"]),
    ("omp", &["omp", "--resume=saved"]),
    ("hermes", &["hermes", "--resume", "saved"]),
    ("opencode", &["opencode", "--session", "saved"]),
    ("qodercli", &["qodercli", "--resume", "saved"]),
    ("kilo", &["kilo", "--session", "saved"]),
    ("cursor", &["cursor-agent", "--resume", "saved"]),
    ("grok", &["grok", "--resume", "saved"]),
];

fn command(cwd: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_coven"));
    cmd.current_dir(cwd);
    cmd
}

fn plan(cwd: &Path, runtime: &str, source: &str, kind: &str, session: &str) -> Command {
    let mut cmd = command(cwd);
    cmd.args([
        "session",
        "plan-resume",
        "--runtime",
        runtime,
        "--source",
        source,
        "--session-kind",
        kind,
        "--session",
        session,
    ]);
    cmd
}

fn status(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn report(cmd: &mut Command, code: i32) -> Value {
    let output = cmd.arg("--json").output().unwrap();
    status(&output, code);
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"));
    assert!(!output.stdout.ends_with(b"\n\n"));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["contract"], "coven.session-resume-plan.v1");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(
        value["outcome"],
        if code == 0 { "planned" } else { "unsupported" }
    );
    for (key, expected) in [
        ("reference_trust", "caller_supplied_unverified"),
        ("delivery", "not_attempted"),
        ("authorization", "none"),
        ("native_lifecycle", "unestablished"),
        ("readiness", "unestablished"),
    ] {
        assert_eq!(value[key], expected);
    }
    assert!(value.get("dedupe_key").is_none());
    if code == 2 {
        assert!(value.get("argv").is_none(), "unsupported must omit argv");
    }
    value
}

fn invalid(cmd: &mut Command, error: &str) {
    let output = cmd.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "command={cmd:?} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, format!("[ERROR] {error}\n").as_bytes());
}

fn syntax_error(output: &Output) {
    status(output, 2);
    assert!(output.stdout.is_empty());
    let error = std::str::from_utf8(&output.stderr).unwrap();
    assert!(error.starts_with("error:"), "{error}");
    for secret in ["SECRET", "saved", "/absent/", "kitsune:claude"] {
        assert!(!error.contains(secret), "operand disclosed: {error}");
    }
}

fn empty(cwd: &Path) {
    assert_eq!(std::fs::read_dir(cwd).unwrap().count(), 0);
}

#[test]
fn all_sixteen_id_mappings_are_independent_explicit_argv() {
    let temp = tempfile::tempdir().unwrap();
    for &(runtime, argv) in ID_CASES {
        let source = format!("kitsune:{runtime}");
        let value = report(&mut plan(temp.path(), runtime, &source, "id", "saved"), 0);
        assert_eq!(value["argv"], json!(argv), "{runtime}");
        assert_eq!(value["runtime"], runtime);
        assert_eq!(value["source"], source);
        assert_eq!(value["session_kind"], "id");
        assert_eq!(value["session_value"], "saved");
        assert_eq!(value["executable_resolution"], "not_attempted");
    }
    empty(temp.path());
}

#[test]
fn only_pi_and_omp_accept_paths_and_paths_remain_literal() {
    let temp = tempfile::tempdir().unwrap();
    let session = "/missing/a/../b//saved session.jsonl";
    for &(runtime, _) in ID_CASES {
        let code = if matches!(runtime, "pi" | "omp") {
            0
        } else {
            2
        };
        let value = report(
            &mut plan(
                temp.path(),
                runtime,
                &format!("kitsune:{runtime}"),
                "path",
                session,
            ),
            code,
        );
        assert_eq!(value["session_kind"], "path");
        assert_eq!(value["session_value"], session);
        match runtime {
            "pi" => assert_eq!(value["argv"], json!(["pi", "--session", session])),
            "omp" => assert_eq!(value["argv"], json!(["omp", format!("--resume={session}")])),
            _ => {}
        }
    }
    empty(temp.path());
}

#[test]
fn exact_source_runtime_pairs_only_no_alias_or_custom_inference() {
    let temp = tempfile::tempdir().unwrap();
    for &(runtime, _) in ID_CASES {
        for &(other, _) in ID_CASES {
            if runtime != other {
                report(
                    &mut plan(
                        temp.path(),
                        runtime,
                        &format!("kitsune:{other}"),
                        "id",
                        "saved",
                    ),
                    2,
                );
            }
        }
    }
    for (runtime, source) in [
        ("claude", "custom:claude"),
        ("claude", "Kitsune:claude"),
        ("claude", "kitsune:Claude"),
        ("claude", " kitsune:claude"),
        ("claude", "kitsune:claude "),
        ("Claude", "kitsune:claude"),
        (" claude", "kitsune:claude"),
        ("claude ", "kitsune:claude"),
        ("unknown", "kitsune:unknown"),
        ("custom-agent", "custom:custom-agent"),
        ("cursor-agent", "kitsune:cursor"),
        ("djinn", "djinn"),
    ] {
        report(&mut plan(temp.path(), runtime, source, "id", "saved"), 2);
    }
    empty(temp.path());
}

#[test]
fn malformed_operands_have_fixed_errors_without_secret_echoes() {
    let temp = tempfile::tempdir().unwrap();
    // NUL cannot be represented in an OS argv; other control classes cross the CLI.
    for bad in [
        "",
        " ",
        "\u{2003}",
        "-SECRET",
        "  -SECRET",
        "\u{2003}-SECRET",
        "SECRET\n",
        "SECRET\t",
        "SECRET\u{7f}",
        "SECRET\u{85}",
    ] {
        for json in [false, true] {
            for field in ["runtime", "source", "session", "executable"] {
                let mut cmd = command(temp.path());
                cmd.args(["session", "plan-resume", "--session-kind=id"]);
                for (operand, valid) in [
                    ("runtime", "unknown"),
                    ("source", "custom:unknown"),
                    ("session", "SECRET-session"),
                ] {
                    cmd.arg(format!(
                        "--{operand}={}",
                        if field == operand { bad } else { valid }
                    ));
                }
                if field == "executable" {
                    cmd.arg(format!("--executable={bad}"));
                }
                if json {
                    cmd.arg("--json");
                }
                invalid(
                    &mut cmd,
                    match field {
                        "runtime" => "invalid_resume_runtime",
                        "source" => "invalid_resume_source",
                        "session" => "invalid_session_selector",
                        _ => "invalid_executable_path",
                    },
                );
            }
        }
    }
    for bad in ["relative-SECRET", " /SECRET", "C:\\SECRET", "/SECRET\n"] {
        invalid(
            &mut plan(temp.path(), "pi", "kitsune:pi", "path", bad),
            "invalid_session_selector",
        );
    }
    for bad in [
        "/",
        "//",
        "relative-SECRET",
        " /SECRET",
        "/SECRET/",
        "/a/../SECRET",
        "/a/..",
        "/SECRET\n",
    ] {
        invalid(
            plan(temp.path(), "unknown", "custom:unknown", "id", "SECRET")
                .args(["--executable", bad]),
            "invalid_executable_path",
        );
    }
    empty(temp.path());
}

#[test]
fn separate_hyphen_operands_distinguish_syntax_from_numeric_prefix_restoration() {
    let temp = tempfile::tempdir().unwrap();
    for bad in ["-SECRET", "-1SECRET", "-.2SECRET", "-\u{663}SECRET"] {
        for json in [false, true] {
            for (field, error) in [
                ("runtime", "invalid_resume_runtime"),
                ("source", "invalid_resume_source"),
                ("session", "invalid_session_selector"),
                ("executable", "invalid_executable_path"),
            ] {
                let mut cmd = plan(
                    temp.path(),
                    if field == "runtime" { bad } else { "unknown" },
                    if field == "source" {
                        bad
                    } else {
                        "custom:unknown"
                    },
                    "id",
                    if field == "session" {
                        bad
                    } else {
                        "SECRET-session"
                    },
                );
                if field == "executable" {
                    cmd.args(["--executable", bad]);
                }
                if json {
                    cmd.arg("--json");
                }
                if bad == "-SECRET" {
                    syntax_error(&cmd.output().unwrap());
                } else {
                    invalid(&mut cmd, error);
                }
            }
        }
    }
    empty(temp.path());
}

#[test]
fn utf8_byte_bounds_are_enforced_without_normalization() {
    let temp = tempfile::tempdir().unwrap();
    for unit in ["a", "\u{e9}"] {
        let short = unit.repeat(512 / unit.len());
        let long = format!("{short}a");
        for (field, error) in [
            ("runtime", "invalid_resume_runtime"),
            ("source", "invalid_resume_source"),
            ("session", "invalid_session_selector"),
        ] {
            for (value, valid) in [(&short, true), (&long, false)] {
                let mut cmd = plan(
                    temp.path(),
                    if field == "runtime" { value } else { "claude" },
                    if field == "source" {
                        value
                    } else {
                        "kitsune:claude"
                    },
                    "id",
                    if field == "session" { value } else { "saved" },
                );
                if valid {
                    let result = report(&mut cmd, if field == "session" { 0 } else { 2 });
                    assert_eq!(
                        result[if field == "session" {
                            "session_value"
                        } else {
                            field
                        }],
                        *value
                    );
                } else {
                    invalid(&mut cmd, error);
                }
            }
        }
        let path = format!(
            "/{}{}",
            unit.repeat(4095 / unit.len()),
            if unit.len() == 2 { "a" } else { "" }
        );
        assert_eq!(path.len(), 4096);
        for (value, valid) in [(&path, true), (&format!("{path}a"), false)] {
            let mut selector = plan(temp.path(), "pi", "kitsune:pi", "path", value);
            let mut executable = plan(temp.path(), "cursor", "kitsune:cursor", "id", "saved");
            executable.args(["--executable", value]);
            if valid {
                assert_eq!(
                    report(&mut selector, 0)["argv"],
                    json!(["pi", "--session", value])
                );
                assert_eq!(
                    report(&mut executable, 0)["argv"],
                    json!([value, "--resume", "saved"])
                );
            } else {
                invalid(&mut selector, "invalid_session_selector");
                invalid(&mut executable, "invalid_executable_path");
            }
        }
    }
    empty(temp.path());
}

#[test]
fn metacharacters_are_data_and_nonexistent_executables_are_lexical() {
    let temp = tempfile::tempdir().unwrap();
    let session = "  saved; $(touch marker) `touch marker` 'quoted' \"x\" \\ \u{202e}  ";
    for (runtime, expected) in [
        ("claude", json!(["claude", "--resume", session])),
        ("copilot", json!(["copilot", format!("--resume={session}")])),
        ("omp", json!(["omp", format!("--resume={session}")])),
    ] {
        let source = format!("kitsune:{runtime}");
        let value = report(&mut plan(temp.path(), runtime, &source, "id", session), 0);
        assert_eq!(value["session_value"], session);
        assert_eq!(value["argv"], expected);
        let human = plan(temp.path(), runtime, &source, "id", session)
            .output()
            .unwrap();
        status(&human, 0);
        assert!(human.stderr.is_empty());
        assert!(human.stdout.is_ascii());
        let text = String::from_utf8(human.stdout).unwrap();
        let encoded = text
            .lines()
            .find_map(|line| line.strip_prefix("argv (JSON data): "))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(encoded).unwrap(), expected);
        assert!(text.contains("No execution capability or authorization"));
    }
    let executable = format!(
        "{}/missing//./literal;$(touch marker)",
        temp.path().display()
    );
    assert!(!Path::new(&executable).exists());
    let value = report(
        plan(temp.path(), "cursor", "kitsune:cursor", "id", "saved")
            .args(["--executable", &executable]),
        0,
    );
    assert_eq!(value["argv"], json!([executable, "--resume", "saved"]));
    assert_eq!(
        value["executable_resolution"],
        "unchecked_absolute_override"
    );
    empty(temp.path());
}

#[test]
fn required_flags_kind_validation_and_no_inference_or_override_self() {
    let temp = tempfile::tempdir().unwrap();
    let required = [
        ("--runtime", "claude"),
        ("--source", "kitsune:claude"),
        ("--session-kind", "id"),
        ("--session", "saved"),
    ];
    for omitted in 0..required.len() {
        let mut cmd = command(temp.path());
        cmd.args(["session", "plan-resume"]);
        for (index, &(flag, value)) in required.iter().enumerate() {
            if index != omitted {
                cmd.args([flag, value]);
            }
        }
        let output = cmd.output().unwrap();
        syntax_error(&output);
    }
    for kind in ["", "ID", "Path", "auto", "-id"] {
        let output = plan(temp.path(), "claude", "kitsune:claude", kind, "saved")
            .output()
            .unwrap();
        syntax_error(&output);
    }
    for args in [
        vec!["--runt", "claude"],
        vec!["--runtime", "claude"],
        vec!["--source", "kitsune:claude"],
        vec!["--session-kind", "id"],
        vec!["--session", "saved"],
        vec!["--executable", "/absent/a", "--executable", "/absent/b"],
    ] {
        let output = plan(temp.path(), "claude", "kitsune:claude", "id", "saved")
            .args(args)
            .output()
            .unwrap();
        syntax_error(&output);
    }
    empty(temp.path());
}

#[test]
fn execution_socket_workspace_timeout_and_report_inputs_are_not_options() {
    let temp = tempfile::tempdir().unwrap();
    for args in [
        vec!["--execute"],
        vec!["--socket", "/absent/socket"],
        vec!["--native-socket", "/absent/native"],
        vec!["--workspace", "/absent/workspace"],
        vec!["/absent/workspace"],
        vec!["--timeout", "1"],
        vec!["--plan", "/absent/report.json"],
        vec!["--report", "/absent/report.json"],
    ] {
        let output = plan(temp.path(), "claude", "kitsune:claude", "id", "saved")
            .args(args)
            .output()
            .unwrap();
        syntax_error(&output);
    }
    empty(temp.path());
}

fn resume(cwd: &Path, runtime: &str) -> Command {
    let mut cmd = command(cwd);
    cmd.args(["session", "resume"]).arg(cwd).args([
        "--runtime",
        runtime,
        "--executable",
        "/absent/djinn",
        "--socket",
        "/absent/socket",
        "--pane",
        "%1",
        "--expected-terminal",
        "123",
        "--session",
        "saved",
        "--execute",
        "--json",
    ]);
    cmd
}

#[test]
fn execution_capable_resume_stays_djinn_only_and_cannot_consume_saved_report() {
    let temp = tempfile::tempdir().unwrap();
    let output = resume(temp.path(), "claude").output().unwrap();
    syntax_error(&output);
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(!error.contains("claude"));
    assert!(!error.contains("djinn"));
    empty(temp.path());

    let report_path = temp.path().join("saved-report.json");
    let output = plan(temp.path(), "claude", "kitsune:claude", "id", "saved")
        .arg("--json")
        .output()
        .unwrap();
    status(&output, 0);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["limitations"].as_array().unwrap().iter().any(|item| {
        item.as_str()
            .unwrap()
            .contains("saved reports are not execution inputs")
    }));
    std::fs::write(&report_path, &output.stdout).unwrap();
    for option in ["--plan", "--report", "--plan-file", "--resume-plan"] {
        let rejected = resume(temp.path(), "djinn")
            .arg(option)
            .arg(&report_path)
            .output()
            .unwrap();
        syntax_error(&rejected);
        assert!(!String::from_utf8_lossy(&rejected.stderr).contains(option));
    }
    assert_eq!(std::fs::read(&report_path).unwrap(), output.stdout);
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn executable_sentinel_is_never_launched_for_planned_or_unsupported() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let executable = fixture.path().join("sentinel");
    // If accidentally launched, record it without depending on a shell command or provider.
    std::fs::write(&executable, "#!/bin/sh\n: > \"$0.launched\"\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let marker = fixture.path().join("sentinel.launched");
    for (runtime, source, code) in [
        ("claude", "kitsune:claude", 0),
        ("unknown", "custom:unknown", 2),
        ("claude", "custom:claude", 2),
    ] {
        let value = report(
            plan(cwd.path(), runtime, source, "id", "saved")
                .arg("--executable")
                .arg(&executable),
            code,
        );
        assert_eq!(
            value["executable_resolution"],
            "unchecked_absolute_override"
        );
        if code == 0 {
            assert_eq!(value["argv"], json!([executable, "--resume", "saved"]));
        }
        assert!(!marker.exists());
        empty(cwd.path());
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
    }
}
