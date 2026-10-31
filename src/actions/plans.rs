//! Pure validation of the frozen `runner.launch` runtime_launch v1 plan.
use super::authority::Budget;
use crate::{Error, Result, error::require, values::Value};

const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;
const ENV_KEYS: [&str; 6] = ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TERM"];

pub(super) fn validate(plan: &Value, budget: &mut Budget) -> Result<()> {
    let encoded = bounded_spaced(plan, budget)?;
    fields(
        plan,
        &[
            "version",
            "operation",
            "root",
            "cwd",
            "namespace",
            "multiplexer",
            "agents",
            "env",
            "steps",
        ],
    )?;
    let expected = build(plan, budget)?;
    require(
        encoded == bounded_spaced(&expected, budget)?,
        "invalid_runtime_launch_plan",
    )
}

fn bounded_spaced(value: &Value, budget: &mut Budget) -> Result<Vec<u8>> {
    let encoded = budget.spaced(value)?;
    require(
        encoded.len() <= MAX_PLAN_BYTES,
        "runtime_launch_plan_too_large",
    )?;
    Ok(encoded)
}

fn get<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .as_object()
        .and_then(|object| object.get(key))
        .ok_or(Error("invalid_runtime_launch_plan"))
}

fn fields(value: &Value, keys: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or(Error("invalid_runtime_launch_plan"))?;
    require(
        object.entries().len() == keys.len() && keys.iter().all(|key| object.get(key).is_some()),
        "invalid_runtime_launch_plan",
    )
}

// Python str.strip additionally treats the ASCII information separators as space.
fn python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn text(value: &Value, nonblank: bool) -> Result<&str> {
    let value = value.as_str().ok_or(Error("invalid_runtime_launch_plan"))?;
    require(
        !value.contains('\0') && (!nonblank || value.chars().any(|c| !python_space(c))),
        "invalid_runtime_launch_plan",
    )?;
    Ok(value)
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Result<Value> {
    Value::object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn strings(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|value| string(value)).collect())
}

fn handle(name: &str) -> Result<Value> {
    object([("handle", string(name))])
}

fn run(argv: Vec<Value>, outputs: Option<Value>) -> Result<Value> {
    let mut entries = vec![
        ("operation".to_owned(), string("run")),
        ("argv".to_owned(), Value::Array(argv)),
    ];
    if let Some(outputs) = outputs {
        entries.push(("outputs".to_owned(), outputs));
    }
    Value::object(entries)
}

fn sleep() -> Result<Value> {
    object([
        ("operation", string("sleep")),
        ("seconds", Value::integer("5")?),
    ])
}

