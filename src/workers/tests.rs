use super::*;
use crate::process::ProbeResult;
use std::{cell::RefCell, collections::BTreeMap, fs};

fn help_text(route: &[&str], commands: &[&str], flags: &[&str]) -> Vec<u8> {
    let mut lines = vec![format!(
        "Usage: djinn {}",
        route
            .iter()
            .copied()
            .chain(["[OPTIONS]"])
            .collect::<Vec<_>>()
            .join(" ")
    )];
    if !commands.is_empty() {
        lines.extend([String::new(), "Commands:".into()]);
        lines.extend(commands.iter().map(|name| format!("  {name}  Description")));
    }
    lines.extend([String::new(), "Options:".into()]);
    lines.extend(flags.iter().map(|name| format!("  {name}  Description")));
    lines.push(String::new());
    lines.join("\n").into_bytes()
}

fn result(data: Vec<u8>) -> ProbeResult {
    ProbeResult {
        status: ProbeStatus::Ok,
        returncode: Some(0),
        stdout: data,
        stderr: b"warning\n".to_vec(),
        duration_ms: 1.0,
    }
}

fn response(case: &str, args: &[&str]) -> ProbeResult {
    let route = &args[..args.len() - 1];
    let commands: &[&str] = match route {
        [] => &["session", "ask", "agents"],
        ["session"] => &["run", "status", "watch"],
        _ => &[],
    };
    let flags: &[&str] = if case == "version" && route.is_empty() {
        &["--help", "--version"]
    } else {
        &["--help"]
    };
    if args == ["--version"] {
        return result(b"djinn 1.2.3-rc.1+build\n".to_vec());
    }
    if matches!(case, "missing" | "missing-failure" | "missing-parse") && route.is_empty() {
        return result(help_text(&[], &["ask"], &["--help"]));
    }
    if case == "missing-failure" && route == ["ask"] {
        return ProbeResult {
            status: ProbeStatus::Nonzero,
            returncode: Some(7),
            stdout: b"TOKEN=private".to_vec(),
            ..result(Vec::new())
        };
    }
    if case == "missing-parse" && route == ["ask"] {
        return result(b"incomplete\n".to_vec());
    }
    result(help_text(route, commands, flags))
}

struct Call {
    executable: PathBuf,
    arguments: Vec<String>,
    cwd: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    timeout: Duration,
    limit: usize,
}
struct FakeRunner<F> {
    calls: RefCell<Vec<Call>>,
    response: F,
}
impl<F: Fn(&ProbeRequest) -> ProbeResult> ProbeRunner for FakeRunner<F> {
    fn run(&self, request: &ProbeRequest) -> ProbeResult {
        self.calls.borrow_mut().push(Call {
            executable: request.executable.clone(),
            arguments: request
                .arguments
                .iter()
                .map(|a| a.to_str().unwrap().to_owned())
                .collect(),
            cwd: request.cwd.clone(),
            environment: request.environment.clone(),
            timeout: request.timeout,
            limit: request.max_output_bytes,
        });
        (self.response)(request)
    }
}
fn fake(case: &str) -> FakeRunner<impl Fn(&ProbeRequest) -> ProbeResult + '_> {
    FakeRunner {
        calls: RefCell::new(Vec::new()),
        response: move |request: &ProbeRequest| {
            response(
                case,
                &request
                    .arguments
                    .iter()
                    .map(|a| a.to_str().unwrap())
                    .collect::<Vec<_>>(),
            )
        },
    }
}
fn options(path: &Path, json: bool) -> ReadinessOptions<'_> {
    ReadinessOptions {
        adapter: "djinn",
        profile: "research-readonly",
        executable: Some(path),
        timeout: 3.0,
        max_output_bytes: 131072,
        json,
    }
}
fn json(output: &TaskOutput) -> serde_json::Value {
    serde_json::from_str(&output.stdout).unwrap()
}

