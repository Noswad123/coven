//! Opaque historical storage with only the fifteen reviewed UUID token bindings.
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ops::Range};

const PATHS: [&str; 3] = [
    "/workspace_id",
    "/groups/0/group_id",
    "/groups/0/members/0/participant_id",
];
const UUIDS: [&str; 3] = [
    "11111111-1111-4111-8111-111111111111",
    "22222222-2222-4222-8222-222222222222",
    "33333333-3333-4333-8333-333333333333",
];
const RANGES: [Range<usize>; 3] = [43..81, 146..184, 284..322];

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn keys(value: &Json, expected: &[&str]) {
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

// Independent byte scanner: no production parser, canonicalizer or encoder.
fn spans(raw: &[u8]) -> BTreeMap<String, Range<usize>> {
    fn ws(raw: &[u8], at: &mut usize) {
        while raw.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    }
    fn string(raw: &[u8], at: &mut usize) {
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
    }
    fn walk(raw: &[u8], at: &mut usize, path: String, leaves: &mut BTreeMap<String, Range<usize>>) {
        ws(raw, at);
        let start = *at;
        match raw[*at] {
            b'{' | b'[' => {
                let object = raw[*at] == b'{';
                let close = if object { b'}' } else { b']' };
                *at += 1;
                ws(raw, at);
                let mut index = 0;
                while raw[*at] != close {
                    let key = if object {
                        let begin = *at;
                        string(raw, at);
                        let key: String = serde_json::from_slice(&raw[begin..*at]).unwrap();
                        assert!(!key.contains(['/', '~']));
                        ws(raw, at);
                        assert_eq!(raw[*at], b':');
                        *at += 1;
                        key
                    } else {
                        index.to_string()
                    };
                    walk(raw, at, format!("{path}/{key}"), leaves);
                    index += 1;
                    ws(raw, at);
                    if raw[*at] != close {
                        assert_eq!(raw[*at], b',');
                        *at += 1;
                        ws(raw, at);
                    }
                }
                *at += 1;
            }
            _ => {
                if raw[*at] == b'"' {
                    string(raw, at);
                } else {
                    while raw
                        .get(*at)
                        .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                    {
                        *at += 1;
                    }
                }
                assert!(leaves.insert(path, start..*at).is_none());
            }
        }
    }
    let mut at = 0;
    let mut leaves = BTreeMap::new();
    walk(raw, &mut at, String::new(), &mut leaves);
    assert_eq!(&raw[at..], b"\n");
    leaves
}

fn uuid(value: &str) {
    let bytes = value.as_bytes();
    assert_eq!(bytes.len(), 36);
    for (i, b) in bytes.iter().enumerate() {
        if [8, 13, 18, 23].contains(&i) {
            assert_eq!(*b, b'-');
        } else {
            assert!(b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        }
    }
    assert_eq!(bytes[14], b'4');
    assert!(b"89ab".contains(&bytes[19]));
}

fn splice(raw: &[u8], ledger: &Json, ids: &[&str]) -> Vec<u8> {
    let slots = ledger.as_array().unwrap();
    assert_eq!(slots.len(), ids.len());
    let tokens = spans(raw);
    let mut output = raw.to_vec();
    for i in (0..ids.len()).rev() {
        let slot = &slots[i];
        keys(slot, &["end", "literal", "path", "source", "start"]);
        assert_eq!(slot["path"], PATHS[i]);
        assert_eq!(slot["source"], PATHS[i]);
        assert_eq!(slot["start"], RANGES[i].start);
        assert_eq!(slot["end"], RANGES[i].end);
        assert_eq!(tokens[PATHS[i]], RANGES[i]);
        let literal = serde_json::to_vec(UUIDS[i]).unwrap();
        assert_eq!(slot["literal"].as_str().unwrap().as_bytes(), literal);
        assert_eq!(&raw[RANGES[i].clone()], literal);
        uuid(ids[i]);
        output.splice(RANGES[i].clone(), serde_json::to_vec(ids[i]).unwrap());
    }
    output
}

// This projection is deliberately limited to these three captured authorities.
// Validate every numeric token before serde decoding; no arbitrary Authority
// integer or float can enter the independent UTF-8 revision implementation.
fn revision(raw: &[u8], process: bool, native: bool) -> (String, Vec<u8>, String) {
    let tokens = spans(raw);
    let mut numbers = BTreeMap::from([
        (
            "/schema_version".to_owned(),
            if process { "2" } else { "1" }.to_owned(),
        ),
        ("/content_revision".to_owned(), "2".to_owned()),
        (
            "/groups/0/members/0/binding/presentation/endpoint/device".to_owned(),
            u64::MAX.to_string(),
        ),
        (
            "/groups/0/members/0/binding/presentation/endpoint/inode".to_owned(),
            "42".to_owned(),
        ),
    ]);
    if process {
        for (i, pid) in [7, 9].into_iter().enumerate() {
            numbers.insert(
                format!("/groups/0/members/0/binding/evidence/process_evidence/matches/{i}/pid"),
                pid.to_string(),
            );
        }
    }
    let actual: BTreeMap<_, _> = tokens
        .iter()
        .filter_map(|(path, range)| {
            let token = &raw[range.clone()];
            (token[0] == b'-' || token[0].is_ascii_digit())
                .then(|| (path.clone(), std::str::from_utf8(token).unwrap().to_owned()))
        })
        .collect();
    assert_eq!(actual, numbers);
    let state: Json = serde_json::from_slice(raw).unwrap();
    keys(
        &state,
        &[
            "content_revision",
            "groups",
            "publication_counters",
            "schema_version",
            "workspace_id",
        ],
    );
    assert_eq!(state["publication_counters"], json!([]));
    assert_eq!(state["groups"].as_array().unwrap().len(), 1);
    let group = &state["groups"][0];
    keys(group, &["group_id", "key", "members", "name"]);
    assert_eq!(group["members"].as_array().unwrap().len(), 1);
    let member = &group["members"][0];
    keys(member, &["binding", "key", "participant_id", "role"]);
    let binding = &member["binding"];
    keys(binding, &["evidence", "presentation"]);
    keys(
        &binding["presentation"],
        &["adapter", "endpoint", "pane_id_at_binding", "terminal_id"],
    );
    keys(
        &binding["presentation"]["endpoint"],
        &["device", "inode", "path"],
    );
    let evidence = &binding["evidence"];
    keys(
        evidence,
        if process {
            &[
                "agent_kind_hint",
                "native_binding",
                "native_reference",
                "observed_at",
                "process_evidence",
                "provenance",
            ]
        } else {
            &[
                "agent_kind_hint",
                "native_binding",
                "native_reference",
                "observed_at",
                "provenance",
            ]
        },
    );
    if native {
        keys(
            &evidence["native_reference"],
            &["agent", "kind", "source", "value"],
        );
    } else {
        assert!(evidence["native_reference"].is_null());
    }
    if process {
        let details = &evidence["process_evidence"];
        keys(details, &["interpreter", "matches"]);
        assert_eq!(details["matches"].as_array().unwrap().len(), 2);
        for item in details["matches"].as_array().unwrap() {
            keys(item, &["executable_family", "pid", "rule"]);
        }
    }
    for path in numbers.keys() {
        let number = state.pointer(path).unwrap().as_number().unwrap();
        assert!(number.is_u64() && !number.is_f64());
    }
    fn sorted(value: &Json) -> Json {
        match value {
            Json::Object(object) => {
                let map: BTreeMap<_, _> =
                    object.iter().map(|(k, v)| (k.clone(), sorted(v))).collect();
                serde_json::to_value(map).unwrap()
            }
            Json::Array(array) => Json::Array(array.iter().map(sorted).collect()),
            _ => value.clone(),
        }
    }
    let content: BTreeMap<_, _> = ["workspace_id", "content_revision", "groups"]
        .into_iter()
        .map(|key| (key, sorted(&state[key])))
        .collect();
    let canonical = serde_json::to_vec(&content).unwrap();
    let hash = digest(&[b"coven.groups.content:v1\0".as_slice(), &canonical].concat());
    (
        format!(
            "groups-v1:{}:2:{hash}",
            state["workspace_id"].as_str().unwrap()
        ),
        canonical,
        hash,
    )
}

pub(super) struct Case {
    row: Json,
    request: Json,
    expected: Json,
}
pub(super) struct Expected {
    pub raw: Vec<u8>,
    pub sha256: String,
    pub revision: String,
    pub binding: String,
}

pub(super) fn case(process: bool, native: bool) -> Case {
    if std::env::var_os("COVEN_GROUP_COMMANDS_ASSERT_PYTHON_ABSENT").is_some() {
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
            #[cfg(target_os = "linux")]
            assert_eq!(error.raw_os_error(), Some(libc::ENOENT), "{path}");
        }
    }
    let corpus = include_bytes!("../../../tests/retirement/fixtures/group_commands.json");
    let proof = include_bytes!("../../../tests/retirement/fixtures/group_commands_provenance.json");
    assert_eq!(
        digest(corpus),
        "6b8b2b1df050d3b9eb69296ca4fe85c12a58f0c037ccdb95ff959b39f8d22918"
    );
    assert_eq!(
        digest(proof),
        "d6c799ea1ef7b6a2b916706a6a51ba6616a8083b49ff8fc1eea801338a920f00"
    );
    let corpus: Json = serde_json::from_slice(corpus).unwrap();
    let proof: Json = serde_json::from_slice(proof).unwrap();
    keys(&corpus, &["cases", "schema_version"]);
    keys(
        &proof,
        &[
            "binding_instances",
            "capture_tool_sha256",
            "case_count",
            "construction",
            "corpus_sha256",
            "environment",
            "framing",
            "historical_commit",
            "interpreter",
            "negative_controls",
            "repeat_capture_equal",
            "request_bytes",
            "request_framed_hex",
            "request_sha256",
            "schema_version",
            "source_sha256",
            "stdout_bytes",
            "stdout_framed_hex",
            "stdout_sha256",
        ],
    );
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(
        proof["corpus_sha256"],
        "6b8b2b1df050d3b9eb69296ca4fe85c12a58f0c037ccdb95ff959b39f8d22918"
    );
    assert_eq!(
        proof["historical_commit"],
        "bbda1c4f79a7c71e6785c25d7bd9d939deec6d71"
    );
    assert_eq!(proof["case_count"], 3);
    assert_eq!(proof["repeat_capture_equal"], true);
    assert_eq!(
        proof["capture_tool_sha256"],
        "cb64d74bb07ea25cd5c498daba41e9dc23595c857cdecc21834dc1b6cf6ca1a2"
    );
    assert_eq!(
        proof["framing"],
        "8-byte big-endian length followed by complete stream, in case order"
    );
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
    assert_eq!(pins.len(), 71);
    assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
    for path in [
        "src/group_commands/oracle.py",
        "src/group_commands/tests.rs",
        "src/group_commands.rs",
        "src/group_commands/observation.rs",
        "src/groups/schema.rs",
        "src/groups/store.rs",
        "src/values/encode.rs",
        "src/values/parser.rs",
        "src/kitsune_observation/bindings.rs",
        "runner/groups.py",
        "runner/group_publisher.py",
    ] {
        assert!(pins.contains_key(path), "{path}");
    }
    for hash in pins.values().chain([&proof["interpreter"]["sha256"]]) {
        let hash = hash.as_str().unwrap().as_bytes();
        assert_eq!(hash.len(), 64);
        assert!(
            hash.iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        );
    }
    assert_eq!(
        proof["negative_controls"],
        json!({"active_checkout_rejected":true, "before_helper_execution":true, "individual_in_memory_pin_rejections":71})
    );
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let mut frames = [Vec::new(), Vec::new()];
    let mut ledger = Vec::new();
    let mut selected = None;
    for (i, (id, p, n)) in [
        ("snapshot-null", false, false),
        ("snapshot-native", false, true),
        ("process-null", true, false),
    ]
    .into_iter()
    .enumerate()
    {
        let row = &rows[i];
        keys(
            row,
            &[
                "canonical_content_sha256",
                "canonical_content_utf8",
                "domain_sha256",
                "id",
                "native",
                "process",
                "produced_utf8",
                "raw_bindings",
                "request_sha256",
                "request_utf8",
                "seed_bindings",
                "seed_sha256",
                "stdout_sha256",
                "stdout_utf8",
            ],
        );
        assert_eq!(row["id"], id);
        assert_eq!(row["process"], p);
        assert_eq!(row["native"], n);
        let input = row["request_utf8"].as_str().unwrap();
        let output = row["stdout_utf8"].as_str().unwrap();
        assert_eq!(digest(input.as_bytes()), row["request_sha256"]);
        assert_eq!(digest(output.as_bytes()), row["stdout_sha256"]);
        for (frame, stream) in frames.iter_mut().zip([input, output]) {
            frame.extend_from_slice(&(stream.len() as u64).to_be_bytes());
            frame.extend_from_slice(stream.as_bytes());
        }
        let request: Json = serde_json::from_str(input).unwrap();
        keys(&request, &["participant", "process", "produced", "seed"]);
        assert_eq!(serde_json::to_vec(&request).unwrap(), input.as_bytes());
        assert_eq!(request["process"], p);
        assert_eq!(request["participant"], UUIDS[2]);
        assert_eq!(
            serde_json::to_vec(&request["produced"]).unwrap(),
            row["produced_utf8"].as_str().unwrap().as_bytes()
        );
        let seed = request["seed"].as_str().unwrap().as_bytes();
        assert_eq!(digest(seed), row["seed_sha256"]);
        assert_eq!(splice(seed, &row["seed_bindings"], &UUIDS[..2]), seed);
        let expected: Json = serde_json::from_str(output).unwrap();
        keys(&expected, &["binding", "raw", "revision", "sha256"]);
        let raw = expected["raw"].as_str().unwrap().as_bytes();
        assert_eq!(splice(raw, &row["raw_bindings"], &UUIDS), raw);
        assert_eq!(digest(raw), expected["sha256"]);
        let (rev, canonical, domain) = revision(raw, p, n);
        assert_eq!(rev, expected["revision"]);
        assert_eq!(
            canonical,
            row["canonical_content_utf8"].as_str().unwrap().as_bytes()
        );
        assert_eq!(digest(&canonical), row["canonical_content_sha256"]);
        assert_eq!(domain, row["domain_sha256"]);
        let binding: Json = serde_json::from_str(expected["binding"].as_str().unwrap()).unwrap();
        assert_eq!(binding, request["produced"]);
        ledger.push(json!({"id":id,"raw":3,"seed":2,"total":5}));
        if (p, n) == (process, native) {
            selected = Some(Case {
                row: row.clone(),
                request,
                expected,
            });
        }
    }
    assert_eq!(proof["binding_instances"], Json::Array(ledger));
    for (prefix, frame) in ["request", "stdout"].into_iter().zip(frames) {
        assert_eq!(
            frame.len() as u64,
            proof[format!("{prefix}_bytes")].as_u64().unwrap()
        );
        assert_eq!(digest(&frame), proof[format!("{prefix}_sha256")]);
        let hex: String = frame.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, proof[format!("{prefix}_framed_hex")]);
    }
    selected.expect("only the three original tuples are allowed")
}

