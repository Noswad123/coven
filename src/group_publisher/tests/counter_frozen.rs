//! Complete historical authority bytes. Only reviewed expected-side scalar spans vary.
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ops::Range};

const ENDPOINT: &str = "/groups/0/members/0/binding/presentation/endpoint";
const STAMP: &str = "/groups/0/members/0/binding/evidence/observed_at";
const UUIDS: [&str; 3] = [
    "/workspace_id",
    "/groups/0/group_id",
    "/groups/0/members/0/participant_id",
];
const IDS: [&str; 16] = [
    "primitive-r0-m0-p0",
    "primitive-r0-m0-p1",
    "primitive-r0-m1-p0",
    "primitive-r0-m1-p1",
    "primitive-r1-m0-p0",
    "primitive-r1-m0-p1",
    "primitive-r1-m1-p0",
    "primitive-r1-m1-p1",
    "publisher-r0-m0-p0",
    "publisher-r0-m0-p1",
    "publisher-r0-m1-p0",
    "publisher-r0-m1-p1",
    "publisher-r1-m0-p0",
    "publisher-r1-m0-p1",
    "publisher-r1-m1-p0",
    "publisher-r1-m1-p1",
];

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

fn encoded(value: &Json) -> Vec<u8> {
    let mut raw = serde_json::to_vec_pretty(value).unwrap();
    raw.push(b'\n');
    raw
}

fn inventory(maximum: bool, counters: usize) -> Vec<(String, String)> {
    let mut paths: Vec<_> = UUIDS
        .into_iter()
        .chain([STAMP])
        .map(|p| (p.into(), p.into()))
        .collect();
    for target in std::iter::once(ENDPOINT.to_owned())
        .chain((0..counters).map(|i| format!("/publication_counters/{i}/endpoint")))
    {
        for key in if maximum {
            &["path"][..]
        } else {
            &["path", "device", "inode"][..]
        } {
            paths.push((format!("{target}/{key}"), format!("{ENDPOINT}/{key}")));
        }
    }
    paths
}

