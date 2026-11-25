//! Complete frozen evidence from the independent historical Python oracle.
use coven::{
    checkpoints::inspect_bytes,
    values::{self, Profile, Value},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const CORPUS: &[u8] = include_bytes!("retirement/fixtures/checkpoint_inspection.json");
const PROVENANCE: &[u8] =
    include_bytes!("retirement/fixtures/checkpoint_inspection_provenance.json");

fn sha(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "odd fixture hex length");
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                _ => panic!("invalid fixture hex"),
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect()
}

#[test]
/// Stable test identity; all inputs and full expected views are frozen reference data.
fn python_checkpoint_inspection_parity() {
    assert_eq!(CORPUS.len(), 10_489_333);
    assert_eq!(
        sha(CORPUS),
        "c5bc0aeb46042a6ac79728c8e5b0d6c29c74149fdab7509499b14ac37ea5f6ce"
    );
    assert_eq!(
        sha(PROVENANCE),
        "cbce355486ae666924e17970fe2e4573a4541245dcf882b9aceac716353a5b68"
    );
    let provenance = values::parse(PROVENANCE, Profile::Authority).unwrap();
    let provenance = provenance.as_object().unwrap();
    let inventory = provenance.get("inventory").unwrap();
    assert_eq!(
        sha(&inventory.canonical().unwrap()),
        provenance
            .get("inventory_sha256")
            .unwrap()
            .as_str()
            .unwrap()
    );
    let inventory = inventory.as_array().unwrap();
    assert_eq!(inventory.len(), 2761);
    let mut identities = HashSet::new();
    let mut original_ids = BTreeMap::<&str, Vec<usize>>::new();
    let mut jsonl = Sha256::new();
    let (mut accepted, mut rejected, mut resources) = (0, 0, 0);
    // The manifest exceeds MAX_INPUT. Only individual unchanged rows are values.
    let body = CORPUS
        .strip_prefix(b"[\n")
        .unwrap()
        .strip_suffix(b"\n]\n")
        .unwrap();
    let rows: Vec<_> = body.split(|c| *c == b'\n').collect();
    assert_eq!(rows.len(), inventory.len());
    for (index, (line, evidence)) in rows.iter().zip(inventory).enumerate() {
        let raw = if index + 1 == inventory.len() {
            *line
        } else {
            line.strip_suffix(b",").expect("array row separator")
        };
        jsonl.update(raw);
        jsonl.update(b"\n");
        let evidence = evidence.as_object().unwrap();
        let mut row_digest = Sha256::new();
        row_digest.update(raw);
        row_digest.update(b"\n");
        assert_eq!(
            format!("{:x}", row_digest.finalize()),
            evidence.get("row_sha256").unwrap().as_str().unwrap()
        );
        assert_eq!(
            evidence.get("index").unwrap().canonical().unwrap(),
            index.to_string().as_bytes()
        );
        let fixture = values::parse(raw, Profile::Authority).expect("valid frozen oracle row");
        let object = fixture.as_object().unwrap();
        let id = object.get("id").unwrap().as_str().unwrap();
        assert_eq!(id, evidence.get("id").unwrap().as_str().unwrap());
        let identity = format!("{index}:{id}");
        assert_eq!(identity, evidence.get("case_id").unwrap().as_str().unwrap());
        assert!(identities.insert(identity));
        original_ids
            .entry(evidence.get("id").unwrap().as_str().unwrap())
            .or_default()
            .push(index);
        let checkpoint = object.get("checkpoint").unwrap().as_str().unwrap();
        let data = object.get("rawbyteshex").unwrap().as_str().map(unhex);
        if let Some(data) = &data {
            assert_eq!(
                sha(data),
                evidence.get("input_sha256").unwrap().as_str().unwrap(),
                "{index}:{id}"
            );
            assert_eq!(
                evidence.get("input_bytes").unwrap().canonical().unwrap(),
                data.len().to_string().as_bytes()
            );
        } else {
            assert!(matches!(evidence.get("input_sha256"), Some(Value::Null)));
            assert!(matches!(evidence.get("input_bytes"), Some(Value::Null)));
        }
        let result = inspect_bytes(checkpoint, data.as_deref());
        if matches!(object.get("accepted"), Some(Value::Bool(true))) {
            let actual =
                result.unwrap_or_else(|error| panic!("{id}: unexpected native error {error}"));
            let expected = object.get("expected").unwrap();
            assert_eq!(
                actual.canonical().unwrap(),
                expected.canonical().unwrap(),
                "{id}"
            );
            accepted += 1;
        } else {
            let error = result.expect_err(id);
            if let Some(expected) = object.get("expectednative_error") {
                assert_eq!(error.code(), expected.as_str().unwrap(), "{id}");
                resources += 1;
            } else {
                assert!(
                    error.code().starts_with("invalid_checkpoint"),
                    "{id}: {error}"
                );
            }
            rejected += 1;
        }
    }
    assert!(
        accepted > 1500 && rejected > 100,
        "incomplete corpus: {accepted} accepted, {rejected} rejected"
    );
    assert_eq!(resources, 12, "missing resource-refusal fixtures");
    assert_eq!((accepted, rejected, resources), (2212, 549, 12));
    assert_eq!(
        format!("{:x}", jsonl.finalize()),
        "f4a577e35f81ced65855de57734d740b032be975e0f5f536f801efd40f55b8e1"
    );
    let duplicates = provenance
        .get("historical_duplicate_ids")
        .unwrap()
        .as_object()
        .unwrap();
    let repeated: Vec<_> = original_ids
        .iter()
        .filter(|(_, indices)| indices.len() > 1)
        .collect();
    assert_eq!(repeated.len(), duplicates.entries().len());
    for (id, indices) in repeated {
        let expected = format!(
            "[{}]",
            indices
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            duplicates.get(id).unwrap().canonical().unwrap(),
            expected.as_bytes()
        );
    }
    assert_eq!(
        provenance
            .get("unique_original_ids")
            .unwrap()
            .canonical()
            .unwrap(),
        original_ids.len().to_string().as_bytes()
    );
    eprintln!(
        "checkpoint oracle: {accepted} accepted views, {rejected} rejected inputs ({resources} resource refusals)"
    );
}
