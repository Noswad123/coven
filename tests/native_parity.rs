#[test]
#[cfg(unix)]
fn python_rust_read_only_evidence_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python3")
        .arg(root.join("tests/_rust_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven-rs"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(root)
        .output()
        .expect("Python compatibility oracle must be available during migration");
    assert!(
        result.status.success(),
        "oracle failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}
