//! Full historical stdin/stdout bytes for both original engine oracle consumers.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MODES: [&str; 8] = [
    "success",
    "reservation_error",
    "reservation_uncertain",
    "invalid_reservation",
    "reject",
    "lost_ack",
    "unsent",
    "unknown",
];

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn keys(value: &Value, expected: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected
    );
}

fn frame(stream: &mut Vec<u8>, raw: &[u8]) {
    stream.extend_from_slice(&u64::try_from(raw.len()).unwrap().to_be_bytes());
    stream.extend_from_slice(raw);
}

pub(super) struct Cases(Vec<Value>);
impl Cases {
    pub(super) fn new() -> Self {
        if std::env::var_os("COVEN_GROUP_PUBLISHER_ENGINE_ASSERT_PYTHON_ABSENT").is_some() {
            for path in [
                "/usr/bin/python3",
                "/bin/python3",
                "/usr/bin/python3.14",
                "/usr/local/bin/python3",
                "/home/linuxbrew/.linuxbrew/bin/python3",
                "/usr/lib/python3.14",
                "/usr/lib/libpython3.14.so",
                "/usr/lib/libpython3.14.so.1.0",
            ] {
                let error = std::fs::symlink_metadata(path).unwrap_err();
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{path}");
                assert_eq!(error.raw_os_error(), Some(libc::ENOENT), "{path}");
            }
        }
        let raw = include_bytes!("../../../tests/retirement/fixtures/group_publisher_engine.json");
        let proof = include_bytes!(
            "../../../tests/retirement/fixtures/group_publisher_engine_provenance.json"
        );
        assert_eq!(
            digest(raw),
            "8087e14b3e9c76660ec639260c74f5021d7f02ccfcaf950aeb0953c8b0759ec8"
        );
        assert_eq!(
            digest(proof),
            "ae7638134f1720a5e91bb1f4f10a4a17ec05ea612178e33e65ceb118c13c937b"
        );
        // Only the outer envelopes are decoded. Expected stdout stays opaque UTF-8.
        let data: Value = serde_json::from_slice(raw).unwrap();
        let proof: Value = serde_json::from_slice(proof).unwrap();
        keys(&data, &["cases", "schema_version"]);
        keys(
            &proof,
            &[
                "capture_tool_sha256",
                "case_count",
                "construction",
                "corpus_sha256",
                "dynamic_bindings",
                "environment",
                "helper_invocations",
                "historical_commit",
                "interpreter",
                "matrix",
                "repeat_capture_equal",
                "schema_version",
                "source_sha256",
                "stdin_stream_bytes",
                "stdin_stream_hex",
                "stdin_stream_sha256",
                "stdout_stream_bytes",
                "stdout_stream_hex",
                "stdout_stream_sha256",
                "stream_framing",
            ],
        );
        assert_eq!(data["schema_version"], 1);
        assert_eq!(proof["schema_version"], 1);
        assert_eq!(
            proof["historical_commit"],
            "727edc7aad55ed76e98670e0cee6449d611b2285"
        );
        assert_eq!(proof["corpus_sha256"], digest(raw));
        assert_eq!(
            proof["capture_tool_sha256"],
            "8686386fd183742831d27758a3ea6b57857e728ff91b343c78d88856815440ff"
        );
        assert_eq!(proof["case_count"], 16);
        assert_eq!(proof["helper_invocations"], 32);
        assert_eq!(proof["dynamic_bindings"], 0);
        assert_eq!(proof["repeat_capture_equal"], true);
        assert_eq!(proof["matrix"], json!({"live":[false,true],"modes":MODES}));
        assert_eq!(
            proof["environment"],
            json!({"LANG":"C.UTF-8", "LC_ALL":"C.UTF-8",
            "PATH":"/usr/bin:/bin", "TMPDIR":"<owned output directory>/tmp"})
        );
        keys(
            &proof["interpreter"],
            &["bytecode", "executable", "isolated", "sha256", "version"],
        );
        assert_eq!(proof["interpreter"]["bytecode"], false);
        assert_eq!(proof["interpreter"]["isolated"], true);
        assert_eq!(proof["interpreter"]["executable"], "/usr/bin/python3");
        assert!(
            proof["interpreter"]["version"]
                .as_str()
                .unwrap()
                .starts_with("3.14.8 ")
        );
        let pins = proof["source_sha256"].as_object().unwrap();
        assert_eq!(pins.len(), 53);
        assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
        for hash in pins.values().chain([&proof["interpreter"]["sha256"]]) {
            let hash = hash.as_str().unwrap().as_bytes();
            assert_eq!(hash.len(), 64);
            assert!(
                hash.iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            );
        }
        for path in [
            "src/group_publisher/oracle.py",
            "src/group_publisher/tests.rs",
            "src/group_publisher/live_tests.rs",
            "src/group_publisher/model.rs",
            "src/group_publisher/engine.rs",
            "src/group_publisher/native.rs",
            "src/groups/store.rs",
            "src/kitsune_observation/bindings.rs",
            "src/values/encode.rs",
        ] {
            assert!(pins.contains_key(path));
        }
        let rows = data["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let mut stdin = Vec::new();
        let mut stdout = Vec::new();
        for i in 0..16 {
            let row = &rows[i];
            keys(
                row,
                &[
                    "id",
                    "input_sha256",
                    "input_utf8",
                    "live",
                    "mode",
                    "stdout",
                    "stdout_sha256",
                ],
            );
            let live = i >= 8;
            let mode = MODES[i % 8];
            assert_eq!(
                row["id"],
                format!("{}-{mode}", if live { "live" } else { "static" })
            );
            assert_eq!(row["live"], live);
            assert_eq!(row["mode"], mode);
            let input = row["input_utf8"].as_str().unwrap().as_bytes();
            let output = row["stdout"].as_str().unwrap().as_bytes();
            assert_eq!(row["input_sha256"], digest(input));
            assert_eq!(row["stdout_sha256"], digest(output));
            let request: Value = serde_json::from_slice(input).unwrap();
            keys(&request, &["fresh", "group", "live", "mode", "status"]);
            assert_eq!(request["live"], live);
            assert_eq!(request["mode"], mode);
            assert_eq!(request["group"]["members"].as_array().unwrap().len(), 2);
            assert_eq!(
                request["status"]["participants"].as_array().unwrap().len(),
                2
            );
            assert_eq!(request["fresh"]["agents"].as_array().unwrap().len(), 2);
            assert_eq!(request["fresh"]["panes"].as_array().unwrap().len(), 2);
            frame(&mut stdin, input);
            frame(&mut stdout, output);
        }
        for (name, stream) in [("stdin", stdin), ("stdout", stdout)] {
            assert_eq!(proof[format!("{name}_stream_bytes")], stream.len());
            assert_eq!(proof[format!("{name}_stream_sha256")], digest(&stream));
            let hex: String = stream.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(proof[format!("{name}_stream_hex")], hex);
        }
        Self(rows.clone())
    }

    pub(super) fn expected(&self, live: bool, mode: &str, input: &[u8]) -> Vec<u8> {
        let i = usize::from(live) * 8 + MODES.iter().position(|m| *m == mode).unwrap();
        let row = &self.0[i];
        assert_eq!(
            input,
            row["input_utf8"].as_str().unwrap().as_bytes(),
            "complete original input before native execution, mode={mode} live={live}"
        );
        row["stdout"].as_str().unwrap().as_bytes().to_vec()
    }
}
