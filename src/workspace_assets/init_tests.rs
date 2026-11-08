use super::*;
use std::process::Command;

#[test]
fn python_exact_init_bytes_all_roles_names_unicode_examples_and_blank_defaults() {
    let expected: serde_json::Value = serde_json::from_slice(&super::super::tests::run(
        Command::new("python3")
            .args(["-B", "-c", include_str!("init_oracle.py")])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("PYTHONDONTWRITEBYTECODE", "1"),
    ))
    .unwrap();
    for case in expected["teams"].as_array().unwrap() {
        let agents: Vec<String> = case["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|agent| agent.as_str().unwrap().to_owned())
            .collect();
        let display_names = values::parse(
            &serde_json::to_vec(&case["display_names"]).unwrap(),
            values::Profile::LegacySnapshot,
        )
        .unwrap();
        let text = team_toml(
            case["name"].as_str().unwrap(),
            &agents,
            case["command"].as_str().unwrap(),
            case["multiplexer"].as_str().unwrap(),
            &display_names,
        )
        .unwrap();
        assert_eq!(text, case["text"]);
        text.parse::<toml::Value>().unwrap();
    }
    for (agent, text) in expected["agents"].as_object().unwrap() {
        assert_eq!(agent_state_text(agent), *text);
    }
    assert_eq!(default_goal(), expected["defaults"]["goal"]);
    assert_eq!(default_work(), expected["defaults"]["work"]);
    assert_eq!(default_output(), expected["defaults"]["output"]);
    let checkpoints = checkpoint_files();
    assert_eq!(
        checkpoints.len(),
        expected["checkpoints"].as_object().unwrap().len()
    );
    for (path, text) in checkpoints {
        assert_eq!(text, expected["checkpoints"][path]);
    }
    let example = init_example(" Interactive Demo ").unwrap();
    let golden = &expected["example"];
    assert_eq!(example.id, golden["id"]);
    assert_eq!(example.seed_message, golden["seed_message"]);
    assert_eq!(example.goal, golden["goal"]);
    assert_eq!(example.spec, golden["spec"]);
    assert_eq!(example.work, golden["work"]);
    assert_eq!(example.output, golden["output"]);
    assert_eq!(example.context, golden["context"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&example.tasks.compact().unwrap()).unwrap(),
        golden["tasks"]
    );
    assert_eq!(
        super::super::example_text(Some("interactive-demo")).unwrap(),
        example.context
    );
    // Raw owners are intentionally left for the init service to normalize.
    assert_eq!(
        example.tasks.as_array().unwrap()[0]
            .as_object()
            .unwrap()
            .get("owner")
            .unwrap()
            .as_str(),
        Some("Clippy")
    );
}

#[test]
fn example_selectors_are_slug_normalized_and_never_paths() {
    for selector in [
        "interactive-demo",
        "INTERACTIVE DEMO",
        "\u{1c}Interactive Demo\u{1f}",
        "../interactive-demo",
    ] {
        assert_eq!(init_example(selector).unwrap().id, "interactive-demo");
    }
    for selector in [
        "",
        " ",
        "___",
        "---",
        "😀",
        "\0",
        "interactive_demo",
        "examples/interactive-demo",
        "interactive-demo/goal.md",
        "/tmp/interactive-demo",
        "other-example",
        "interactive-demo\0suffix",
    ] {
        assert_eq!(
            init_example(selector).err().unwrap().code(),
            "unsupported_init_example_profile",
            "{selector:?}"
        );
    }
}

#[test]
fn toml_limit_counts_escaping_utf8_roster_and_exact_final_bytes() {
    let names = values::Value::object(vec![]).unwrap();
    let empty_size = team_toml("", &[], "", "", &names).unwrap().len();
    let exact = "x".repeat(1_048_576 - empty_size);
    assert_eq!(
        team_toml(&exact, &[], "", "", &names).unwrap().len(),
        1_048_576
    );
    for (name, agents, command, multiplexer, display_names) in [
        (
            format!("{exact}x"),
            vec![],
            String::new(),
            String::new(),
            names.clone(),
        ),
        (
            "\0".repeat(180_000),
            vec![],
            String::new(),
            String::new(),
            names.clone(),
        ),
        (
            "😀".repeat(270_000),
            vec![],
            String::new(),
            String::new(),
            names.clone(),
        ),
        (
            String::new(),
            vec!["clippy".into(); 10_000],
            String::new(),
            String::new(),
            names.clone(),
        ),
        (
            String::new(),
            vec!["x".repeat(1_048_577)],
            String::new(),
            String::new(),
            names.clone(),
        ),
        (
            String::new(),
            vec![],
            "x".repeat(1_048_577),
            String::new(),
            names.clone(),
        ),
        (
            String::new(),
            vec![],
            String::new(),
            "x".repeat(1_048_577),
            names.clone(),
        ),
        (
            String::new(),
            vec!["clippy".into()],
            String::new(),
            String::new(),
            values::Value::object(vec![(
                "clippy".into(),
                values::Value::String("\0".repeat(180_000)),
            )])
            .unwrap(),
        ),
    ] {
        assert_eq!(
            team_toml(&name, &agents, &command, &multiplexer, &display_names)
                .unwrap_err()
                .code(),
            "unsupported_init_template_profile"
        );
    }
    for names in [
        values::Value::Null,
        values::Value::object(vec![("clippy".into(), values::Value::Bool(true))]).unwrap(),
    ] {
        assert_eq!(
            team_toml("", &["clippy".into()], "", "", &names)
                .unwrap_err()
                .code(),
            "unsupported_init_template_profile"
        );
    }
}
