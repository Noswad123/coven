//! Temporary parent-side Python differential oracle. Every acceptance command is
//! the Cargo-built native binary; Python is never on the native child's PATH.
#[test]
#[cfg(unix)]
fn native_group_observation_workflow() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(root.join("tests/_group_observation_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(root)
        .output()
        .expect("temporary parent-side group oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
