use super::*;
use crate::runtime_command::{Output, Request};
use serde_json::{Value as Json, json};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    process::{Command, Stdio},
};

fn native(value: &Json) -> Value {
    values::parse(&serde_json::to_vec(value).unwrap(), Profile::Authority).unwrap()
}
fn json(value: &Value) -> Json {
    serde_json::from_slice(&value.canonical().unwrap()).unwrap()
}
fn inputs(backend: &str, count: usize) -> Json {
    json!({"root":"/not/read/../workspace with spaces//", "namespace":"coven-aA1-B2", "multiplexer":backend,
        "agents":(0..count).map(|i| json!({"id":format!("agent_{i}"), "command":" exec worker --prompt /mutable/prompt.md ",
            "starter":format!("- literal 'quotes' \"double\" $HOME $(touch nope)\n\u{96ea} {i}")})).collect::<Vec<_>>(),
        "environment":{"PATH":"/usr/bin:/bin", "HOME":"/frozen", "LANG":"C.UTF-8", "TERM":"", "SHELL":"/bin/zsh", "LC_ALL":" "}})
}
fn plan(input: &Json) -> Value {
    try_plan(input).unwrap()
}
fn try_plan(input: &Json) -> Result<Value> {
    build_plan(
        input["root"].as_str().unwrap(),
        input["namespace"].as_str().unwrap(),
        input["multiplexer"].as_str().unwrap(),
        &native(&input["agents"]),
        &native(&input["environment"]),
    )
}
struct Fake<'a> {
    case: &'a Json,
    calls: RefCell<Vec<Json>>,
    tabs: Cell<usize>,
}
impl Runner for Fake<'_> {
    fn run(&self, request: &Request) -> Output {
        let index = self.calls.borrow().len();
        assert_eq!(request.max_output_bytes, MAX_OUTPUT);
        assert!(!request.timeout.is_zero() && request.timeout <= Duration::from_secs(60));
        assert!(Path::new(&request.argv[0]).is_absolute());
        assert_eq!(
            Path::new(&request.argv[0]).parent(),
            Some(Path::new("/injected"))
        );
        let mut lexical_argv = request.argv.clone();
        lexical_argv[0] = Path::new(&request.argv[0])
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        // Python exposes the logical budget, while native candidate requests get
        // the remaining budget. Exact deadline consumption is tested separately.
        self.calls
            .borrow_mut()
            .push(json!({"argv":lexical_argv,"cwd":request.cwd,"env":request.env,"timeout":60}));
        if let Some(replacement) = self.case["overrides"].get(index.to_string()) {
            let status = match replacement["status"].as_str() {
                Some("Timeout") => Status::Timeout,
                Some("SpawnError") => Status::SpawnError,
                Some("OutputLimit") => Status::OutputLimit,
                _ => Status::Exited(replacement["status"].as_i64().unwrap_or(0) as i32),
            };
            return Output {
                status,
                stdout: replacement["stdout"]
                    .as_str()
                    .unwrap_or("{}")
                    .as_bytes()
                    .to_vec(),
                stderr: replacement["stderr"]
                    .as_str()
                    .unwrap_or("")
                    .as_bytes()
                    .to_vec(),
            };
        }
        let argv = lexical_argv.iter().map(String::as_str).collect::<Vec<_>>();
        let mut payload = json!({});
        if argv.starts_with(&["tmux", "has-session"]) {
            return Output {
                status: Status::Exited(1),
                stdout: vec![],
                stderr: format!(
                    "can't find session: {}",
                    self.case["inputs"]["namespace"].as_str().unwrap()
                )
                .into_bytes(),
            };
        }
        if argv.starts_with(&["herdr", "workspace", "list"]) {
            payload = json!({"result":{"workspaces":self.case.get("workspaces").cloned().unwrap_or(json!([]))}});
        }
        if argv.starts_with(&["herdr", "workspace", "create"]) {
            payload = json!({"result":{"workspace":{"workspace_id":"ws_1"},"root_pane":{"pane_id":"root-1"}}});
        }
        if argv.starts_with(&["herdr", "tab", "create"]) {
            self.tabs.set(self.tabs.get() + 1);
            payload =
                json!({"result":{"root_pane":{"pane_id":format!("pane_{}",self.tabs.get())}}});
        }
        Output {
            status: Status::Exited(0),
            stdout: serde_json::to_vec(&payload).unwrap(),
            stderr: vec![],
        }
    }
}

