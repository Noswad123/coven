use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn legacy_record_oracle_closed_schema_and_integer_preservation() {
    let authority = fixture();
    let original = fixture_value(&authority);
    let mut corpus = vec![String::from_utf8(authority.bytes.clone()).unwrap()];
    for (key, value) in [
        ("schema_version", json!(true)),
        ("schema_version", json!(1.0)),
        ("extra", json!(1)),
        ("object_format", json!("sha512")),
        ("registered_at", json!("2026-02-30T12:00:00Z")),
        ("registered_at", json!("2026-11-02T12:00:00+01:00")),
        ("registered_at", json!("2026-11-02T12:00:00Z")),
        ("registered_at", json!("2026-11-02T12:00:00xZ")),
        ("registered_at", json!("2026-11-02T12:00:00.123xZ")),
        ("registered_at", json!("2026-11-02T12:00:00Z\0junk")),
        ("registered_at", json!("2026-11-02T12x+00:00")),
        ("canonical_root", json!("/a//b")),
        ("canonical_root", json!("relative")),
    ] {
        let mut record = original.clone();
        record[key] = value;
        corpus.push(serde_json::to_string(&record).unwrap());
    }
    for value in [
        json!(true),
        json!(1.0),
        json!(-1),
        json!(0),
        json!(u64::MAX),
    ] {
        let mut record = original.clone();
        record["filesystem"]["root"]["inode"] = value;
        corpus.push(serde_json::to_string(&record).unwrap());
    }
    let raw = corpus[0].replace(
        "\"inode\":2",
        "\"inode\":18446744073709551616000000000000000000000000001",
    );
    corpus.push(raw.clone());
    for path in [
        "/deleted",
        "//deleted",
        "///deleted",
        "/deleted/.",
        "/deleted//",
        "/a/../deleted",
    ] {
        let mut value = original.clone();
        value["original_request"]["path"] = json!(path);
        corpus.push(serde_json::to_string(&value).unwrap());
        let mut value = original.clone();
        value["canonical_root"] = json!(path);
        corpus.push(serde_json::to_string(&value).unwrap());
    }
    for date in [
        "2026-11-02",
        "20261102",
        "2026-W45-1",
        "2026W451",
        "2026-W45",
        "2026W45",
        "2026-02-30",
        "0000-12-31",
        "9999-12-31",
    ] {
        for separator in ["T", " ", "x", "\u{1f98a}"] {
            for clock in [
                "12",
                "1200",
                "12:00",
                "12:00:00",
                "120000",
                "12:00:00.123456789",
                "24:00:00",
                "25:00:00",
                "12:00:00.0",
            ] {
                for zone in ["Z", "+00:00", "-0000", "+00:00:00.5", "+01:00", ""] {
                    let mut value = original.clone();
                    value["registered_at"] = json!(format!("{date}{separator}{clock}{zone}"));
                    corpus.push(serde_json::to_string(&value).unwrap());
                }
            }
        }
    }
    corpus.extend([
        "null".to_owned(),
        "{}".to_owned(),
        "{\"x\":1,\"x\":2}".to_owned(),
    ]);
    let mut child=Command::new("python3").args(["-B","-c","import sys; sys.path.insert(0,sys.argv[1]); exec(compile(open(sys.argv[2], 'rb').read(),sys.argv[2], 'exec'))",
        env!("CARGO_MANIFEST_DIR"),concat!(env!("CARGO_MANIFEST_DIR"),"/src/repositories/oracle.py")])
        .env("PYTHONDONTWRITEBYTECODE","1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&corpus).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let expected: Vec<bool> = serde_json::from_slice(&output.stdout).unwrap();
    for (raw, expected) in corpus.into_iter().zip(expected) {
        assert_eq!(
            record::Authority::decode(raw.as_bytes().to_vec()).is_ok(),
            expected,
            "{raw}"
        );
    }
    let root = workspace();
    fs::write(root.path().join(AUTHORITY), raw.as_bytes()).unwrap();
    let output = inspect_with(root.path(), false, true, &Never).unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(
        output
            .stdout
            .contains("18446744073709551616000000000000000000000000001")
    );
    let big = record::Authority::decode(raw.into_bytes()).unwrap();
    let replay = registration::register(
        root.path(),
        big.record.original_request.clone(),
        true,
        &Never,
        &registration::SystemIo,
    )
    .unwrap();
    assert_eq!(replay.exit_code, 0);
    assert!(
        replay
            .stdout
            .contains("18446744073709551616000000000000000000000000001")
    );
}