#[test]
fn help_parser_is_section_and_declaration_bounded() {
    let data = b"Examples:\n  ask  not a command\n\x1b[1mUsage:\x1b[0m djinn [OPTIONS]\nCommands:\n  session  Session\n  session  Again\nOptions:\n  -h, --help  Help\n          --version is prose\n      --zebra  Z\n      --alpha=<VALUE>  A\n";
    let parsed = help::parse(data, &[]).unwrap();
    assert_eq!(parsed.commands, ["session"]);
    assert_eq!(parsed.flags, ["--alpha", "--help", "--zebra"]);
    let osc = b"\x1b]0;title\x07Usage: djinn session [OPTIONS]\nOptions:\n  --help  Help\n";
    assert!(help::parse(osc, &["session"]).is_ok());
    assert!(help::parse(osc, &[]).is_err());
    assert_eq!(help::strip_ansi("a\x1b]title\x1b\\b"), "ab");
    for data in [
        b"session run --help\n".as_slice(),
        b"{\"Commands\":[\"session\"]}\n",
        b"\xff",
        b"Usage: djinn [OPTIONS]\n",
        b"Usage: djinn [OPTIONS]\nCommands:\nsession\n",
        b"Usage: djinn [OPTIONS]\nCommands:\n",
        b"Usage: djinn [OPTIONS]\nOptions:\n",
        b"Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\0\n",
        b"Usage: djinn [OPTIONS]\nOptions:\n  --help  Help",
        b"Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\nOptions:\n  --version  Version\n",
        b"Usage: djinn [OPTIONS]\nUsage: djinn [OPTIONS]\nOptions:\n  --help  Help\n",
        b"Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1bZ\n",
    ] {
        assert!(help::parse(data, &[]).is_err(), "{data:?}");
    }
}

#[test]
fn version_is_bounded_and_not_capability_attestation() {
    for data in ["djinn 1.2.3", "v1.2.3-rc.1+build", "\x1b[32m1.2.3\x1b[0m\n"] {
        assert!(help::version(data.as_bytes()).is_ok());
    }
    for data in [
        "package version unknown",
        "{\"version\":\"1.2.3\"}",
        "1.2",
        "1.2.3+",
        "1.2.3\x1bZ",
        "1.2.3 private",
    ] {
        assert!(help::version(data.as_bytes()).is_err());
    }
    assert!(help::version(format!("1.2.3-{}", "x".repeat(256)).as_bytes()).is_err());
    assert_eq!(
        visible_text("a\n\x1b\u{a0}\u{200b}\u{202e}\u{e000}\u{e0001}é"),
        "a\\u000a\\u001b\\u00a0\\u200b\\u202e\\ue000\\ue0001é"
    );
}

#[test]
fn help_parser_matches_legacy_oracle_vectors() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut vectors: Vec<(Vec<u8>, Vec<&str>)> = Vec::new();
    for route in [vec![], vec!["session"], vec!["session", "run"]] {
        let good = help_text(
            &route,
            &["session", "ask", "session"],
            &["--help", "--version", "--help"],
        );
        vectors.push((good.clone(), route.clone()));
        vectors.push((good[..good.len() - 1].to_vec(), route.clone()));
        vectors.push((
            String::from_utf8(good.clone())
                .unwrap()
                .replace("Usage:", "\x1b[1mUsage:\x1b[0m")
                .into_bytes(),
            route.clone(),
        ));
        vectors.push((
            String::from_utf8(good)
                .unwrap()
                .replace("  --version  Description", "          --version is prose")
                .into_bytes(),
            route,
        ));
    }
    for text in [
        "Usage: djinn [OPTIONS]\nOptions:\n  -h, --help  Help\n      --version  Version\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\n          --version is prose\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help[=VALUE]  Help\n",
        "Usage: djinn [OPTIONS]\nCommands:\n    Wrapped prose\nOptions:\n  --help  Help\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  -h  Help\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\nOptions:\n  --help  Again\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1bZ\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1b]title\x07\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1b]title\x1b\\\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1b]unterminated\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\x1b[31\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\0\n",
        "Usage: djinn [OPTIONS]\nCommands:\n  status  Session\n  watch  Session\nOptions:\n  --help  Help\n",
        "Usage: djinn bare [OPTIONS]\nOptions:\n  --help  Help\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help!  Help\n",
        "Usage: djinn [OPTIONS]\nOptions:\n    wrapped prose\n",
        "Usage: djinn [OPTIONS]\nOptions:\n  --help  Help\nUsage: djinn [OPTIONS]\n",
        "Usage: djinn [OPTIONS]\r\nOptions:\r\n  --help  Help\r\n",
    ] {
        vectors.push((text.as_bytes().to_vec(), vec![]));
    }
    vectors.push((vec![255], vec![]));
    let input: Vec<_> = vectors
        .iter()
        .map(|(data, route)| serde_json::json!({"data":data,"route":route}))
        .collect();
    let mut child = Command::new("python3")
        .args([
            "-B",
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/workers/oracle.py"),
            "--parse",
        ])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    for ((data, route), expected) in vectors.iter().zip(expected) {
        let actual = help::parse(data, route);
        match actual {
            Ok(help) => {
                assert_eq!(
                    serde_json::json!(help.commands),
                    expected["commands"],
                    "{data:?}"
                );
                assert_eq!(serde_json::json!(help.flags), expected["flags"], "{data:?}");
                assert_eq!(digest(data), expected["help_sha256"], "{data:?}");
            }
            Err(_) => assert!(expected.is_null(), "{data:?}: {expected}"),
        }
    }
}

