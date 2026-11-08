//! Acceptance uses the Cargo binary and compiled inert backends, never installed runtimes.
#[test]
#[cfg(unix)]
fn runtime_launch_workflow_and_python_oracle() {
    use std::process::{Command, Stdio};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = tempfile::tempdir().expect("fixture binary directory");
    let fixture = temporary.path().join("runtime-backend-fixture");
    let compiled = Command::new("rustc")
        .args(["--edition=2024", "-O"])
        .arg(root.join("tests/support/runtime_backend_fixture.rs"))
        .arg("-o")
        .arg(&fixture)
        .output()
        .expect("compile inert runtime fixture");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let output = Command::new("python3")
        .arg("-B")
        .arg(root.join("tests/_runtime_launch_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .arg("--fixture")
        .arg(fixture)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .stdin(Stdio::null())
        .current_dir(root)
        .output()
        .expect("run isolated runtime launch acceptance oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
