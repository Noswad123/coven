//! Acceptance-owner fixtures: real CLI -> NativeRunner -> private self-exec
//! supervisor -> controlled help producer. No installed Djinn or provider calls.
#![cfg(unix)]
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
    time::{Duration, Instant},
};

fn fixture(root: &Path, mode: &str) -> std::path::PathBuf {
    let python = Command::new("python3")
        .args(["-B", "-c", "import sys; print(sys.executable)"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(python.status.success());
    let python = std::str::from_utf8(&python.stdout).unwrap().trim();
    let path = root.join(format!("{mode}.py"));
    fs::write(
        &path,
        format!(
            "#!{python} -B\n{}",
            include_str!("../src/workers/fixture.py")
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn run(root: &Path, executable: Option<&Path>, extra: &[&str], json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command
        .args(["worker", "readiness"])
        .arg(root.join("workspace"));
    if let Some(path) = executable {
        command.arg("--executable").arg(path);
    }
    command.args(extra);
    if json {
        command.arg("--json");
    }
    command
        .current_dir(root)
        .env("PATH", root)
        .env("HOME", root)
        .env("LC_ALL", "C.UTF-8")
        .env("PROVIDER_TOKEN", "private-token")
        .env("LD_PRELOAD", "")
        .env_remove("COVEN_HOME")
        .env_remove("COVEN_COMMAND")
        .output()
        .unwrap()
}
fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
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
fn workspace(root: &Path) {
    fs::create_dir(root.join("workspace")).unwrap();
    fs::create_dir(root.join("workspace/state")).unwrap();
    for file in [
        "coven.json",
        "coven.toml",
        "state/tasks.json",
        "state/task-authority.json",
        "state/runtime-launch-control.json",
        "state/execution.json",
    ] {
        fs::write(root.join("workspace").join(file), b"corrupt\xffrecord").unwrap();
    }
}

#[test]
fn real_help_advertisement_traversal_and_all_flags_preserve_workspace() {
    let root = tempfile::tempdir().unwrap();
    workspace(root.path());
    let path = fixture(root.path(), "complete");
    let alias = root.path().join("djinn");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let before = snapshot(&root.path().join("workspace"));
    // Discovery uses caller PATH, not a synthetic environment or an installed adapter.
    let output = run(
        root.path(),
        None,
        &[
            "--adapter",
            "djinn",
            "--profile",
            "research-readonly",
            "--timeout",
            "1",
            "--max-output-bytes",
            "65536",
        ],
        true,
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["assessment"], "unknown");
    assert_eq!(report["native_version"], "djinn 1.2.3");
    assert_eq!(
        report["executable"]["selected_path"],
        alias.to_str().unwrap()
    );
    assert_eq!(
        report["executable"]["resolved_path"],
        path.to_str().unwrap()
    );
    assert_eq!(report["probes"].as_array().unwrap().len(), 8);
    let trace = fs::read_to_string(root.path().join("trace")).unwrap();
    let actual: Vec<Value> = trace
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        actual,
        serde_json::json!([
            ["--help"],
            ["session", "--help"],
            ["session", "run", "--help"],
            ["session", "status", "--help"],
            ["session", "watch", "--help"],
            ["ask", "--help"],
            ["agents", "--help"],
            ["--version"]
        ])
        .as_array()
        .unwrap()
        .to_vec()
    );
    assert!(
        report["probes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["status"] == "ok" && p["returncode"] == 0)
    );
    assert_eq!(snapshot(&root.path().join("workspace")), before);
    assert!(!output.stdout.windows(13).any(|w| w == b"private-token"));
    let relative = run(root.path(), Some(Path::new("./djinn")), &[], true);
    assert_eq!(relative.status.code(), Some(2));
    let relative: Value = serde_json::from_slice(&relative.stdout).unwrap();
    assert_eq!(
        relative["executable"]["selected_path"],
        alias.to_str().unwrap()
    );
    assert_eq!(
        relative["executable"]["resolved_path"],
        path.to_str().unwrap()
    );
    // Explicit ~/ selection and the default human renderer use the same runner.
    let output = run(root.path(), Some(Path::new("~/djinn")), &[], false);
    assert_eq!(output.status.code(), Some(2));
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(text.starts_with("djinn adapter compatibility: UNKNOWN\nProfile: research-readonly; adapter contract: djinn-help-v1\n"));
    assert!(text.contains("Native version: djinn 1.2.3\nAuthorization: none."));
    assert!(text.ends_with('\n') && !text.ends_with("\n\n"));
    assert_eq!(snapshot(&root.path().join("workspace")), before);
}

#[test]
fn real_missing_routes_failures_bounds_and_utf8_have_legacy_precedence() {
    for (mode, code, blocker, status, count) in [
        (
            "missing",
            1,
            "unsupported_advertised_session_route",
            "ok",
            3,
        ),
        ("missing-failure", 2, "probe_failure", "ok", 3),
        ("nonzero", 2, "probe_failure", "nonzero", 1),
        ("timeout", 2, "probe_failure", "timeout", 1),
        ("oversize", 2, "probe_failure", "output_limit", 1),
        ("stderr-invalid", 2, "probe_parse_error", "ok", 1),
        ("no-version", 2, "worker_contract_unverified", "ok", 7),
    ] {
        let root = tempfile::tempdir().unwrap();
        workspace(root.path());
        let path = fixture(root.path(), mode);
        let before = snapshot(&root.path().join("workspace"));
        let output = run(
            root.path(),
            Some(&path),
            &["--timeout", "0.3", "--max-output-bytes", "1024"],
            true,
        );
        assert_eq!(
            output.status.code(),
            Some(code),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["probes"][0]["status"], status, "{mode}");
        assert_eq!(report["probes"].as_array().unwrap().len(), count, "{mode}");
        assert!(
            report["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["code"] == blocker),
            "{mode}: {report}"
        );
        assert_eq!(
            report["executable"]["identity"],
            report["executable"]["identity_after"]
        );
        assert!(
            report["probes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["output_size"].as_u64().unwrap() <= 1024)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("TOKEN=private"));
        assert_eq!(snapshot(&root.path().join("workspace")), before);
    }
}

#[test]
fn real_descendants_are_cleaned_after_timeout_and_success() {
    for mode in ["inherited", "closed-descendant"] {
        let root = tempfile::tempdir().unwrap();
        workspace(root.path());
        let path = fixture(root.path(), mode);
        let started = Instant::now();
        let output = run(root.path(), Some(&path), &["--timeout", "0.15"], true);
        assert_eq!(output.status.code(), Some(2));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let status = if mode == "inherited" { "timeout" } else { "ok" };
        assert_eq!(report["probes"][0]["status"], status, "{report}");
        assert!(started.elapsed() < Duration::from_secs(4));
        std::thread::sleep(Duration::from_millis(800));
        assert!(!root.path().join("escaped-descendant").exists(), "{mode}");
    }
}

#[test]
fn real_explicit_and_path_discovery_preserve_symlink_parent_resolution() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let caller = root.path().join("caller");
    let trusted = root.path().join("trusted");
    fs::create_dir(&caller).unwrap();
    fs::create_dir(&trusted).unwrap();
    fs::create_dir(trusted.join("subdir")).unwrap();
    workspace(&caller);
    let target = trusted.join("djinn");
    let decoy = caller.join("djinn");
    for (path, flag) in [(&target, "--trusted"), (&decoy, "--decoy")] {
        fs::write(
            path,
            format!("#!/bin/sh\nprintf 'Usage: djinn [OPTIONS]\\nOptions:\\n  {flag}  Help\\n'\n"),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::os::unix::fs::symlink(trusted.join("subdir"), caller.join("link")).unwrap();
    let selector = caller.join("link/../djinn");
    let before = snapshot(&caller.join("workspace"));
    for mode in ["explicit", "relative-path", "absolute-path"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
        command
            .args(["worker", "readiness"])
            .arg(caller.join("workspace"))
            .arg("--json");
        if mode == "explicit" {
            command.args(["--executable", "./link/../djinn"]);
        }
        let path = if mode == "absolute-path" {
            caller.join("link/..")
        } else {
            "link/..".into()
        };
        let output = command
            .current_dir(&caller)
            .env("PATH", path)
            .env_remove("COVEN_HOME")
            .env_remove("COVEN_COMMAND")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["executable"]["selected_path"],
            selector.to_str().unwrap(),
            "{mode}"
        );
        assert_eq!(
            report["executable"]["resolved_path"],
            target.to_str().unwrap(),
            "{mode}"
        );
        assert_eq!(
            report["executable"]["identity"]["sha256"],
            format!("{:x}", Sha256::digest(fs::read(&target).unwrap())),
            "{mode}"
        );
        assert_eq!(
            report["executable"]["identity"],
            report["executable"]["identity_after"]
        );
        assert_eq!(
            report["probes"][0]["argv"],
            serde_json::json!([target.to_str().unwrap(), "--help"])
        );
        assert_eq!(report["probes"][0]["status"], "ok");
        assert_eq!(
            report["cli_surfaces"]["root"]["flags"],
            serde_json::json!(["--trusted"])
        );
        assert_eq!(snapshot(&caller.join("workspace")), before);
    }
}