#[cfg(unix)]
fn executable(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("fixture-é\n");
    fs::write(&path, b"#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn python_oracle_matches_complete_reports_and_exact_human_renderer() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    for case in [
        "complete",
        "version",
        "missing",
        "missing-failure",
        "missing-parse",
    ] {
        let runner = fake(case);
        let output = readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap();
        let human = readiness_with_runner(temp.path(), options(&path, false), &fake(case)).unwrap();
        let oracle = Command::new("python3")
            .args([
                "-B",
                concat!(env!("CARGO_MANIFEST_DIR"), "/src/workers/oracle.py"),
            ])
            .arg(temp.path())
            .arg(&path)
            .arg(case)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env_remove("COVEN_HOME")
            .env_remove("COVEN_COMMAND")
            .output()
            .unwrap();
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let mut expected: serde_json::Value = serde_json::from_slice(&oracle.stdout).unwrap();
        let mut actual = json(&output);
        for report in [&mut actual, &mut expected["report"]] {
            for probe in report["probes"].as_array_mut().unwrap() {
                probe.as_object_mut().unwrap().remove("observed_at");
            }
        }
        assert_eq!(actual, expected["report"], "{case}");
        assert_eq!(human.stdout, expected["human"].as_str().unwrap(), "{case}");
        assert_eq!(
            output.exit_code,
            expected["exit_code"].as_u64().unwrap() as u8
        );
        assert!(!output.stdout.ends_with('\n') && !human.stdout.ends_with('\n'));
        assert!(output.stdout.contains('é'));
        assert!(!human.stdout.contains("fixture-é\n"));
    }
}

#[cfg(unix)]
#[test]
fn provenance_routes_environment_and_limits_are_exact() {
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    let runner = fake("version");
    let output = readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap();
    let report = json(&output);
    assert_eq!(report["assessment"], "unknown");
    assert_eq!(output.exit_code, 2);
    assert_eq!(report["authorization"], "none");
    assert!(report.get("dispatchable").is_none());
    assert_eq!(report["capabilities"].as_object().unwrap().len(), 11);
    assert!(
        report["capabilities"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v["state"] == "unknown")
    );
    let expected = [
        vec!["--help"],
        vec!["session", "--help"],
        vec!["session", "run", "--help"],
        vec!["session", "status", "--help"],
        vec!["session", "watch", "--help"],
        vec!["ask", "--help"],
        vec!["agents", "--help"],
        vec!["--version"],
    ];
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 8);
    for ((call, expected), provenance) in calls
        .iter()
        .zip(expected)
        .zip(report["probes"].as_array().unwrap())
    {
        assert_eq!(call.arguments, expected);
        assert_eq!(call.executable, path);
        assert_eq!(call.cwd, temp.path().canonicalize().unwrap());
        assert_eq!(call.environment, identity::adapter_environment());
        assert!(call.environment.keys().all(|k| {
            ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"]
                .iter()
                .any(|allowed| k == allowed)
        }));
        assert_eq!(call.timeout, Duration::from_secs(3));
        assert_eq!(call.limit, 131072);
        let result = response("version", &expected);
        assert_eq!(provenance["stdout_sha256"], digest(&result.stdout));
        assert_eq!(provenance["stderr_sha256"], digest(&result.stderr));
        assert_eq!(
            provenance["output_sha256"],
            digest(&[result.stdout.clone(), result.stderr.clone()].concat())
        );
        assert_eq!(provenance["stdout_size"], result.stdout.len());
        assert_eq!(provenance["stderr_size"], result.stderr.len());
        assert_eq!(
            provenance["output_size"],
            result.stdout.len() + result.stderr.len()
        );
        assert_eq!(provenance["duration_ms"], 1.0);
        assert_eq!(provenance["returncode"], 0);
        assert!(
            provenance["observed_at"]
                .as_str()
                .unwrap()
                .ends_with("+00:00")
        );
    }
    assert_eq!(
        report["executable"]["identity"],
        report["executable"]["identity_after"]
    );
}

