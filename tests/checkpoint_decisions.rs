//! Python is an independent test-only oracle, never a production dependency.
use coven::{
    checkpoints::inspect_bytes,
    values::{self, Profile, Value},
};
use std::process::Command;

fn unhex(text: &str) -> Vec<u8> {
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
fn python_checkpoint_inspection_parity() {
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/_checkpoint_parity.py"
        ))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run test-only Python checkpoint oracle");
    assert!(
        output.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (mut accepted, mut rejected, mut resources) = (0, 0, 0);
    for raw in output
        .stdout
        .split(|c| *c == b'\n')
        .filter(|line| !line.is_empty())
    {
        let fixture = values::parse(raw, Profile::Authority).expect("valid oracle JSONL");
        let object = fixture.as_object().unwrap();
        let id = object.get("id").unwrap().as_str().unwrap();
        let checkpoint = object.get("checkpoint").unwrap().as_str().unwrap();
        let data = object.get("rawbyteshex").unwrap().as_str().map(unhex);
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
    eprintln!(
        "checkpoint oracle: {accepted} accepted views, {rejected} rejected inputs ({resources} resource refusals)"
    );
}
