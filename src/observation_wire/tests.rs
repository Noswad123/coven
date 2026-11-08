use super::*;

#[test]
fn native_profile_preserves_extension_integer_and_legacy_error_codes() {
    let raw = format!("{{\"extension\":{}}}", "9".repeat(400));
    assert_eq!(
        decode_native(raw.as_bytes()).unwrap().compact().unwrap(),
        raw.as_bytes()
    );
    for (raw, code) in [
        (b"\xff".as_slice(), "invalid_utf8"),
        (b"{\"a\":1,\"a\":2}", "duplicate_key"),
        (b"NaN", "nonfinite_json"),
        (b"1e9999", "schema_error"),
        (b"\"\\ud800\"", "schema_error"),
        (b"{", "invalid_json"),
    ] {
        assert_eq!(decode_native(raw).unwrap_err().code(), code);
    }
}

#[test]
fn global_character_container_and_lexical_depth_limits() {
    for (key, n, ok) in [
        ("extension", 4096, true),
        ("extension", 4097, false),
        ("path", 8192, true),
        ("path", 8193, false),
    ] {
        let raw = serde_json::to_vec(&serde_json::json!({key:"\u{e9}".repeat(n)})).unwrap();
        assert_eq!(decode_native(&raw).is_ok(), ok);
    }
    for n in [4096, 4097] {
        let raw = serde_json::to_vec(&vec![0; n]).unwrap();
        assert_eq!(decode_native(&raw).is_ok(), n == 4096);
    }
    let raw = format!("{}malformed", "[".repeat(33));
    assert_eq!(
        decode_native(raw.as_bytes()).unwrap_err().code(),
        "nesting_limit"
    );
}

#[test]
fn integer_projection_is_lossless_or_explicitly_refused() {
    for raw in [
        "0",
        "-1",
        "9223372036854775807",
        "-9223372036854775808",
        "18446744073709551615",
    ] {
        let native = decode_native(raw.as_bytes()).unwrap();
        assert_eq!(to_json(&native).unwrap().to_string(), raw);
    }
    for raw in [
        "18446744073709551616".to_owned(),
        "-9223372036854775809".into(),
        "9".repeat(4000),
    ] {
        let native = decode_native(raw.as_bytes()).unwrap();
        assert_eq!(native.compact().unwrap(), raw.as_bytes());
        assert_eq!(
            to_json(&native).unwrap_err().code(),
            "unsupported_json_integer_projection"
        );
    }
}

#[test]
fn float_projection_preserves_binary64_and_signed_zero() {
    for raw in [
        "-0.0",
        "0.0",
        "1.2345678901234567",
        "5e-324",
        "1.7976931348623157e308",
    ] {
        let native = decode_native(raw.as_bytes()).unwrap();
        let Value::Number(Number::Float(n)) = &native else {
            panic!("float required");
        };
        assert_eq!(
            to_json(&native).unwrap().as_f64().unwrap().to_bits(),
            n.bits()
        );
    }
}

#[test]
fn acknowledgement_projects_after_global_validation_without_serde_magic() {
    for integer in ["18446744073709551616".to_owned(), "9".repeat(4000)] {
        let raw = format!(
            r#"{{"id":"r","result":{{"type":"ok","ignored":{integer}}},"$serde_json::private::Number":{integer},"ignored":{integer}}}"#
        );
        assert_eq!(
            decode_ack(raw.as_bytes()).unwrap(),
            serde_json::json!({"id":"r","result":{"type":"ok"}})
        );
        let raw = format!(
            r#"{{"id":"r","error":{{"code":"disabled","message":"safe","ignored":{integer}}}}}"#
        );
        assert_eq!(
            decode_ack(raw.as_bytes()).unwrap(),
            serde_json::json!({"id":"r","error":{"code":"disabled","message":"safe"}})
        );
        for key in ["id", "type", "code", "message"] {
            let raw = match key {
                "id" => format!(r#"{{"id":{integer},"result":{{"type":"ok"}}}}"#),
                "type" => format!(r#"{{"id":"r","result":{{"type":{integer}}}}}"#),
                "code" => format!(r#"{{"id":"r","error":{{"code":{integer},"message":"safe"}}}}"#),
                _ => format!(r#"{{"id":"r","error":{{"code":"disabled","message":{integer}}}}}"#),
            };
            assert_eq!(
                decode_ack(raw.as_bytes()).unwrap_err().code(),
                "schema_error"
            );
        }
    }
    for raw in [
        r#"{"id":"r","result":{"type":"ok"},"ignored":{"x":1,"x":2}}"#,
        r#"{"id":"r","result":{"type":"ok"},"ignored":NaN}"#,
    ] {
        assert!(decode_ack(raw.as_bytes()).is_err());
    }
}
