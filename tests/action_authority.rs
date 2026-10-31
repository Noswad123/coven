#[test]
#[cfg(unix)]
fn python_action_authority_inspection_and_refusal_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("tests/_action_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env("PYTHONUTF8", "1")
        .current_dir(root)
        .output()
        .expect("run Python action authority oracle");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