// Transcript parity isolates transport from discovery. Selector tests below use
// real frozen paths through run_with and assert the exact absolute target.
fn run_transcript(
    plan: &Value,
    runner: &dyn Runner,
    sleeper: &mut dyn FnMut(Duration) -> TransportResult<()>,
    stopping: bool,
) -> Value {
    run_resolving_with(plan, runner, sleeper, stopping, &|name, _, _, _| {
        Box::new(std::iter::once(Ok(format!("/injected/{name}"))))
    })
}
fn evaluate(case: &Json) -> Json {
    let Ok(mut p) = try_plan(&case["inputs"]) else {
        return json!({"build_error":true});
    };
    if case["invalid_plan"] == true {
        p.set_field("steps", Value::Array(vec![])).unwrap();
    }
    let fake = Fake {
        case,
        calls: RefCell::new(vec![]),
        tabs: Cell::new(0),
    };
    let mut delays = vec![];
    let report = run_transcript(
        &p,
        &fake,
        &mut |duration| {
            delays.push(duration.as_secs());
            if case["sleep_error"] == true
                || case["sleep_failure_at"].as_u64() == Some(delays.len() as u64 - 1)
            {
                Err(("RuntimeError", "delay failed".into()))
            } else {
                Ok(())
            }
        },
        case["stopping"] == true,
    );
    json!({"plan":json(&p), "report":json(&report), "calls":fake.calls.into_inner(), "delays":delays})
}
fn compare(cases: Vec<Json>, exact_diagnostics: bool) {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut oracle = Command::new("python3")
        .arg("-B")
        .arg(format!("{root}/src/launch/oracle.py"))
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = oracle.stdin.take().unwrap();
    let input = serde_json::to_vec(&cases).unwrap();
    // Feed concurrently: oracle output can exceed the pipe capacity for 32-agent plans.
    let writer = std::thread::spawn(move || {
        stdin.write_all(&input).unwrap();
    });
    let output = oracle.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<Json> = serde_json::from_slice(&output.stdout).unwrap();
    for (index, (case, mut expected)) in cases.iter().zip(expected).enumerate() {
        let mut actual = evaluate(case);
        if !exact_diagnostics {
            // Parser wording is implementation-specific; effects and error class remain parity obligations.
            for value in [&mut actual, &mut expected] {
                if let Some(diagnostics) = value["report"]["diagnostics"].as_array_mut() {
                    for diagnostic in diagnostics {
                        diagnostic.as_object_mut().unwrap().remove("message");
                    }
                }
            }
        }
        assert_eq!(actual, expected, "case {index}: {case}");
    }
}

#[test]
fn oracle_builder_validation_and_literal_boundaries() {
    let mut cases = vec![];
    for backend in ["tmux", "herdr"] {
        for (key, value) in [
            ("root", json!("relative")),
            ("root", json!("/bad\0path")),
            ("namespace", json!("coven-human")),
            ("namespace", json!("coven-\u{ff11}-ab")),
            ("multiplexer", json!("screen")),
            ("agents", json!([])),
            ("agents", json!(null)),
            ("environment", json!([])),
            ("environment", json!({"TOKEN":"secret"})),
            ("environment", json!({"PATH":"a\0b"})),
            ("environment", json!({"PATH":"x".repeat(32769)})),
        ] {
            let mut input = inputs(backend, 1);
            input[key] = value;
            cases.push(json!({"inputs":input}));
        }
        for (key, value) in [
            ("id", json!("alice:other")),
            ("id", json!("x".repeat(129))),
            ("command", json!("\u{1c}\u{a0}\u{3000}")),
            ("starter", json!("a\0b")),
            ("extra", json!("hidden")),
        ] {
            let mut input = inputs(backend, 1);
            input["agents"][0][key] = value;
            cases.push(json!({"inputs":input}));
        }
        let mut duplicate = inputs(backend, 2);
        duplicate["agents"][1] = duplicate["agents"][0].clone();
        cases.push(json!({"inputs":duplicate}));
        cases.push(json!({"inputs":inputs(backend,33)}));
        for environment in [
            json!({"LANG":"\u{e9}".repeat(16384)}),
            json!({"LANG":format!("{}x","\u{e9}".repeat(16384))}),
        ] {
            let mut input = inputs(backend, 1);
            input["environment"] = environment;
            cases.push(json!({"inputs":input}));
        }
    }
    compare(cases, true);
}

