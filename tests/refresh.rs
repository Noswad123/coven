#[test]
#[cfg(unix)]
fn native_refresh_matches_python_and_shares_its_lock() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python3")
        .arg(root.join("tests/_refresh_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .env_remove("COVEN_MULTIPLEXER")
        .current_dir(root)
        .output()
        .expect("run bounded refresh CLI oracle");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}
