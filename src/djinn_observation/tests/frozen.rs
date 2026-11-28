use super::*;
use sha2::{Digest, Sha256};

pub(super) fn expected(payloads: &[String], folder: &str) -> Vec<Json> {
    if std::env::var_os("COVEN_DJINN_ASSERT_PYTHON_ABSENT").is_some() {
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
        // These are prerequisites for the unchanged controlled supervisor test.
        assert!(Path::new("/bin/sh").is_file());
        assert!(
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("coven")
                .is_file()
        );
    }
    let corpus = include_bytes!("../../../tests/retirement/fixtures/djinn_decoder.json");
    let provenance =
        include_bytes!("../../../tests/retirement/fixtures/djinn_decoder_provenance.json");
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        digest(corpus),
        "7d207f3e6bb75b9f7281ebaeb1dc4b6c4c56595ac0bab41f2b4908b53dfa3a0c"
    );
    assert_eq!(
        digest(provenance),
        "22e1615d729d1808b0d384a1f56fe33a59723ee98fb74acf6ceb4eba8109344d"
    );
    // Only unwrap raw input strings and bounded, already-projected expectations.
    // Never parse an input payload as serde Value (notably the 4300-digit int).
    let data: Json = serde_json::from_slice(corpus).unwrap();
    let proof: Json = serde_json::from_slice(provenance).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(proof["corpus_sha256"], digest(corpus));
    assert_eq!(
        proof["historical_commit"],
        "727edc7aad55ed76e98670e0cee6449d611b2285"
    );
    assert_eq!(proof["repeat_capture_equal"], true);
    assert_eq!(proof["case_count"], 161);
    let pins = proof["source_sha256"].as_object().unwrap();
    assert_eq!(pins.len(), 30);
    assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
    let request = proof["request_utf8"].as_str().unwrap();
    let stdout = proof["stdout_utf8"].as_str().unwrap();
    assert_eq!(
        request.len(),
        proof["request_bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(digest(request.as_bytes()), proof["request_sha256"]);
    assert_eq!(
        stdout.len(),
        proof["stdout_bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(digest(stdout.as_bytes()), proof["stdout_sha256"]);
    let requests: Vec<Json> = serde_json::from_str(request).unwrap();
    let captured: Vec<Json> = serde_json::from_str(stdout).unwrap();
    let rows = data["cases"].as_array().unwrap();
    assert_eq!(payloads.len(), 161);
    assert_eq!(rows.len(), 161);
    assert_eq!(requests.len(), 161);
    assert_eq!(captured.len(), 161);
    let mut expected = Vec::new();
    let mut tally = std::collections::BTreeMap::<String, usize>::new();
    // Validate the entire input inventory before dispatching any native decode.
    for i in 0..161 {
        let row = &rows[i];
        let keys: Vec<_> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["expected", "folder", "id", "payload", "payload_sha256"]
        );
        assert_eq!(row["id"], i);
        assert_eq!(row["folder"], folder);
        assert_eq!(
            row["payload"].as_str().unwrap().as_bytes(),
            payloads[i].as_bytes(),
            "input case {i}"
        );
        assert_eq!(row["payload_sha256"], digest(payloads[i].as_bytes()));
        assert_eq!(requests[i], json!({"payload":payloads[i],"folder":folder}));
        let result = &row["expected"];
        assert_eq!(result, &captured[i]);
        let object = result.as_object().unwrap();
        let keys: Vec<_> = object.keys().map(String::as_str).collect();
        let outcome = if let Some(error) = object.get("error") {
            assert_eq!(keys, ["error"]);
            let code = error.as_str().unwrap();
            assert!(matches!(
                code,
                "schema_error"
                    | "unsupported_contract"
                    | "invalid_json"
                    | "duplicate_key"
                    | "nonfinite_json"
                    | "nesting_limit"
            ));
            code
        } else {
            assert_eq!(keys, ["assessment", "observation", "producer"]);
            let assessment = result["assessment"].as_str().unwrap();
            match assessment {
                "observed" => assert!(result["observation"].is_object()),
                "unavailable" => assert!(result["observation"].is_null()),
                _ => panic!("unexpected assessment"),
            }
            assert!(result["producer"].is_object());
            assessment
        };
        *tally.entry(outcome.to_owned()).or_default() += 1;
        expected.push(result.clone());
    }
    let counts = json!({"observed":23,"unavailable":1,"schema_error":127,
        "unsupported_contract":2,"invalid_json":2,"duplicate_key":2,
        "nonfinite_json":3,"nesting_limit":1});
    assert_eq!(serde_json::to_value(tally).unwrap(), counts);
    assert_eq!(proof["outcome_counts"], counts);
    expected
}