#[cfg(unix)]
#[test]
fn missing_route_never_overrides_transport_parse_or_identity_failure() {
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    for status in [
        ProbeStatus::Nonzero,
        ProbeStatus::Timeout,
        ProbeStatus::OutputLimit,
        ProbeStatus::SpawnError,
    ] {
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            response: |request: &ProbeRequest| {
                if request.arguments.len() == 1 {
                    response("missing", &["--help"])
                } else {
                    ProbeResult {
                        status,
                        returncode: Some(7),
                        stdout: b"TOKEN=private".to_vec(),
                        ..result(Vec::new())
                    }
                }
            },
        };
        let output = readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap();
        assert_eq!(output.exit_code, 2);
        assert_eq!(json(&output)["assessment"], "unknown");
        assert!(!output.stdout.contains("TOKEN=private"));
        assert!(output.stdout.contains("probe_failure"));
        assert!(json(&output)["executable"].get("identity_after").is_some());
    }
    for data in [
        b"incomplete\n".to_vec(),
        help_text(&["ask"], &[], &["--help"]),
    ] {
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            response: |request: &ProbeRequest| {
                if request.arguments.len() == 1 {
                    response("missing", &["--help"])
                } else {
                    ProbeResult {
                        stdout: data.clone(),
                        stderr: vec![255],
                        ..result(Vec::new())
                    }
                }
            },
        };
        let output = readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap();
        assert_eq!(output.exit_code, 2);
        assert!(output.stdout.contains("probe_parse_error"));
    }
    let before = fs::metadata(&path).unwrap();
    let runner = FakeRunner {
        calls: RefCell::new(Vec::new()),
        response: |_: &ProbeRequest| {
            let bytes = fs::read(&path).unwrap();
            fs::write(&path, vec![b'X'; bytes.len()]).unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
                .unwrap();
            result(help_text(&[], &[], &["--help"]))
        },
    };
    let output = readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap();
    assert_eq!(output.exit_code, 2);
    assert!(output.stdout.contains("executable_changed"));
    let report = json(&output);
    assert!(!output.stdout.contains("probe_parse_error"));
    assert_eq!(report["probes"].as_array().unwrap().len(), 1);
    for field in ["device", "inode", "size", "mtime_ns"] {
        assert_eq!(
            report["executable"]["identity"][field],
            report["executable"]["identity_after"][field]
        );
    }
    assert_ne!(
        report["executable"]["identity"]["sha256"],
        report["executable"]["identity_after"]["sha256"]
    );
}

#[cfg(unix)]
#[test]
fn alias_is_preserved_and_rechecked_before_every_probe_and_after_last() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    let output =
        readiness_with_runner(temp.path(), options(&alias, true), &fake("complete")).unwrap();
    assert_eq!(
        json(&output)["executable"]["selected_path"],
        alias.to_str().unwrap()
    );
    assert_eq!(
        json(&output)["executable"]["resolved_path"],
        path.to_str().unwrap()
    );
    let replacement = temp.path().join("replacement");
    fs::copy(&path, &replacement).unwrap();
    for case in ["complete", "missing"] {
        fs::remove_file(&alias).unwrap();
        symlink(&path, &alias).unwrap();
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            response: |_: &ProbeRequest| {
                fs::remove_file(&alias).unwrap();
                symlink(&replacement, &alias).unwrap();
                response(case, &["--help"])
            },
        };
        let output = readiness_with_runner(temp.path(), options(&alias, true), &runner).unwrap();
        assert_eq!(runner.calls.borrow().len(), 1);
        assert_eq!(output.exit_code, 2);
        assert!(output.stdout.contains("executable_changed"));
    }
}

