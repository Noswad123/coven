use super::*;
use sha2::{Digest, Sha256};

pub(super) fn rows() -> Vec<Value> {
    // The strong sandbox requests syscall-level absence checks, not just an empty PATH.
    if std::env::var_os("COVEN_HISTORY_ASSERT_PYTHON_ABSENT").is_some() {
        for path in [
            "/usr/bin/python3",
            "/bin/python3",
            "/usr/bin/python3.14",
            "/usr/local/bin/python3",
            "/home/linuxbrew/.linuxbrew/bin/python3",
            "/usr/lib/python3.14",
            "/usr/lib/libpython3.14.so",
        ] {
            let error = fs::symlink_metadata(path).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{path}");
            #[cfg(target_os = "linux")]
            assert_eq!(error.raw_os_error(), Some(libc::ENOENT), "{path}");
        }
    }
    let corpus = include_bytes!("../../../../tests/retirement/fixtures/history_rendering.json");
    let provenance =
        include_bytes!("../../../../tests/retirement/fixtures/history_rendering_provenance.json");
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let corpus_hash = digest(corpus);
    assert_eq!(
        corpus_hash,
        "150ef31104ea3158a48c53aa2a804c54ef401ea3611cb630a4dee7099fae5e31"
    );
    assert_eq!(
        digest(provenance),
        "9aca74fc0b4a34b61282817ebe7f941bbff1e6fd74793fb1f2368f594c82f78a"
    );
    // serde only unwraps the envelope of strings, never a record or its numbers.
    let data: serde_json::Value = serde_json::from_slice(corpus).unwrap();
    let proof: serde_json::Value = serde_json::from_slice(provenance).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(
        proof["historical_commit"],
        "ca3b19d87c955d6415231b1c4ebbbc591195a105"
    );
    assert_eq!(proof["corpus_sha256"], corpus_hash);
    assert_eq!(proof["repeat_capture_equal"], true);
    assert_eq!(proof["record_rows"], 225);
    assert_eq!(proof["raw_rows"], 7);
    assert_eq!(proof["pretty_outputs"], 450);
    assert_eq!(proof["raw_outputs"], 7);
    let pins = proof["source_sha256"].as_object().unwrap();
    assert_eq!(pins.len(), 28);
    assert_eq!(
        pins.keys()
            .filter(|p| p.starts_with("runner/") && p.ends_with(".py"))
            .count(),
        25
    );
    let lines = data["jsonl_lines"].as_array().unwrap();
    let hashes = proof["row_sha256"].as_array().unwrap();
    assert_eq!(lines.len(), 232);
    assert_eq!(hashes.len(), 232);
    let mut stdout = String::new();
    let mut rows = Vec::new();
    for i in 0..232 {
        let line = lines[i].as_str().unwrap();
        assert!(line.is_ascii() && line.ends_with('\n'));
        assert_eq!(line.bytes().filter(|b| *b == b'\n').count(), 1);
        assert_eq!(digest(line.as_bytes()), hashes[i], "case {i}");
        stdout.push_str(line);
        let row = values::parse(line.as_bytes(), values::Profile::LegacySnapshot).unwrap();
        let object = row.as_object().unwrap();
        let keys: Vec<_> = object
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        if i < 225 {
            assert_eq!(keys, ["record", "events", "messages"]);
            assert!(object.get("record").unwrap().as_object().is_some());
            for key in ["events", "messages"] {
                assert!(object.get(key).unwrap().as_str().is_some());
            }
        } else {
            assert_eq!(keys, ["raw", "text"]);
            assert!(matches!(
                object.get("raw").unwrap(),
                Value::Null | Value::String(_)
            ));
            assert!(object.get("text").unwrap().as_str().is_some());
        }
        rows.push(row);
    }
    assert_eq!(stdout.len(), 42_199);
    assert_eq!(proof["stdout_bytes"], stdout.len());
    assert_eq!(digest(stdout.as_bytes()), proof["stdout_sha256"]);
    rows
}