#[test]
fn oracle_deterministic_inputs_roster_environment_and_required_failures() {
    let mut cases = vec![];
    for backend in ["tmux", "herdr"] {
        for count in [1, 2, 32] {
            let mut case = json!({"inputs":inputs(backend,count)});
            cases.push(case.clone());
            case["inputs"]["environment"] = json!({});
            cases.push(case);
        }
        let input = inputs(backend, 2);
        let p = json(&plan(&input));
        let commands = p["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["operation"] != "sleep")
            .count();
        for index in 1..commands {
            cases.push(json!({"inputs":input,"overrides":{index.to_string():{"status":9,"stderr":"failed\n"}}}));
        }
        for status in ["Timeout", "SpawnError"] {
            for index in 0..commands {
                cases.push(
                    json!({"inputs":input,"overrides":{index.to_string():{"status":status}}}),
                );
            }
        }
        cases.push(json!({"inputs":input,"sleep_error":true}));
        cases.push(json!({"inputs":input,"sleep_failure_at":1}));
        cases.push(json!({"inputs":input,"invalid_plan":true}));
        cases.push(json!({"inputs":input,"stopping":true,"invalid_plan":true}));
    }
    compare(cases, true);
}

#[test]
fn oracle_tmux_presence_and_stop_attempts() {
    let mut cases = vec![];
    for stopping in [false, true] {
        for code in [0, 1, 2] {
            for detail in [
                "",
                "can't find session: coven-aA1-B2",
                "can't find session: =coven-aA1-B2",
                "no server running on /tmp/tmux-test",
                "ERROR connecting to /tmp/tmux-test (No such file or directory)",
                "failed to connect to /tmp/tmux-test (No such file or directory)",
                "error connecting to tmux socket (Permission denied)",
                "unexpected socket failure",
                "\u{1c}can't find session: coven-aA1-B2\u{1f}",
            ] {
                cases.push(json!({"inputs":inputs("tmux",1),"stopping":stopping,"overrides":{"0":{"status":code,"stderr":detail}}}));
            }
        }
    }
    for backend in ["tmux", "herdr"] {
        cases.push(json!({"inputs":inputs(backend,1),"stopping":true}));
        for status in [json!(0), json!(7), json!("Timeout"), json!("SpawnError")] {
            let mut case = json!({"inputs":inputs(backend,1),"stopping":true,"workspaces":[{"label":"coven-aA1-B2","workspace_id":"scoped-id"},{"label":"human","workspace_id":"legacy"}],"overrides":{"1":{"status":status}}});
            if backend == "tmux" {
                case["overrides"]["0"] = json!({"status":0});
            }
            cases.push(case);
        }
    }
    compare(cases, true);
}

#[test]
fn oracle_untrusted_lists_handles_and_partial_observations() {
    let input = inputs("herdr", 1);
    let mut exact = vec![];
    for workspaces in [
        json!([{"label":"coven-aA1-B2"}]),
        json!([{"label":"coven-aA1-B2","workspace_id":"bad;id"}]),
        json!([{"label":"coven-aA1-B2","workspace_id":"one"},{"label":"coven-aA1-B2","workspace_id":"two"}]),
        json!([{"label":"human-name","workspace_id":"legacy"}]),
    ] {
        for stopping in [false, true] {
            exact.push(json!({"inputs":input,"stopping":stopping,"workspaces":workspaces}));
        }
    }
    for bad in [
        Json::Null,
        json!(""),
        json!("$(evil)"),
        json!("a/b"),
        json!("x".repeat(129)),
        json!("\u{96ea}"),
        json!(123),
    ] {
        for index in [1, 3] {
            let payload = if index == 1 {
                json!({"result":{"workspace":{"workspace_id":bad},"root_pane":{"pane_id":"root"}}})
            } else {
                json!({"result":{"root_pane":{"pane_id":bad}}})
            };
            exact.push(json!({"inputs":input,"overrides":{index.to_string():{"stdout":payload.to_string()}}}));
        }
    }
    for (index, payload) in [
        (1, json!({})),
        (
            1,
            json!({"result":{"workspace":{"workspace_id":"observed"}}}),
        ),
        (3, json!({"result":{}})),
    ] {
        exact.push(
            json!({"inputs":input,"overrides":{index.to_string():{"stdout":payload.to_string()}}}),
        );
    }
    for stdout in [
        "{\"result\":{\"workspaces\":null}}",
        "{\"result\":{\"workspaces\":[1]}}",
        "{\"result\":{\"workspaces\":[],\"workspaces\":[]}}",
        "{\"result\":{\"workspaces\":[]},\"extra\":NaN}",
        "{\"result\":{\"workspaces\":[]},\"extra\":Infinity}",
        "{\"result\":{\"workspaces\":[]},\"extra\":1e999}",
    ] {
        exact.push(json!({"inputs":input,"overrides":{"0":{"stdout":stdout}}}));
    }
    for stdout in [
        "{\"result\":{\"workspaces\":[]},\"extra\":-Infinity}",
        "[]",
        "null",
        "1",
        "true",
        "1.2",
        "\"string\"",
        "{\"result\":null}",
    ] {
        exact.push(json!({"inputs":input,"overrides":{"0":{"stdout":stdout}}}));
    }
    compare(exact, true);
    let mut malformed = vec![];
    for index in [0, 1, 3] {
        for stdout in ["{", "", "[] trailing", "{\"result\":"] {
            malformed
                .push(json!({"inputs":input,"overrides":{index.to_string():{"stdout":stdout}}}));
        }
    }
    compare(malformed, false);
}

#[test]
fn caller_mutation_cannot_change_independent_validated_plan() {
    let mut original = plan(&inputs("herdr", 2));
    let frozen = validate_plan(&original).unwrap();
    original
        .set_field("env", native(&json!({"HOME":"/changed"})))
        .unwrap();
    original.set_field("agents", native(&json!([]))).unwrap();
    assert!(validate_plan(&original).is_err());
    let case = json!({"inputs":inputs("herdr",2)});
    let fake = Fake {
        case: &case,
        calls: RefCell::new(vec![]),
        tabs: Cell::new(0),
    };
    let report = run_transcript(&frozen, &fake, &mut |_| Ok(()), false);
    assert_eq!(json(&report)["status"], "transport_completed");
    for call in fake.calls.borrow().iter() {
        assert_eq!(call["env"]["HOME"], "/frozen");
    }
    assert!(fake.calls.borrow().iter().any(|call| {
        call["argv"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("\u{96ea} 1")
    }));
}

#[test]
fn explicit_output_bounds_unknown_effects_and_unicode_error_truncation() {
    for index in [0, 1, 4] {
        let case = json!({"inputs":inputs("herdr",1),"overrides":{index.to_string():{"status":"OutputLimit"}}});
        let value = evaluate(&case);
        assert_eq!(value["report"]["outcome_unknown"], true);
        assert_eq!(value["report"]["completed_steps"], index);
        assert_eq!(value["calls"].as_array().unwrap().len(), index + 1);
    }
    let case =
        json!({"inputs":inputs("herdr",1),"overrides":{"3":{"stdout":"x".repeat(MAX_OUTPUT+1)}}});
    let value = evaluate(&case);
    assert_eq!(value["report"]["handles"]["workspace"], "ws_1");
    assert_eq!(value["report"]["outcome_unknown"], true);
    assert_eq!(value["report"]["diagnostics"][0]["error"], "OutputLimit");
    for (stderr, completed) in [("x", true), ("xx", false)] {
        let case = json!({"inputs":inputs("herdr",1),"overrides":{"4":{"stdout":"x".repeat(MAX_OUTPUT-1),"stderr":stderr}}});
        let value = evaluate(&case);
        assert_eq!(
            value["report"]["status"] == "transport_completed",
            completed
        );
        assert_eq!(value["report"]["handles"]["agent_0"], "pane_1");
        if !completed {
            assert_eq!(value["report"]["outcome_unknown"], true);
        }
    }
    for backend in ["tmux", "herdr"] {
        compare(
            vec![
                json!({"inputs":inputs(backend,1),"overrides":{"1":{"status":7,"stderr":"\u{1f980}".repeat(3000)}}}),
            ],
            true,
        );
    }
    let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let case = json!({"inputs":inputs("herdr",1),"overrides":{"1":{"stdout":nested}}});
    let value = evaluate(&case);
    assert_eq!(
        value["report"]["diagnostics"][0]["error"],
        "UnsupportedOutputProfile"
    );
    assert_eq!(value["report"]["outcome_unknown"], true);
}

#[test]
fn invalid_native_entry_points_return_complete_reports_without_running() {
    for (report, stopping) in [(execute(&Value::Null), false), (stop(&Value::Null), true)] {
        let report = json(&report);
        assert_eq!(report["status"], "transport_failed_or_partial");
        assert_eq!(report["completed_steps"], 0);
        assert_eq!(report["handles"], json!({}));
        assert_eq!(report["diagnostics"][0]["step"], Json::Null);
        assert_eq!(report["outcome_unknown"], false);
        if stopping {
            assert_eq!(report["stop_command_attempted"], false);
            assert_eq!(report["stop_command_succeeded"], false);
        }
    }
}
