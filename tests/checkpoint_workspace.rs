//! Linux filesystem/permission oracle, separate from pure checkpoint grammar tests.
#[test]
#[cfg(target_os = "linux")]
fn checkpoint_workspace_cli_parity_and_no_effects() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python3")
        .arg(root.join("tests/_checkpoint_workspace_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("COVEN_MULTIPLEXER", "")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}
