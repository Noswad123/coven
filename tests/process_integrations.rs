//! Public CLI acceptance. Private NativeRunner/supervisor fault gates live with
//! their owner; this launches the actual Cargo binary, never an installed coven.
#[test]
#[cfg(unix)]
fn temporary_python_process_integrations_cli_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(root.join("tests/_process_integrations_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .current_dir(root)
        .output()
        .expect("temporary Python oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
