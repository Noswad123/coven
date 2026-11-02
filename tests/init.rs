//! Real CLI init acceptance. Private fault injection belongs to service/bootstrap units.
#[test]
#[cfg(unix)]
fn initial_workspace_cli_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("tests/_init_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .current_dir(root)
        .output()
        .expect("start init parity corpus");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
