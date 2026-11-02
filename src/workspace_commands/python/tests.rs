use super::*;

#[test]
fn independent_python_str_repr_truthiness_all_unicode_scalars_and_numbers() {
    let oracle = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/workspace_commands/python/oracle.py"
        ))
        .output()
        .unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let mut count = 0;
    for line in oracle
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row = values::parse(line, values::Profile::LegacySnapshot).unwrap();
        let row = row.as_object().unwrap();
        let value = row.get("value").unwrap();
        let mut output = Output::default();
        output.value(value).unwrap();
        assert_eq!(
            output.finish(),
            row.get("text").unwrap().as_str().unwrap(),
            "case {count}"
        );
        assert_eq!(
            truthy(value),
            matches!(row.get("truthy"), Some(Value::Bool(true))),
            "case {count}"
        );
        count += 1;
    }
    assert!(count > 1300, "oracle corpus unexpectedly small: {count}");
}

#[test]
fn complete_output_cap_is_incremental_and_exact() {
    let mut output = Output::default();
    let block = "a".repeat(values::MAX_STRING);
    for _ in 0..32 {
        output.push(&block).unwrap();
    }
    assert_eq!(output.0.len(), values::MAX_OUTPUT);
    assert_eq!(
        output.push("x").unwrap_err().code(),
        "unsupported_history_output_profile"
    );
    assert_eq!(output.0.len(), values::MAX_OUTPUT);
    output.push("").unwrap();

    // Nested repr expansion, rather than input size, can exhaust the budget.
    let value = Value::Array(vec![Value::String("\0".repeat(values::MAX_STRING))]);
    let mut output = Output::default();
    for _ in 0..7 {
        output.value(&value).unwrap();
    }
    assert_eq!(
        output.value(&value).unwrap_err().code(),
        "unsupported_history_output_profile"
    );
    assert!(output.0.len() <= values::MAX_OUTPUT);
}

#[test]
fn recursion_is_bounded_even_for_values_constructed_without_parser() {
    let mut value = Value::Null;
    for _ in 0..values::MAX_DEPTH {
        value = Value::Array(vec![value]);
    }
    Output::default().value(&value).unwrap();
    let value = Value::Array(vec![value]);
    assert_eq!(
        Output::default().value(&value).unwrap_err().code(),
        "unsupported_json_depth_profile"
    );
}
