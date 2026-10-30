#[test]
#[cfg(unix)]
fn python_ledger_authority_corruption_and_projection_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("tests/_ledger_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven-rs"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
