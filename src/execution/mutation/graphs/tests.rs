use super::super::super::{Profile, apply_successor, validated, values};
use super::*;
use std::{cell::Cell, process::Command};

struct Ids(Cell<usize>);
impl Metadata for Ids {
    fn uuid(&self) -> Result<String> {
        let next = self.0.get() + 1;
        self.0.set(next);
        Ok(format!("{next:032x}"))
    }
}

fn vectors() -> Value {
    let output = Command::new("python3")
        .args(["-B", "src/execution/mutation/graphs/oracle.py"])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    values::parse(&output.stdout, Profile::Authority).unwrap()
}

fn operation(value: &Value) -> Operation {
    match text(get(value, "operation").unwrap()).unwrap() {
        "graph_propose" => Operation::GraphPropose,
        "graph_decide" => Operation::GraphDecide,
        "prepare_graph" => Operation::PrepareGraph,
        "prepare" => Operation::Prepare,
        _ => panic!("unexpected oracle operation"),
    }
}

#[test]
fn python_graph_effects_and_authoritative_successors_match() {
    let vectors = vectors();
    let cases = vectors.as_array().unwrap();
    assert!(cases.len() >= 20);
    for case in cases {
        let label = text(get(case, "label").unwrap()).unwrap();
        let mut control = get(case, "control").unwrap().clone();
        let mut selected = validated(&control).unwrap();
        let source = get(case, "source").unwrap();
        let request = Request {
            operation: operation(case),
            parameters: get(case, "parameters").unwrap().clone(),
            expected_execution: text(get(case, "expected_execution").unwrap()).unwrap(),
            expected_tasks: get(case, "expected_tasks").unwrap().as_str(),
            key: text(get(case, "key").unwrap()).unwrap(),
        };
        let ids = Ids(Cell::new(text_number(get(case, "counter").unwrap())));
        let mut work = Work::default();
        let binding = request.binding(2, &mut work).unwrap();
        let built = if request.operation == Operation::Prepare {
            super::super::effects::build(
                &control,
                Some(&selected),
                &request,
                source,
                text(get(case, "now").unwrap()).unwrap(),
                &ids,
                &mut work,
            )
        } else {
            build(
                &control,
                &selected,
                &request,
                if matches!(source, Value::Null) {
                    None
                } else {
                    Some(source)
                },
                text(get(case, "now").unwrap()).unwrap(),
                &ids,
                &mut work,
            )
        };
        let accepted = matches!(get(case, "accepted").unwrap(), Value::Bool(true));
        let effect = match built {
            Ok(effect) => effect,
            Err(error) => {
                assert!(!accepted, "{label}: builder rejected {}", error.0);
                continue;
            }
        };
        let generation = text_number(get(&control, "generation").unwrap()) + 1;
        let result = apply_successor(
            &control,
            Some(&mut selected),
            &binding,
            &effect,
            generation,
            &mut work,
        );
        if !accepted {
            assert!(
                result.is_err(),
                "{label}: accepted a Python-rejected successor"
            );
            continue;
        }
        let result = result.unwrap_or_else(|e| panic!("{label}: reducer rejected {}", e.0));
        assert_eq!(
            effect.canonical().unwrap(),
            get(case, "effect").unwrap().canonical().unwrap(),
            "{label}: effect"
        );
        assert_eq!(
            result.canonical().unwrap(),
            get(case, "result").unwrap().canonical().unwrap(),
            "{label}: result"
        );
        let mut history = get(&control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        history.push(
            object(vec![
                ("generation", integer(generation).unwrap()),
                ("request_id", string(request.key)),
                ("binding", binding),
                ("effect", effect),
                ("result", result),
            ])
            .unwrap(),
        );
        control.set_field("history", Value::Array(history)).unwrap();
        control
            .set_field("generation", integer(generation).unwrap())
            .unwrap();
        validated(&control).unwrap_or_else(|e| panic!("{label}: candidate rejected {}", e.0));
        assert_eq!(
            control.canonical().unwrap(),
            get(case, "after").unwrap().canonical().unwrap(),
            "{label}: complete candidate"
        );
    }
}

fn text_number(value: &Value) -> usize {
    String::from_utf8(value.canonical().unwrap())
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn changes_requested_effect_never_uses_source_or_changes_operator_reason() {
    let vectors = vectors();
    let case = vectors
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            get(case, "label").unwrap().as_str()
                == Some("source-free-revocation-preserves-reservations")
        })
        .unwrap();
    let control = get(case, "control").unwrap();
    let selected = validated(control).unwrap();
    let request = Request {
        operation: Operation::GraphDecide,
        parameters: get(case, "parameters").unwrap().clone(),
        expected_execution: text(get(case, "expected_execution").unwrap()).unwrap(),
        expected_tasks: None,
        key: "source-free",
    };
    // Even a supplied poison value must not become part of a source-free effect.
    let poison = string("invalid cfg/tasks source");
    for source in [None, Some(&poison)] {
        let effect = build(
            control,
            &selected,
            &request,
            source,
            text(get(case, "now").unwrap()).unwrap(),
            &Ids(Cell::new(text_number(get(case, "counter").unwrap()))),
            &mut Work::default(),
        )
        .unwrap();
        assert_eq!(
            effect.canonical().unwrap(),
            get(case, "effect").unwrap().canonical().unwrap()
        );
        assert_eq!(
            get(&effect, "decision")
                .unwrap()
                .as_object()
                .unwrap()
                .entries()
                .len(),
            6
        );
        assert_eq!(effect.as_object().unwrap().entries().len(), 1);
    }
}
