//! Private replay of independently captured streams, never an executable oracle.
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

// An independent opaque-number JSON canonicalizer. Authority numbers never enter
// serde_json or a production parser/encoder, including the 100-digit endpoint stats.
#[derive(Clone)]
enum Token {
    Atom(String),
    Object(BTreeMap<String, Token>),
    Array(Vec<Token>),
}
impl Token {
    fn parse(raw: &str) -> Self {
        fn ws(raw: &[u8], at: &mut usize) {
            while raw.get(*at).is_some_and(u8::is_ascii_whitespace) {
                *at += 1;
            }
        }
        fn string(raw: &[u8], at: &mut usize) -> String {
            let start = *at;
            assert_eq!(raw[*at], b'"');
            *at += 1;
            loop {
                let byte = raw[*at];
                *at += 1;
                match byte {
                    b'\\' => *at += 1,
                    b'"' => break,
                    _ => (),
                }
            }
            serde_json::from_slice(&raw[start..*at]).unwrap()
        }
        fn walk(raw: &[u8], at: &mut usize) -> Token {
            ws(raw, at);
            match raw[*at] {
                b'{' => {
                    *at += 1;
                    let mut fields = BTreeMap::new();
                    ws(raw, at);
                    while raw[*at] != b'}' {
                        let key = string(raw, at);
                        ws(raw, at);
                        assert_eq!(raw[*at], b':');
                        *at += 1;
                        assert!(fields.insert(key, walk(raw, at)).is_none());
                        ws(raw, at);
                        if raw[*at] != b'}' {
                            assert_eq!(raw[*at], b',');
                            *at += 1;
                            ws(raw, at);
                        }
                    }
                    *at += 1;
                    Token::Object(fields)
                }
                b'[' => {
                    *at += 1;
                    let mut items = Vec::new();
                    ws(raw, at);
                    while raw[*at] != b']' {
                        items.push(walk(raw, at));
                        ws(raw, at);
                        if raw[*at] != b']' {
                            assert_eq!(raw[*at], b',');
                            *at += 1;
                        }
                    }
                    *at += 1;
                    Token::Array(items)
                }
                b'"' => Token::Atom(serde_json::to_string(&string(raw, at)).unwrap()),
                _ => {
                    let start = *at;
                    while raw
                        .get(*at)
                        .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                    {
                        *at += 1;
                    }
                    assert!(*at > start);
                    Token::Atom(std::str::from_utf8(&raw[start..*at]).unwrap().into())
                }
            }
        }
        let mut at = 0;
        let result = walk(raw.as_bytes(), &mut at);
        assert!(raw[at..].trim().is_empty());
        result
    }
    fn compact(&self) -> String {
        match self {
            Self::Atom(raw) => raw.clone(),
            Self::Object(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap(),
                            value.compact()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Array(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Self::compact)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

fn revision(raw: &str) -> (String, String) {
    let Token::Object(state) = Token::parse(raw) else {
        panic!("authority object")
    };
    let canonical = Token::Object(
        ["workspace_id", "content_revision", "groups"]
            .into_iter()
            .map(|key| (key.to_owned(), state[key].clone()))
            .collect(),
    )
    .compact();
    let workspace: String = serde_json::from_str(&state["workspace_id"].compact()).unwrap();
    let counter = state["content_revision"].compact();
    assert!(counter.bytes().all(|b| b.is_ascii_digit()));
    let mut domain = b"coven.groups.content:v1\0".to_vec();
    domain.extend(canonical.as_bytes());
    (
        format!("groups-v1:{workspace}:{counter}:{}", digest(&domain)),
        canonical,
    )
}

fn participant_binding(raw: &str, participant: &str, copies: usize) -> String {
    let bytes = participant.as_bytes();
    assert_eq!(bytes.len(), 36);
    for (i, byte) in bytes.iter().enumerate() {
        if [8, 13, 18, 23].contains(&i) {
            assert_eq!(*byte, b'-');
        } else {
            assert!(byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
        }
    }
    assert_eq!(bytes[14], b'4');
    assert!(b"89ab".contains(&bytes[19]));
    let old = "\"00000004-1111-4111-8111-111111111111\"";
    assert_eq!(raw.matches(old).count(), copies);
    raw.replace(old, &serde_json::to_string(participant).unwrap())
}

pub struct Corpus {
    data: Json,
}
impl Corpus {
    pub fn new() -> Self {
        let raw = include_bytes!("../../../tests/retirement/fixtures/source_live_observation.json");
        let proof_raw = include_bytes!(
            "../../../tests/retirement/fixtures/source_live_observation_provenance.json"
        );
        assert_eq!(
            digest(raw),
            "110009c29e84822210b2e656e23b619078a4a02c882f40fc90b01fd2a8fd230f"
        );
        assert_eq!(
            digest(proof_raw),
            "b24e5b8fefbc7ebcf766413611000fd4c9ddc9798b31572025e4d8fe29dfce9b"
        );
        let data: Json = serde_json::from_slice(raw).unwrap();
        let proof: Json = serde_json::from_slice(proof_raw).unwrap();
        assert_eq!(data["schema_version"], 1);
        assert_eq!(proof["schema_version"], 1);
        assert_eq!(
            proof["historical_commit"],
            "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
        );
        assert_eq!(proof["corpus_sha256"], digest(raw));
        assert_eq!(proof["case_count"], 63);
        assert_eq!(proof["repeat_capture_equal"], true);
        assert_eq!(proof["interpreter"]["isolated"], true);
        assert_eq!(proof["interpreter"]["bytecode"], false);
        assert!(
            proof["interpreter"]["version"]
                .as_str()
                .unwrap()
                .starts_with("3.14.8 ")
        );
        let pins = proof["source_sha256"].as_object().unwrap();
        assert_eq!(pins.len(), 87);
        assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
        for caller in [
            "group_live",
            "djinn_live_observation",
            "kitsune_observation",
        ] {
            assert!(pins.contains_key(&format!("src/{caller}/tests.rs")));
        }
        let rows = data["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 63);
        let mut ids = Vec::new();
        for i in 0..16 {
            ids.push(format!("djinn-{i:02}"));
        }
        ids.extend(["djinn-mode-600", "djinn-mode-640"].map(str::to_owned));
        for huge in 0..2 {
            ids.extend([
                format!("seed-{huge}"),
                format!("predecessor-{huge}"),
                format!("attach-{huge}-0"),
                format!("attach-{huge}-1"),
            ]);
            if huge == 0 {
                ids.push("group-status".into());
            }
        }
        for i in 0..16 {
            ids.push(format!("kitsune-{i:02}"));
        }
        for parent in ["real", "alias"] {
            for leaf in [
                "missing.sock",
                "regular",
                "leaf-link",
                "unsafe.sock",
                "missing",
            ] {
                for mode in 0..2 {
                    ids.push(format!("endpoint-{parent}-{leaf}-{mode}"));
                }
            }
        }
        assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
        assert_eq!(proof["case_ids"], json!(ids));
        for (row, id) in rows.iter().zip(ids) {
            assert_eq!(row["id"], id);
            let authority = row.get("raw_utf8").is_some();
            let mut keys = vec![
                "id",
                "input_sha256",
                "input_utf8",
                "stdout_sha256",
                "stdout_utf8",
            ];
            if authority {
                keys.extend([
                    "canonical_sha256",
                    "canonical_utf8",
                    "raw_sha256",
                    "raw_utf8",
                    "revision",
                ]);
            }
            keys.sort_unstable();
            assert_eq!(
                row.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                keys
            );
            for (stream, hash) in [
                ("input_utf8", "input_sha256"),
                ("stdout_utf8", "stdout_sha256"),
            ] {
                assert_eq!(row[hash], digest(row[stream].as_str().unwrap().as_bytes()));
            }
            if authority {
                let raw = row["raw_utf8"].as_str().unwrap();
                assert_eq!(row["raw_sha256"], digest(raw.as_bytes()));
                let (revision, canonical) = revision(raw);
                assert_eq!(row["revision"], revision);
                assert_eq!(row["canonical_utf8"], canonical);
                assert_eq!(row["canonical_sha256"], digest(canonical.as_bytes()));
            }
        }
        Self { data }
    }
    pub fn root(&self) -> &str {
        self.data["capture_root"].as_str().unwrap()
    }
    pub fn row(&self, id: &str) -> &Json {
        self.data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
    }
    pub fn check(&self, id: &str, input: Json) {
        let row = self.row(id);
        let captured: Json = serde_json::from_str(row["input_utf8"].as_str().unwrap()).unwrap();
        assert_eq!(captured, input, "independent input inventory: {id}");
    }
    pub fn expected(&self, id: &str) -> Json {
        serde_json::from_str(self.row(id)["stdout_utf8"].as_str().unwrap()).unwrap()
    }
    pub fn seed(&self, huge: bool, predecessor: bool) -> Vec<u8> {
        let id = format!(
            "{}-{}",
            if predecessor { "predecessor" } else { "seed" },
            u8::from(huge)
        );
        self.check(&id, json!({"huge":huge}));
        self.row(&id)["raw_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec()
    }
    pub fn predecessor(&self, huge: bool, participant: &str) -> Vec<u8> {
        let seed = self.seed(huge, true);
        participant_binding(std::str::from_utf8(&seed).unwrap(), participant, 1).into_bytes()
    }
    // Bind only artifact_path and the pre-attach participant input UUID. Binding
    // order/stats, other identities, counters and timestamps remain captured literals.
    pub fn group(&self, id: &str, file: &str, participant: Option<&str>) -> (Vec<u8>, Vec<u8>) {
        use std::{os::unix::fs::MetadataExt, path::Path};
        let path = Path::new(file);
        assert_eq!(path.canonicalize().unwrap().as_path(), path);
        assert_eq!(
            path.file_name().unwrap(),
            "616fed3e-ae7f-41a0-99e5-80546d659d76.json"
        );
        let info = std::fs::symlink_metadata(path).unwrap();
        assert!(info.is_file());
        assert_eq!(info.mode() & 0o7777, 0o600);
        assert_eq!(info.uid(), unsafe { libc::getuid() });
        assert_eq!(info.nlink(), 1);
        let row = self.row(id);
        let old = format!("{}/616fed3e-ae7f-41a0-99e5-80546d659d76.json", self.root());
        let old = serde_json::to_string(&old).unwrap();
        let new = serde_json::to_string(file).unwrap();
        let raw = row["raw_utf8"].as_str().unwrap();
        assert_eq!(raw.matches(&old).count(), 1);
        assert_eq!(
            row["stdout_utf8"].as_str().unwrap().matches(&old).count(),
            if participant.is_some() { 2 } else { 1 }
        );
        let mut raw = raw.replace(&old, &new);
        let mut output = row["stdout_utf8"].as_str().unwrap().replace(&old, &new);
        if let Some(participant) = participant {
            assert!(id.starts_with("attach-"));
            raw = participant_binding(&raw, participant, 1);
            output = participant_binding(&output, participant, 2);
        } else {
            assert_eq!(id, "group-status");
        }
        let (new_revision, _) = revision(&raw);
        let old_revision = row["revision"].as_str().unwrap();
        assert_eq!(output.matches(old_revision).count(), 1);
        (
            output.replace(old_revision, &new_revision).into_bytes(),
            raw.into_bytes(),
        )
    }
}
