#[test]
#[cfg(unix)]
fn python_rust_read_only_evidence_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python3")
        .arg(root.join("tests/_rust_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven"))
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

#[test]
fn native_build_uses_the_coven_command_identity() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_coven"));
    assert_eq!(
        binary.file_name().unwrap().to_str().unwrap(),
        format!("coven{}", std::env::consts::EXE_SUFFIX)
    );
    let help = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = std::str::from_utf8(&help.stdout).unwrap();
    assert!(help.contains("Usage: coven <COMMAND>"), "{help}");
    let version = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert!(version.stderr.is_empty());
    assert_eq!(
        std::str::from_utf8(&version.stdout).unwrap(),
        concat!("coven ", env!("CARGO_PKG_VERSION"), "\n")
    );
}
