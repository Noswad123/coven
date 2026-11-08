use super::*;
use std::{cell::Cell, process::Command};

const NOW: &str = "2026-11-02T01:02:03.123456+00:00";

struct Fixed(Cell<usize>);
impl Metadata for Fixed {
    fn uuid(&self) -> Result<String> {
        self.0.set(self.0.get() + 1);
        Ok("d".repeat(32))
    }
}

#[test]
fn python_effects_and_checked_successors_match_for_both_schemas() {
    let output = Command::new("python3")
        .args(["-B", "-c", include_str!("oracle.py")])
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases = values::parse(&output.stdout, Profile::Authority).unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 23);
    for case in cases.as_array().unwrap() {
        let name = text(get(case, "case").unwrap()).unwrap();
        let control = get(case, "control").unwrap();
        let binding = get(case, "binding").unwrap();
        let operation = match text(get(binding, "operation").unwrap()).unwrap() {
            "enable" => Operation::Enable,
            "enroll" => Operation::Enroll,
            "prepare" => Operation::Prepare,
            _ => panic!("unexpected oracle operation"),
        };
        let request = Request {
            operation,
            parameters: get(binding, "parameters").unwrap().clone(),
            expected_execution: text(get(binding, "expected_execution_revision").unwrap()).unwrap(),
            expected_tasks: get(binding, "expected_tasks_revision").unwrap().as_str(),
            key: "candidate",
        };
        let mut selected = if matches!(operation, Operation::Enable) {
            None
        } else {
            Some(validated(control).unwrap())
        };
        let metadata = Fixed(Cell::new(0));
        let mut work = Work::default();
        let effect = build(
            control,
            selected.as_ref(),
            &request,
            get(case, "source").unwrap(),
            NOW,
            &metadata,
            &mut work,
        );
        if name == "missing_task" {
            assert_eq!(effect.unwrap_err().code(), "execution_task_not_found");
            assert_eq!(metadata.0.get(), 0);
            continue;
        }
        let effect = effect.unwrap();
        assert_eq!(
            effect.canonical().unwrap(),
            get(case, "effect").unwrap().canonical().unwrap(),
            "{name}"
        );
        assert!(work.0 > 0);
        assert_eq!(
            metadata.0.get(),
            usize::from(!matches!(
                name,
                "enable" | "reenroll" | "rebind" | "rebind_denied"
            )),
            "{name}"
        );
        if name == "missing_enrollment" {
            assert!(matches!(
                get(get(&effect, "prepared").unwrap(), "enrollment_id").unwrap(),
                Value::Null
            ));
        }
        let generation = selected
            .as_ref()
            .map_or(1, |view| view.replayed.receipts.len() + 1);
        let result = apply_successor(
            control,
            selected.as_mut(),
            binding,
            &effect,
            generation,
            &mut work,
        );
        let accepted = matches!(get(case, "accepted").unwrap(), Value::Bool(true));
        assert_eq!(result.is_ok(), accepted, "{name}");
        if accepted {
            let result = result.unwrap();
            assert_eq!(
                result.canonical().unwrap(),
                get(case, "result").unwrap().canonical().unwrap(),
                "{name}"
            );
            let mut candidate = control.clone();
            let mut history = get(control, "history")
                .unwrap()
                .as_array()
                .unwrap()
                .to_vec();
            history.push(
                object(vec![
                    ("generation", integer(generation).unwrap()),
                    ("request_id", string("candidate")),
                    ("binding", binding.clone()),
                    ("effect", effect),
                    ("result", result),
                ])
                .unwrap(),
            );
            candidate
                .set_field("history", Value::Array(history))
                .unwrap();
            candidate
                .set_field("generation", integer(generation).unwrap())
                .unwrap();
            validated(&candidate).unwrap();
        }
    }
}

#[test]
fn standalone_builder_rejects_other_operations_and_keeps_shared_work_limit() {
    let metadata = Fixed(Cell::new(0));
    for operation in [
        Operation::PrepareGraph,
        Operation::GraphPropose,
        Operation::GraphDecide,
        Operation::Upgrade,
    ] {
        let request = Request {
            operation,
            parameters: Value::Null,
            expected_execution: "",
            expected_tasks: None,
            key: "key",
        };
        assert_eq!(
            build(
                &Value::Null,
                None,
                &request,
                &Value::Null,
                NOW,
                &metadata,
                &mut Work::default()
            )
            .unwrap_err()
            .code(),
            "unsupported_execution_operation"
        );
    }
    assert_eq!(metadata.0.get(), 0);
    let control = super::super::super::tests::enabled();
    let source = get(
        get(&control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .first()
            .unwrap(),
        "effect",
    )
    .unwrap();
    let request = Request {
        operation: Operation::Enable,
        parameters: object(vec![]).unwrap(),
        expected_execution: DISABLED,
        expected_tasks: None,
        key: "enable",
    };
    assert_eq!(
        build(
            &control,
            None,
            &request,
            get(source, "source").unwrap(),
            NOW,
            &metadata,
            &mut Work(MAX_WORK)
        )
        .unwrap_err()
        .code(),
        "unsupported_execution_work_profile"
    );
}
