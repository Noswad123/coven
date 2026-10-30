//! Python-compatible encoding ONLY for the accepted closed v1 evidence models.
//! Not a generic canonicalizer for future task extensions, floats or big integers.
use crate::{
    Result,
    archive::{Envelope, Payload},
    handoff::ValidatedHandoff,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // serde_json's default Map is ordered. Converting structs to Value sorts all
    // object levels, rather than serializing Rust declaration order into v1 IDs.
    Ok(serde_json::to_vec(&serde_json::to_value(value)?)?)
}

pub(crate) fn handoff(value: &ValidatedHandoff) -> Result<Vec<u8>> {
    encode(value)
}
pub(crate) fn payload(value: &Payload) -> Result<Vec<u8>> {
    encode(value)
}
pub(crate) fn envelope(value: &Envelope) -> Result<Vec<u8>> {
    let mut raw = encode(value)?;
    raw.push(b'\n');
    Ok(raw)
}
pub(crate) fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
pub(crate) fn collection_id(value: &Payload) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"coven.handoff-collection.v1\0");
    hash.update(payload(value)?);
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_domain_has_frozen_sorted_utf8_bytes() {
        // Python's sorted compact ensure_ascii=False representation, not Rust
        // struct declaration order. Changing serde map features must fail here.
        let expected = r#"{"blockers":[],"checks":[],"claimed_status":"completed","confidence":"medium","contract":"coven.worker-handoff.v1","files_changed":[],"files_inspected":[],"findings":[],"follow_ups":[],"references":[],"risks":[],"schema_version":1,"source_claim":null,"summary":"Golden λ 🦊","task":{"id":"task","owner":"worker","revision":"tasks-missing:v1"}}"#;
        let model = crate::handoff::decode(expected.as_bytes()).unwrap();
        assert_eq!(handoff(&model).unwrap(), expected.as_bytes());
    }
}
