use coven::{
    records,
    values::{self, Profile},
};

#[test]
fn typed_records_keep_extensions_status_and_absent_fields() {
    let raw =
        br#"[{"id":"_","title":"","status":"anything","large":18446744073709551616,"float":1.0}]"#;
    let value = values::parse(raw, Profile::LegacySnapshot).unwrap();
    let records = records::TaskRecords::validate(value).unwrap();
    assert_eq!(records.value().compact().unwrap(), raw);
    for raw in [
        br#"[{"id":"a","title":"x","status":"x","notes":null}]"#.as_slice(),
        br#"[{"id":"a","title":"x","status":"x"},{"id":"a","title":"y","status":"y"}]"#,
    ] {
        assert!(
            records::TaskRecords::validate(values::parse(raw, Profile::LegacySnapshot).unwrap())
                .is_err()
        );
    }
}

#[test]
#[cfg(unix)]
fn snapshot_cli_preflight_authority_and_no_write_parity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = std::process::Command::new("python3")
        .arg(root.join("tests/_task_snapshot_parity.py"))
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_coven-rs"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("COVEN_MULTIPLEXER", "")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
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