// Walk token boundaries independently of the capture offsets. Authority parsing
// validates the document first, without ever projecting its integers into serde.
fn spans(raw: &[u8]) -> BTreeMap<String, Range<usize>> {
    crate::values::parse(raw, crate::values::Profile::Authority).unwrap();
    fn whitespace(raw: &[u8], at: &mut usize) {
        while raw.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    }
    fn string_end(raw: &[u8], at: &mut usize) {
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
        whitespace(raw, at);
        let start = *at;
        match raw[*at] {
            b'{' | b'[' => {
                let object = raw[*at] == b'{';
                let close = if object { b'}' } else { b']' };
                *at += 1;
                let mut index = 0;
                whitespace(raw, at);
                while raw[*at] != close {
                    let key = if object {
                        let begin = *at;
                        string_end(raw, at);
                        let key: String = serde_json::from_slice(&raw[begin..*at]).unwrap();
                        assert!(!key.contains(['/', '~']));
                        whitespace(raw, at);
                        assert_eq!(raw[*at], b':');
                        *at += 1;
                        key
                    } else {
                        index.to_string()
                    };
                    walk(raw, at, format!("{path}/{key}"), leaves);
                    index += 1;
                    whitespace(raw, at);
                    if raw[*at] != close {
                        assert_eq!(raw[*at], b',');
                        *at += 1;
                        whitespace(raw, at);
                    }
                }
                *at += 1;
            }
            _ => {
                if raw[*at] == b'"' {
                    string_end(raw, at);
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
    let mut leaves = BTreeMap::new();
    let mut at = 0;
    walk(raw, &mut at, String::new(), &mut leaves);
    assert_eq!(&raw[at..], b"\n");
    leaves
}

fn literal(source: &str) -> &'static [u8] {
    match source {
        "/workspace_id" => br#""11111111-1111-4111-8111-111111111111""#,
        "/groups/0/group_id" => br#""22222222-2222-4222-8222-222222222222""#,
        "/groups/0/members/0/participant_id" => br#""33333333-3333-4333-8333-333333333333""#,
        STAMP => br#""2026-11-27T12:34:56.123456+00:00""#,
        "/groups/0/members/0/binding/presentation/endpoint/path" => {
            br#""/group-counter-capture/socket""#
        }
        "/groups/0/members/0/binding/presentation/endpoint/device" => b"314159",
        "/groups/0/members/0/binding/presentation/endpoint/inode" => b"271828",
        _ => panic!("unreviewed source {source}"),
    }
}

fn bind(
    raw: &[u8],
    slots: &Json,
    inventory: &[(String, String)],
    sources: &BTreeMap<String, Vec<u8>>,
) -> Vec<u8> {
    let tokens = spans(raw);
    let slots = slots.as_array().unwrap();
    assert_eq!(slots.len(), inventory.len());
    let mut edits = BTreeMap::new();
    for i in 0..inventory.len() {
        let slot = &slots[i];
        keys(slot, &["end", "literal", "path", "source", "start"]);
        let (path, source) = &inventory[i];
        assert_eq!(slot["path"], path.as_str());
        assert_eq!(slot["source"], source.as_str());
        let range = tokens.get(path).unwrap().clone();
        assert_eq!(slot["start"], range.start);
        assert_eq!(slot["end"], range.end);
        assert_eq!(&raw[range.clone()], literal(source));
        assert_eq!(
            slot["literal"].as_str().unwrap().as_bytes(),
            literal(source)
        );
        assert!(
            edits
                .insert(range.start, (range.end, sources.get(source).unwrap()))
                .is_none()
        );
    }
    let mut result = Vec::new();
    let mut at = 0;
    for (start, (end, replacement)) in edits {
        assert!(start >= at);
        result.extend_from_slice(&raw[at..start]);
        result.extend_from_slice(replacement);
        at = end;
    }
    result.extend_from_slice(&raw[at..]);
    result
}

pub(super) struct Cases(Vec<Json>);
impl Cases {
    pub(super) fn new() -> Self {
        if std::env::var_os("COVEN_GROUP_COUNTER_ASSERT_PYTHON_ABSENT").is_some() {
            for path in [
                "/usr/bin/python3",
                "/bin/python3",
                "/usr/bin/python3.14",
                "/usr/local/bin/python3",
                "/home/linuxbrew/.linuxbrew/bin/python3",
                "/usr/lib/python3.14",
                "/usr/lib/libpython3.14.so",
            ] {
                let error = std::fs::symlink_metadata(path).unwrap_err();
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{path}");
                assert_eq!(error.raw_os_error(), Some(libc::ENOENT), "{path}");
            }
        }
        let raw = include_bytes!("../../../tests/retirement/fixtures/group_counter.json");
        let proof =
            include_bytes!("../../../tests/retirement/fixtures/group_counter_provenance.json");
        assert_eq!(
            digest(raw),
            "05c08611c0ecd220ab4c5aad891253aaf6bed33a35adb9e7f41fc034d1b4d31a"
        );
        assert_eq!(
            digest(proof),
            "f779dba7327c8ea7fb9a60a464416e82527d1c7e76e32616d7a914650dbeb132"
        );
        // Outer JSON unwraps opaque authority strings only, never authority numbers.
        let data: Json = serde_json::from_slice(raw).unwrap();
        let proof: Json = serde_json::from_slice(proof).unwrap();
        keys(&data, &["cases", "schema_version"]);
        assert_eq!(data["schema_version"], 1);
        assert_eq!(proof["schema_version"], 1);
        assert_eq!(
            proof["historical_commit"],
            "727edc7aad55ed76e98670e0cee6449d611b2285"
        );
        assert_eq!(proof["corpus_sha256"], digest(raw));
        assert_eq!(proof["case_count"], 16);
        assert_eq!(proof["repeat_capture_equal"], true);
        let pins = proof["source_sha256"].as_object().unwrap();
        assert_eq!(pins.len(), 53);
        assert_eq!(pins.keys().filter(|p| p.starts_with("runner/")).count(), 25);
        for path in [
            "src/group_publisher/counter_oracle.py",
            "src/group_publisher/native_tests.rs",
            "src/group_publisher/native.rs",
            "src/groups.rs",
            "src/groups/store.rs",
            "src/kitsune_observation/bindings.rs",
            "src/values/encode.rs",
        ] {
            assert_eq!(pins[path].as_str().unwrap().len(), 64);
        }
        let rows = data["cases"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let mut requests = Vec::new();
        let mut stdout = Vec::new();
        let counts = proof["binding_instances"].as_array().unwrap();
        assert_eq!(counts.len(), 16);
        for i in 0..16 {
            let row = &rows[i];
            keys(
                row,
                &[
                    "before_bindings",
                    "before_sha256",
                    "id",
                    "request",
                    "request_sha256",
                    "stdout",
                    "stdout_bindings",
                    "stdout_sha256",
                ],
            );
            assert_eq!(row["id"], IDS[i]);
            let request = &row["request"];
            keys(
                request,
                &[
                    "before",
                    "maximum_stats",
                    "policy",
                    "prior_stream",
                    "reordered_declaration",
                    "source",
                ],
            );
            assert_eq!(
                request["policy"],
                if i < 8 { "primitive" } else { "publisher" }
            );
            assert_eq!(request["reordered_declaration"], i % 8 >= 4);
            assert_eq!(request["maximum_stats"], i % 4 >= 2);
            assert_eq!(request["prior_stream"], i % 2 == 1);
            assert_eq!(request["source"], "coven");
            assert_eq!(row["request_sha256"], digest(&encoded(request)));
            let before = request["before"].as_str().unwrap().as_bytes();
            let response = row["stdout"].as_str().unwrap().as_bytes();
            assert_eq!(row["before_sha256"], digest(before));
            assert_eq!(row["stdout_sha256"], digest(response));
            let maximum = i % 4 >= 2;
            let before_slots = inventory(maximum, 1);
            let after_slots = inventory(maximum, if i % 2 == 1 { 1 } else { 2 });
            let sources = before_slots
                .iter()
                .map(|(_, source)| (source.clone(), literal(source).to_vec()))
                .collect();
            assert_eq!(
                bind(before, &row["before_bindings"], &before_slots, &sources),
                before
            );
            assert_eq!(
                bind(response, &row["stdout_bindings"], &after_slots, &sources),
                response
            );
            assert_eq!(
                counts[i],
                json!({"id":IDS[i], "before":before_slots.len(), "stdout":after_slots.len(), "total":before_slots.len()+after_slots.len()})
            );
            requests.push(request.clone());
            stdout.extend_from_slice(response);
        }
        let requests = encoded(&Json::Array(requests));
        assert_eq!(proof["request_utf8"].as_str().unwrap().as_bytes(), requests);
        assert_eq!(proof["request_bytes"], requests.len());
        assert_eq!(proof["request_sha256"], digest(&requests));
        assert_eq!(proof["stdout_utf8"].as_str().unwrap().as_bytes(), stdout);
        assert_eq!(proof["stdout_bytes"], stdout.len());
        assert_eq!(proof["stdout_sha256"], digest(&stdout));
        Self(rows.clone())
    }

    pub(super) fn expected(
        &self,
        i: usize,
        policy: &str,
        reordered: bool,
        maximum: bool,
        prior: bool,
        before: &[u8],
    ) -> Vec<u8> {
        let row = &self.0[i];
        assert_eq!(
            row["id"],
            format!(
                "{policy}-r{}-m{}-p{}",
                u8::from(reordered),
                u8::from(maximum),
                u8::from(prior)
            )
        );
        let tokens = spans(before);
        let paths = inventory(maximum, 1);
        let mut sources = BTreeMap::new();
        for (_, source) in &paths {
            sources.insert(source.clone(), before[tokens[source].clone()].to_vec());
        }
        let mut ids = Vec::new();
        for path in UUIDS {
            let id: String = serde_json::from_slice(&sources[path]).unwrap();
            let bytes = id.as_bytes();
            assert_eq!(bytes.len(), 36);
            for (at, byte) in bytes.iter().enumerate() {
                if [8, 13, 18, 23].contains(&at) {
                    assert_eq!(*byte, b'-');
                } else {
                    assert!(byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
                }
            }
            assert_eq!(bytes[14], b'4');
            assert!(matches!(bytes[19], b'8' | b'9' | b'a' | b'b'));
            assert!(!ids.contains(&id));
            ids.push(id);
        }
        let template = row["request"]["before"].as_str().unwrap().as_bytes();
        let expected_before = bind(template, &row["before_bindings"], &paths, &sources);
        assert_eq!(
            before, expected_before,
            "original complete pre-mutation input, {}",
            IDS[i]
        );
        // Sources are frozen here, before Session open/reread/reserve. Full output
        // equality consequently also requires all three generated IDs unchanged.
        bind(
            row["stdout"].as_str().unwrap().as_bytes(),
            &row["stdout_bindings"],
            &inventory(maximum, if prior { 1 } else { 2 }),
            &sources,
        )
    }
}
