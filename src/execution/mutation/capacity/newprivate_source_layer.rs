//! Independent padding/prefix recipe, checked against the complete historical hash.
use super::*;

pub(super) fn near_nodes() -> Value {
    fn revision(control: &Value, schema: usize) -> String {
        format!(
            "execution-control-v{schema}:{}:{}:{}",
            text(get(control, "execution_authority_id").unwrap()).unwrap(),
            text_number(get(control, "generation").unwrap()),
            canonical::digest(&control.canonical().unwrap())
        )
    }
    fn text_number(value: &Value) -> String {
        String::from_utf8(value.canonical().unwrap()).unwrap()
    }
    let mut control = fixtures()[0].clone();
    let extension = object(vec![(
        "padding",
        Value::Array(
            [65536, 65019, 0]
                .map(|n| Value::Array(vec![Value::Null; n]))
                .to_vec(),
        ),
    )])
    .unwrap();
    control.set_field("extensions", extension.clone()).unwrap();
    let mut legacy = get(&control, "legacy_v1").unwrap().clone();
    legacy.set_field("extensions", extension).unwrap();
    let mut history = get(&legacy, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    for index in 1..history.len() {
        let mut prefix = legacy.clone();
        prefix
            .set_field("history", Value::Array(history[..index].to_vec()))
            .unwrap();
        prefix
            .set_field("generation", integer(index).unwrap())
            .unwrap();
        let mut binding = get(&history[index], "binding").unwrap().clone();
        binding
            .set_field("expected_execution_revision", string(revision(&prefix, 1)))
            .unwrap();
        history[index].set_field("binding", binding).unwrap();
    }
    legacy.set_field("history", Value::Array(history)).unwrap();
    let mut previous = revision(&legacy, 1);
    control.set_field("legacy_v1", legacy).unwrap();
    let mut upgrade = get(&control, "upgrade").unwrap().clone();
    upgrade
        .set_field("source_revision", string(&previous))
        .unwrap();
    control.set_field("upgrade", upgrade).unwrap();
    let mut history = get(&control, "history")
        .unwrap()
        .as_array()
        .unwrap()
        .to_vec();
    for key in ["effect", "result"] {
        let mut value = get(&history[0], key).unwrap().clone();
        value
            .set_field("source_revision", string(&previous))
            .unwrap();
        history[0].set_field(key, value).unwrap();
    }
    for index in 0..history.len() {
        let mut binding = get(&history[index], "binding").unwrap().clone();
        binding
            .set_field("expected_execution_revision", string(&previous))
            .unwrap();
        history[index].set_field("binding", binding).unwrap();
        let mut prefix = control.clone();
        prefix
            .set_field("history", Value::Array(history[..=index].to_vec()))
            .unwrap();
        prefix
            .set_field(
                "generation",
                get(&history[index], "generation").unwrap().clone(),
            )
            .unwrap();
        previous = revision(&prefix, 2);
    }
    control.set_field("history", Value::Array(history)).unwrap();
    assert_eq!(node_count(&control, 0).unwrap(), 262048);
    let bytes = control.canonical().unwrap();
    assert_eq!(bytes.len(), 1333175);
    assert_eq!(
        canonical::digest(&bytes),
        "9fc38d19ee086bba46853c703f914b7d3f0b67d2f53484cd5828024ae6882466"
    );
    control
}
