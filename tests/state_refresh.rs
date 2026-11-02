//! CLI acceptance corpus. Private publication/confirmation faults stay in SDK units.
#[test]
#[cfg(unix)]
fn state_commands_and_existing_workspace_refresh_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("tests/_state_refresh_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .current_dir(root)
        .output()
        .expect("start state/refresh parity corpus");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