impl Case {
    pub(super) fn check_input(&self, seed: &[u8], produced: &Json) -> [String; 2] {
        let tokens = spans(seed);
        let ids: [String; 2] = std::array::from_fn(|i| {
            serde_json::from_slice(&seed[tokens[PATHS[i]].clone()]).unwrap()
        });
        for id in &ids {
            uuid(id);
        }
        assert_ne!(ids[0], ids[1]);
        assert_eq!(
            seed,
            splice(
                self.request["seed"].as_str().unwrap().as_bytes(),
                &self.row["seed_bindings"],
                &[&ids[0], &ids[1]]
            )
        );
        assert_eq!(
            serde_json::to_vec(produced).unwrap(),
            self.row["produced_utf8"].as_str().unwrap().as_bytes()
        );
        ids
    }

    pub(super) fn bind(self, ids: &[String; 2], participant: &str) -> Expected {
        uuid(participant);
        assert_ne!(participant, ids[0]);
        assert_ne!(participant, ids[1]);
        let raw = splice(
            self.expected["raw"].as_str().unwrap().as_bytes(),
            &self.row["raw_bindings"],
            &[&ids[0], &ids[1], participant],
        );
        let (revision, _, _) = revision(
            &raw,
            self.row["process"].as_bool().unwrap(),
            self.row["native"].as_bool().unwrap(),
        );
        Expected {
            sha256: digest(&raw),
            raw,
            revision,
            binding: self.expected["binding"].as_str().unwrap().to_owned(),
        }
    }
}