#[cfg(unix)]
#[test]
fn symlink_parent_selector_pins_and_invokes_trusted_target_not_lexical_decoy() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let caller = root.path().join("caller");
    let trusted = root.path().join("trusted");
    fs::create_dir(&caller).unwrap();
    fs::create_dir(&trusted).unwrap();
    fs::create_dir(trusted.join("subdir")).unwrap();
    let target = executable(&trusted);
    let decoy = executable(&caller);
    fs::write(&decoy, b"#!/bin/sh\nexit 88\n").unwrap();
    symlink(trusted.join("subdir"), caller.join("link")).unwrap();
    let selector = caller.join("link/..").join(target.file_name().unwrap());
    let runner = fake("complete");
    let output = readiness_with_runner(&caller, options(&selector, true), &runner).unwrap();
    let report = json(&output);
    assert_eq!(
        report["executable"]["selected_path"],
        selector.to_str().unwrap()
    );
    assert_eq!(
        report["executable"]["resolved_path"],
        target.to_str().unwrap()
    );
    assert_eq!(
        report["executable"]["identity"]["sha256"],
        digest(&fs::read(&target).unwrap())
    );
    assert_ne!(
        report["executable"]["identity"]["sha256"],
        digest(&fs::read(&decoy).unwrap())
    );
    assert!(
        runner
            .calls
            .borrow()
            .iter()
            .all(|call| call.executable == target)
    );
}