fn build(plan: &Value, budget: &mut Budget) -> Result<Value> {
    let root = text(get(plan, "root")?, true)?;
    require(root.starts_with('/'), "invalid_runtime_launch_plan")?;
    let namespace = text(get(plan, "namespace")?, true)?;
    let parts = namespace
        .strip_prefix("coven-")
        .and_then(|s| s.split_once('-'));
    require(
        parts.is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_hexdigit())
                && b.bytes().all(|c| c.is_ascii_hexdigit())
        }),
        "invalid_runtime_launch_plan",
    )?;
    let backend = text(get(plan, "multiplexer")?, true)?;
    require(
        matches!(backend, "tmux" | "herdr"),
        "invalid_runtime_launch_plan",
    )?;
    let agents = get(plan, "agents")?
        .as_array()
        .ok_or(Error("invalid_runtime_launch_plan"))?;
    require(
        (1..=32).contains(&agents.len()),
        "invalid_runtime_launch_plan",
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for agent in agents {
        fields(agent, &["id", "command", "starter"])?;
        let id = text(get(agent, "id")?, true)?;
        require(
            (1..=128).contains(&id.len())
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                && seen.insert(id),
            "invalid_runtime_launch_plan",
        )?;
        text(get(agent, "command")?, true)?;
        text(get(agent, "starter")?, true)?;
    }
    let env = get(plan, "env")?;
    for (key, value) in env
        .as_object()
        .ok_or(Error("invalid_runtime_launch_plan"))?
        .entries()
    {
        require(
            ENV_KEYS.contains(&key.as_str()),
            "invalid_runtime_launch_plan",
        )?;
        require(
            text(value, false)?.len() <= 32768,
            "invalid_runtime_launch_plan",
        )?;
    }

    // Charge retained data before cloning; charge each repeated payload before
    // constructing argv. The shared serializer also bounds/charges final output.
    budget.canonical(get(plan, "agents")?)?;
    budget.canonical(env)?;
    budget.charge(root.len() * 2 + namespace.len() + backend.len())?;
    let mut steps = Vec::with_capacity(3 + agents.len() * 5);
    let target = format!("={namespace}");
    if backend == "tmux" {
        steps.push(object([
            ("operation", string("ensure_absent")),
            ("argv", strings(&["tmux", "has-session", "-t", &target])),
        ])?);
        budget.charge(root.len() + namespace.len())?;
        steps.push(run(
            vec![
                string("tmux"),
                string("new-session"),
                string("-d"),
                string("-s"),
                string(namespace),
                string("-n"),
                string("orchestrator"),
                string("-c"),
                string(root),
                string("/bin/zsh"),
            ],
            None,
        )?);
    } else {
        steps.push(object([
            ("operation", string("ensure_absent")),
            ("argv", strings(&["herdr", "workspace", "list"])),
        ])?);
        budget.charge(root.len() + namespace.len())?;
        steps.push(run(
            vec![
                string("herdr"),
                string("workspace"),
                string("create"),
                string("--cwd"),
                string(root),
                string("--label"),
                string(namespace),
                string("--no-focus"),
            ],
            Some(object([
                (
                    "workspace",
                    strings(&["result", "workspace", "workspace_id"]),
                ),
                ("orchestrator", strings(&["result", "root_pane", "pane_id"])),
            ])?),
        )?);
        steps.push(run(
            vec![
                string("herdr"),
                string("pane"),
                string("run"),
                handle("orchestrator")?,
                string("/bin/zsh"),
            ],
            None,
        )?);
    }
    for (index, agent) in agents.iter().enumerate() {
        let id = text(get(agent, "id")?, true)?;
        let command = text(get(agent, "command")?, true)?;
        let starter = text(get(agent, "starter")?, true)?;
        budget.charge(
            root.len() + command.len() + starter.len() + id.len() * 4 + namespace.len() * 4,
        )?;
        let label = format!("agent-{id}");
        if backend == "tmux" {
            let pane = format!("={namespace}:={label}");
            steps.push(run(
                vec![
                    string("tmux"),
                    string("new-window"),
                    string("-d"),
                    string("-t"),
                    string(&target),
                    string("-n"),
                    string(&label),
                    string("-c"),
                    string(root),
                    string(command),
                ],
                None,
            )?);
            steps.push(sleep()?);
            steps.push(run(
                vec![
                    string("tmux"),
                    string("send-keys"),
                    string("-t"),
                    string(&pane),
                    string("-l"),
                    string("--"),
                    string(starter),
                ],
                None,
            )?);
            steps.push(run(
                vec![
                    string("tmux"),
                    string("send-keys"),
                    string("-t"),
                    string(&pane),
                    string("Enter"),
                ],
                None,
            )?);
        } else {
            let name = format!("agent_{index}");
            steps.push(run(
                vec![
                    string("herdr"),
                    string("tab"),
                    string("create"),
                    string("--workspace"),
                    handle("workspace")?,
                    string("--cwd"),
                    string(root),
                    string("--label"),
                    string(&label),
                    string("--no-focus"),
                ],
                Some(object([(
                    &name,
                    strings(&["result", "root_pane", "pane_id"]),
                )])?),
            )?);
            steps.push(run(
                vec![
                    string("herdr"),
                    string("pane"),
                    string("run"),
                    handle(&name)?,
                    string(command),
                ],
                None,
            )?);
            steps.push(sleep()?);
            steps.push(run(
                vec![
                    string("herdr"),
                    string("pane"),
                    string("send-text"),
                    handle(&name)?,
                    string(starter),
                ],
                None,
            )?);
            steps.push(run(
                vec![
                    string("herdr"),
                    string("pane"),
                    string("send-keys"),
                    handle(&name)?,
                    string("enter"),
                ],
                None,
            )?);
        }
    }
    object([
        ("version", Value::integer("1")?),
        ("operation", string("runtime_launch")),
        ("root", string(root)),
        ("cwd", string(root)),
        ("namespace", string(namespace)),
        ("multiplexer", string(backend)),
        ("agents", get(plan, "agents")?.clone()),
        ("env", env.clone()),
        ("steps", Value::Array(steps)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{self, Profile};

    fn budget() -> Budget {
        Budget::default()
    }

    fn parse(json: &str) -> Value {
        values::parse(json.as_bytes(), Profile::Authority).unwrap()
    }

    fn inputs(backend: &str, count: usize) -> Value {
        object([
            ("root", string("/not/read/../workspace//")),
            ("namespace", string("coven-aA1-B2")),
            ("multiplexer", string(backend)),
            ("agents", Value::Array((0..count).map(|index| {
                object([
                    ("id", string(&format!("agent_{index}"))),
                    ("command", string(" exec worker --prompt /mutable/prompt.md ")),
                    ("starter", string(" - literal 'quotes' \"double\" $HOME $(touch nope)\n\u{96ea} ")),
                ]).unwrap()
            }).collect())),
            ("env", object([("PATH", string("/usr/bin:/bin")), ("HOME", string("/frozen")), ("LANG", string("C.UTF-8"))]).unwrap()),
        ]).unwrap()
    }

    fn plan(backend: &str, count: usize) -> Value {
        build(&inputs(backend, count), &mut budget()).unwrap()
    }

    fn set(value: &mut Value, key: &str, replacement: Value) {
        value.set_field(key, replacement).unwrap();
    }

    fn edit(value: &mut Value, key: &str, change: impl FnOnce(&mut Value)) {
        let mut child = get(value, key).unwrap().clone();
        change(&mut child);
        set(value, key, child);
    }

    #[test]
    fn both_backends_preserve_order_and_literal_payloads_at_roster_limits() {
        for backend in ["tmux", "herdr"] {
            for count in [1, 32] {
                let plan = plan(backend, count);
                validate(&plan, &mut budget()).unwrap();
                assert_eq!(
                    text(get(&plan, "cwd").unwrap(), true).unwrap(),
                    "/not/read/../workspace//"
                );
                let steps = get(&plan, "steps").unwrap().as_array().unwrap();
                assert_eq!(
                    steps.len(),
                    if backend == "tmux" {
                        2 + count * 4
                    } else {
                        3 + count * 5
                    }
                );
                assert_eq!(
                    steps
                        .iter()
                        .filter(|s| get(s, "operation").unwrap().as_str() == Some("sleep"))
                        .count(),
                    count
                );
                assert_eq!(
                    get(&steps[steps.len() - 2], "argv")
                        .unwrap()
                        .as_array()
                        .unwrap()
                        .last()
                        .unwrap()
                        .as_str(),
                    Some(" - literal 'quotes' \"double\" $HOME $(touch nope)\n\u{96ea} ")
                );
                let final_create = &steps[steps.len() - if backend == "tmux" { 4 } else { 5 }];
                let argv = get(final_create, "argv").unwrap().as_array().unwrap();
                let label_index = if backend == "tmux" { 6 } else { 8 };
                assert_eq!(
                    argv[label_index].as_str(),
                    Some(format!("agent-agent_{}", count - 1).as_str())
                );
                if backend == "herdr" {
                    let outputs = get(final_create, "outputs").unwrap().as_object().unwrap();
                    assert!(outputs.get(&format!("agent_{}", count - 1)).is_some());
                }
            }
        }
    }

    #[test]
    fn python_frozen_tmux_vector_and_exact_sorted_spaced_bytes() {
        // Literal build_plan('/', 'coven-a-B', 'tmux',
        //   [{'id':'x','command':'worker','starter':'go'}], {}) result.
        let golden = r#"{"agents": [{"command": "worker", "id": "x", "starter": "go"}], "cwd": "/", "env": {}, "multiplexer": "tmux", "namespace": "coven-a-B", "operation": "runtime_launch", "root": "/", "steps": [{"argv": ["tmux", "has-session", "-t", "=coven-a-B"], "operation": "ensure_absent"}, {"argv": ["tmux", "new-session", "-d", "-s", "coven-a-B", "-n", "orchestrator", "-c", "/", "/bin/zsh"], "operation": "run"}, {"argv": ["tmux", "new-window", "-d", "-t", "=coven-a-B", "-n", "agent-x", "-c", "/", "worker"], "operation": "run"}, {"operation": "sleep", "seconds": 5}, {"argv": ["tmux", "send-keys", "-t", "=coven-a-B:=agent-x", "-l", "--", "go"], "operation": "run"}, {"argv": ["tmux", "send-keys", "-t", "=coven-a-B:=agent-x", "Enter"], "operation": "run"}], "version": 1}"#;
        let frozen = parse(golden);
        validate(&frozen, &mut budget()).unwrap();
        let expected = build(&frozen, &mut budget()).unwrap();
        assert_eq!(budget().spaced(&expected).unwrap(), golden.as_bytes());
    }

    #[test]
    fn python_frozen_herdr_vector() {
        let frozen = parse(
            r#"{"version":1,"operation":"runtime_launch","root":"/","cwd":"/","namespace":"coven-a-B","multiplexer":"herdr","agents":[{"id":"x","command":"worker","starter":"go"}],"env":{},"steps":[{"operation":"ensure_absent","argv":["herdr","workspace","list"]},{"operation":"run","argv":["herdr","workspace","create","--cwd","/","--label","coven-a-B","--no-focus"],"outputs":{"workspace":["result","workspace","workspace_id"],"orchestrator":["result","root_pane","pane_id"]}},{"operation":"run","argv":["herdr","pane","run",{"handle":"orchestrator"},"/bin/zsh"]},{"operation":"run","argv":["herdr","tab","create","--workspace",{"handle":"workspace"},"--cwd","/","--label","agent-x","--no-focus"],"outputs":{"agent_0":["result","root_pane","pane_id"]}},{"operation":"run","argv":["herdr","pane","run",{"handle":"agent_0"},"worker"]},{"operation":"sleep","seconds":5},{"operation":"run","argv":["herdr","pane","send-text",{"handle":"agent_0"},"go"]},{"operation":"run","argv":["herdr","pane","send-keys",{"handle":"agent_0"},"enter"]}]}"#,
        );
        validate(&frozen, &mut budget()).unwrap();
        assert_eq!(
            budget()
                .spaced(&build(&frozen, &mut budget()).unwrap())
                .unwrap(),
            budget().spaced(&frozen).unwrap()
        );
    }

    #[test]
    fn rejects_noncanonical_fields_steps_numbers_and_handles() {
        for backend in ["tmux", "herdr"] {
            for (key, value) in [
                ("version", Value::Bool(true)),
                ("version", Value::float(1.0).unwrap()),
                ("version", Value::integer("2").unwrap()),
                ("operation", string("task_dispatch")),
                ("cwd", string("/elsewhere")),
                ("hidden_program", string("evil")),
                ("steps", Value::Array(vec![])),
            ] {
                let mut p = plan(backend, 1);
                set(&mut p, key, value);
                assert!(validate(&p, &mut budget()).is_err(), "{backend}: {key}");
            }
            for mutation in 0..8 {
                let mut p = plan(backend, 2);
                edit(&mut p, "steps", |steps| {
                    let steps = steps.array_mut().unwrap();
                    match mutation {
                        0 => steps.reverse(),
                        1 => edit(&mut steps[1], "argv", |argv| {
                            argv.array_mut().unwrap().push(string("evil"))
                        }),
                        2 => {
                            let index = if backend == "tmux" { 3 } else { 5 };
                            set(&mut steps[index], "seconds", Value::float(5.0).unwrap());
                        }
                        3 => set(&mut steps[1], "outputs", parse(r#"{"evil":["command"]}"#)),
                        4 => set(&mut steps[0], "operation", string("ensure_present")),
                        5 => edit(&mut steps[2], "argv", |argv| {
                            argv.array_mut().unwrap()[3] =
                                parse(r#"{"handle":"wrong","extra":18446744073709551617}"#)
                        }),
                        6 => {
                            let len = steps.len();
                            steps.swap(len - 1, len - 2);
                        }
                        _ => set(&mut steps[0], "extra", Value::Bool(false)),
                    }
                });
                assert!(
                    validate(&p, &mut budget()).is_err(),
                    "{backend}: mutation {mutation}"
                );
            }
            let mut p = plan(backend, 2);
            edit(&mut p, "agents", |agents| {
                agents.array_mut().unwrap().reverse()
            });
            assert!(validate(&p, &mut budget()).is_err());
        }
        assert!(validate(&Value::Null, &mut budget()).is_err());
        let p = plan("tmux", 1);
        let entries = p
            .as_object()
            .unwrap()
            .entries()
            .iter()
            .filter(|(key, _)| key != "version")
            .cloned()
            .collect();
        assert!(validate(&Value::object(entries).unwrap(), &mut budget()).is_err());
    }

    #[test]
    fn input_constraints_and_python_unicode_blank_semantics() {
        for (key, value) in [
            ("root", string("relative")),
            ("root", string("")),
            ("root", string(" \u{3000}")),
            ("root", string("/bad\0path")),
            ("root", Value::Bool(true)),
            ("namespace", string("coven-human")),
            ("namespace", string("coven-\u{ff11}-ab")),
            ("namespace", string("coven--a")),
            ("namespace", string("coven-a-b-c")),
            ("multiplexer", string("screen")),
            ("agents", Value::Null),
            ("env", Value::Array(vec![])),
        ] {
            let mut input = inputs("tmux", 1);
            set(&mut input, key, value);
            assert!(build(&input, &mut budget()).is_err(), "{key}");
        }
        for count in [0, 33] {
            assert!(build(&inputs("tmux", count), &mut budget()).is_err());
        }
        for (key, value) in [
            ("id", string("")),
            ("id", string(&"a".repeat(129))),
            ("id", string("alice:other")),
            ("id", string("\u{96ea}")),
            (
                "command",
                string("\u{85}\u{a0}\u{1c}\u{1d}\u{1e}\u{1f}\u{2003}\u{2028}\u{3000}"),
            ),
            ("starter", string("")),
            ("starter", string("a\0b")),
            ("command", string("a\0b")),
            ("starter", Value::Bool(true)),
            ("command", Value::Null),
            ("extra", string("hidden")),
        ] {
            let mut input = inputs("tmux", 1);
            edit(&mut input, "agents", |agents| {
                set(&mut agents.array_mut().unwrap()[0], key, value)
            });
            assert!(build(&input, &mut budget()).is_err(), "{key}");
        }
        let mut duplicate = inputs("herdr", 2);
        edit(&mut duplicate, "agents", |agents| {
            let agents = agents.array_mut().unwrap();
            agents[1] = agents[0].clone();
        });
        assert!(build(&duplicate, &mut budget()).is_err());
        for backend in ["tmux", "herdr"] {
            let mut input = inputs(backend, 1);
            edit(&mut input, "agents", |agents| {
                let a = &mut agents.array_mut().unwrap()[0];
                set(a, "id", string(&"a".repeat(128)));
                set(a, "command", string(" \u{200b} "));
                set(a, "starter", string("\u{feff}"));
            });
            validate(&build(&input, &mut budget()).unwrap(), &mut budget()).unwrap();
        }
    }

    #[test]
    fn environment_allowlist_nul_and_utf8_byte_limits() {
        for key in ENV_KEYS {
            for value in [
                "".to_owned(),
                " \n\u{1c}".to_owned(),
                "\u{e9}".repeat(16384),
            ] {
                let mut input = inputs("herdr", 1);
                set(&mut input, "env", object([(key, string(&value))]).unwrap());
                validate(&build(&input, &mut budget()).unwrap(), &mut budget()).unwrap();
            }
        }
        for env in [
            object([("TOKEN", string("secret"))]).unwrap(),
            object([("path", string("/bin"))]).unwrap(),
            object([("PATH", string("a\0b"))]).unwrap(),
            object([("PATH", string(&"x".repeat(32769)))]).unwrap(),
            object([("LANG", string(&format!("{}x", "\u{e9}".repeat(16384))))]).unwrap(),
            object([("HOME", Value::Null)]).unwrap(),
        ] {
            let mut input = inputs("tmux", 1);
            set(&mut input, "env", env);
            assert!(build(&input, &mut budget()).is_err());
        }
    }

    #[test]
    fn sorted_spaced_size_limit_is_inclusive() {
        // Eight bounded strings, not one string outside the shared value profile.
        let values = vec![string(&"x".repeat(1024 * 1024 - 4)); 8];
        let mut boundary = Value::Array(values);
        // Array overhead is 32 bytes (quotes, commas and spaces), giving 8 MiB.
        assert_eq!(
            bounded_spaced(&boundary, &mut budget()).unwrap().len(),
            MAX_PLAN_BYTES
        );
        boundary.array_mut().unwrap()[0] = string(&"x".repeat(1024 * 1024 - 3));
        assert_eq!(
            bounded_spaced(&boundary, &mut budget()).unwrap_err().code(),
            "runtime_launch_plan_too_large"
        );
        assert_eq!(
            validate(&boundary, &mut budget()).unwrap_err().code(),
            "runtime_launch_plan_too_large"
        );
    }

    #[test]
    fn expected_plan_size_and_shared_budget_are_also_enforced() {
        let mut input = inputs("tmux", 32);
        set(
            &mut input,
            "root",
            string(&format!("/{}", "r".repeat(250_000))),
        );
        set(&mut input, "version", Value::integer("1").unwrap());
        set(&mut input, "operation", string("runtime_launch"));
        let root = get(&input, "root").unwrap().clone();
        set(&mut input, "cwd", root);
        set(&mut input, "steps", Value::Array(vec![]));
        bounded_spaced(&input, &mut budget()).unwrap();
        assert_eq!(
            validate(&input, &mut budget()).unwrap_err().code(),
            "runtime_launch_plan_too_large"
        );

        let p = plan("herdr", 1);
        let mut exhausted = budget();
        exhausted.charge(256 * 1024 * 1024).unwrap();
        assert_eq!(
            validate(&p, &mut exhausted).unwrap_err().code(),
            "unsupported_action_work_profile"
        );
    }

    #[test]
    fn valid_large_repeated_agent_payloads_stay_within_shared_string_profile() {
        for backend in ["tmux", "herdr"] {
            let mut input = inputs(backend, 32);
            edit(&mut input, "agents", |agents| {
                for agent in agents.array_mut().unwrap() {
                    set(agent, "command", string(&"c".repeat(60_000)));
                    set(agent, "starter", string(&"s".repeat(60_000)));
                }
            });
            let p = build(&input, &mut budget()).unwrap();
            validate(&p, &mut budget()).unwrap();
            edit(&mut input, "agents", |agents| {
                for agent in agents.array_mut().unwrap() {
                    set(agent, "starter", string(&"s".repeat(80_000)));
                }
            });
            let p = build(&input, &mut budget()).unwrap();
            assert_eq!(
                validate(&p, &mut budget()).unwrap_err().code(),
                "runtime_launch_plan_too_large"
            );
        }
    }
}
