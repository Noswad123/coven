use coven::values::{self, Number, Profile, Value};

#[test]
fn cpython_numeric_unicode_order_and_storage_corpus() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle = std::process::Command::new("python3")
        .arg(root.join("tests/_compat_values.py"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONINTMAXSTRDIGITS", "4300")
        .output()
        .expect("CPython compatibility oracle available during migration");
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let mut cases = 0;
    for line in oracle
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let expected: serde_json::Value = serde_json::from_slice(line).unwrap();
        let raw = expected["raw"].as_str().unwrap();
        let profile = if expected["profile"] == "legacy" {
            Profile::LegacySnapshot
        } else {
            Profile::Authority
        };
        let actual = values::parse(raw.as_bytes(), profile);
        if expected["valid"] == false {
            assert!(
                actual.is_err(),
                "unexpected accept in case {cases}: {:?}",
                &raw[..raw.len().min(80)]
            );
        } else {
            let actual =
                actual.unwrap_or_else(|error| panic!("case {cases} refused {raw:?}: {error}"));
            assert_eq!(
                actual.canonical().unwrap(),
                expected["canonical"].as_str().unwrap().as_bytes(),
                "canonical case {cases}: {raw:?}"
            );
            assert_eq!(
                actual.storage().unwrap(),
                expected["storage"].as_str().unwrap().as_bytes(),
                "storage case {cases}: {raw:?}"
            );
            if let Some(bits) = expected["float_bits"].as_str() {
                let Value::Number(Number::Float(value)) = actual else {
                    panic!("float type lost in {cases}")
                };
                assert_eq!(
                    format!("{:016x}", value.bits()),
                    bits,
                    "parsed bits in {cases}: {raw:?}"
                );
            } else if let Some(integer) = expected["integer"].as_str() {
                let Value::Number(Number::Integer(value)) = actual else {
                    panic!("integer type lost in {cases}")
                };
                assert_eq!(value.as_str(), integer);
            }
        }
        cases += 1;
    }
    assert!(cases > 17000, "corpus unexpectedly shrank: {cases}");
    println!("{cases} independent CPython value/byte/bit cases passed");
}

#[test]
fn duplicate_profiles_preserve_order_but_never_share_authority_policy() {
    let raw = br#"{"b":1,"a":2,"b":3}"#;
    assert!(values::parse(raw, Profile::Authority).is_err());
    let value = values::parse(raw, Profile::LegacySnapshot).unwrap();
    assert_eq!(value.canonical().unwrap(), br#"{"a":2,"b":3}"#);
    assert_eq!(value.storage().unwrap(), b"{\n  \"b\": 3,\n  \"a\": 2\n}\n");
}

#[test]
fn malformed_numbers_never_become_normalized_valid_values() {
    for raw in [
        b"00".as_slice(),
        b"01",
        b"-01",
        b"+0",
        b".1",
        b"1.",
        b"1e",
        b"1e+",
        b"1x",
        b"--1",
    ] {
        assert!(values::parse(raw, Profile::LegacySnapshot).is_err());
    }
}

#[test]
fn shortest_round_trip_is_not_enough_decimal_ties_must_match_python() {
    // Rust's standard Debug formatter chooses ...313 here; CPython chooses the
    // equally short even ...312. Keep this regression independent of the oracle.
    for raw in [
        b"2.9802322387695312e-08".as_slice(),
        b"-2.9802322387695312e-08",
    ] {
        assert_eq!(
            values::parse(raw, Profile::Authority)
                .unwrap()
                .canonical()
                .unwrap(),
            raw
        );
    }
}

#[test]
fn bounded_profile_edges_are_explicit_and_do_not_emit_partial_output() {
    for count in [64, 65] {
        let raw = format!("{}0{}", "[".repeat(count), "]".repeat(count));
        assert_eq!(
            values::parse(raw.as_bytes(), Profile::Authority).is_ok(),
            count == 64
        );
    }
    for count in [values::MAX_ITEMS, values::MAX_ITEMS + 1] {
        let raw = format!("[{}]", vec!["0"; count].join(","));
        assert_eq!(
            values::parse(raw.as_bytes(), Profile::Authority).is_ok(),
            count == values::MAX_ITEMS
        );
    }
    for count in [values::MAX_MEMBERS, values::MAX_MEMBERS + 1] {
        let raw = format!(
            "{{{}}}",
            (0..count)
                .map(|index| format!("\"k{index}\":0"))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            values::parse(raw.as_bytes(), Profile::Authority).is_ok(),
            count == values::MAX_MEMBERS
        );
    }
    let oversized = vec![b' '; values::MAX_INPUT + 1];
    assert!(values::parse(&oversized, Profile::Authority).is_err());
    let expansion = Value::Array(vec![Value::String("x".repeat(values::MAX_STRING)); 33]);
    assert!(expansion.storage().is_err());
}