#[cfg(unix)]
#[test]
fn help_surface_item_bound_is_consistent_before_either_renderer() {
    use std::fmt::Write;
    let root = tempfile::tempdir().unwrap();
    let executable = executable(root.path());
    for section in ["Commands", "Options"] {
        for count in [crate::values::MAX_ITEMS, crate::values::MAX_ITEMS + 1] {
            let mut text = format!("Usage: djinn [OPTIONS]\n{section}:\n");
            for index in 0..count {
                let prefix = if section == "Commands" { "c" } else { "--f" };
                writeln!(text, "  {prefix}{index:05}").unwrap();
            }
            if count == crate::values::MAX_ITEMS {
                // Repeated declarations do not consume another distinct item.
                writeln!(
                    text,
                    "  {}00000",
                    if section == "Commands" { "c" } else { "--f" }
                )
                .unwrap();
            }
            assert!(text.len() <= 1_048_576);
            let runner = FakeRunner {
                calls: RefCell::new(Vec::new()),
                response: |_: &ProbeRequest| result(text.as_bytes().to_vec()),
            };
            for json_output in [false, true] {
                let mut request = options(&executable, json_output);
                request.max_output_bytes = 1_048_576;
                let output = readiness_with_runner(root.path(), request, &runner).unwrap();
                assert_eq!(
                    output.exit_code,
                    if count == crate::values::MAX_ITEMS {
                        1
                    } else {
                        2
                    }
                );
                if count == crate::values::MAX_ITEMS {
                    assert!(!output.stdout.contains("probe_parse_error"));
                } else {
                    assert!(output.stdout.contains("probe_parse_error"));
                    assert!(
                        !output.stdout.contains("c00000") && !output.stdout.contains("--f00000")
                    );
                }
                if json_output {
                    let report = json(&output);
                    assert_eq!(report["authorization"], "none");
                    let capabilities = report["capabilities"].as_object().unwrap();
                    assert_eq!(capabilities.len(), 11);
                    assert!(capabilities.values().all(|v| v["state"] == "unknown"));
                    let probe = &report["probes"][0];
                    assert_eq!(probe["status"], "ok");
                    assert_eq!(probe["stdout_size"], text.len());
                    assert_eq!(probe["stdout_sha256"], digest(text.as_bytes()));
                    assert_eq!(
                        report["executable"]["identity"],
                        report["executable"]["identity_after"]
                    );
                    if count == crate::values::MAX_ITEMS {
                        let field = if section == "Commands" {
                            "commands"
                        } else {
                            "flags"
                        };
                        assert_eq!(
                            report["cli_surfaces"]["root"][field]
                                .as_array()
                                .unwrap()
                                .len(),
                            count
                        );
                        assert_eq!(report["assessment"], "blocked");
                    } else {
                        assert_eq!(report["assessment"], "unknown");
                        assert_eq!(probe["parse_status"], "error");
                        assert_eq!(report["cli_surfaces"]["root"]["state"], "unknown");
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn invalid_requests_and_unavailable_executables_do_not_probe() {
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    let runner = fake("complete");
    for timeout in [0.0, -1.0, 31.0, f64::NAN, f64::INFINITY] {
        let mut request = options(&path, true);
        request.timeout = timeout;
        assert!(readiness_with_runner(temp.path(), request, &runner).is_err());
    }
    for limit in [i64::MIN, 1023, 1048577, i64::MAX] {
        let mut request = options(&path, true);
        request.max_output_bytes = limit;
        assert!(readiness_with_runner(temp.path(), request, &runner).is_err());
    }
    for (adapter, profile) in [("other", "research-readonly"), ("djinn", "read-only")] {
        let mut request = options(&path, true);
        request.adapter = adapter;
        request.profile = profile;
        assert!(readiness_with_runner(temp.path(), request, &runner).is_err());
    }
    assert!(readiness_with_runner(&path, options(&path, true), &runner).is_err());
    for selected in [temp.path().join("missing"), temp.path().to_owned()] {
        let output = readiness_with_runner(temp.path(), options(&selected, true), &runner).unwrap();
        assert_eq!(output.exit_code, 2);
        assert_eq!(json(&output)["probes"], serde_json::json!([]));
    }
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len((1 << 30) + 1)
        .unwrap();
    assert_eq!(
        json(&readiness_with_runner(temp.path(), options(&path, true), &runner).unwrap())["probes"],
        serde_json::json!([])
    );
    assert!(runner.calls.borrow().is_empty());
}

#[cfg(unix)]
#[test]
fn bounded_provenance_withholds_outputs_and_workspace_is_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let path = executable(temp.path());
    fs::create_dir(temp.path().join("state")).unwrap();
    for name in [
        "coven.json",
        "coven.toml",
        "state/tasks.json",
        "state/runtime-launch-control.json",
        "state/execution.json",
        "state/.workspace.lock",
        "state/.runtime-transport.lock",
    ] {
        fs::write(temp.path().join(name), b"corrupt\xff\0record").unwrap();
    }
    let before = snapshot(temp.path());
    for state in ["complete", "missing", "missing-failure", "missing-parse"] {
        readiness_with_runner(temp.path(), options(&path, true), &fake(state)).unwrap();
        assert_eq!(snapshot(temp.path()), before);
    }
    let runner = FakeRunner {
        calls: RefCell::new(Vec::new()),
        response: |_: &ProbeRequest| ProbeResult {
            stdout: vec![b'x'; 800],
            stderr: vec![b'y'; 800],
            duration_ms: 1.23456,
            ..result(Vec::new())
        },
    };
    let mut request = options(&path, true);
    request.max_output_bytes = 1024;
    let output = readiness_with_runner(temp.path(), request, &runner).unwrap();
    let report = json(&output);
    let probe = &report["probes"][0];
    assert_eq!(probe["status"], "output_limit");
    assert_eq!(probe["output_size"], 1024);
    assert_eq!(probe["stdout_size"], 800);
    assert_eq!(probe["stderr_size"], 224);
    assert_eq!(probe["stderr_sha256"], digest(&vec![b'y'; 224]));
    assert_eq!(probe["duration_ms"], 1.235);
    assert_eq!(snapshot(temp.path()), before);
    let ordinary = tempfile::tempdir().unwrap();
    readiness_with_runner(ordinary.path(), options(&path, true), &fake("complete")).unwrap();
    assert!(fs::read_dir(ordinary.path()).unwrap().next().is_none());
}

#[cfg(unix)]
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut values = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            values.extend(snapshot(&path));
        } else {
            values.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    values
}
