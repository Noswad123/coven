//! Isolated legacy acceptance: Python is only the development oracle/driver.
#[test]
#[cfg(unix)]
fn legacy_lifecycle_workflow_and_python_oracle() {
    use std::process::{Command, Stdio};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = tempfile::tempdir().expect("fixture directory");
    let fixture = temporary.path().join("legacy-backend-fixture");
    let compiled = Command::new("rustc")
        .args(["--edition=2024", "-O"])
        .arg(root.join("tests/support/legacy_backend_fixture.rs"))
        .arg("-o")
        .arg(&fixture)
        .output()
        .expect("compile std-only inert backend");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let output = Command::new("python3")
        .arg("-B")
        .arg(root.join("tests/_legacy_lifecycle_parity.py"))
        .args(["--binary", env!("CARGO_BIN_EXE_coven"), "--fixture"])
        .arg(fixture)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .current_dir(root)
        .output()
        .expect("run lifecycle acceptance");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
