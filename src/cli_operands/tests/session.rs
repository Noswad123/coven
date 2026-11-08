use super::super::super::{Handoff, Session, run};
use super::*;

fn session_args(operation: &str) -> Vec<OsString> {
    let mut values = vec!["coven", "session", operation];
    if operation != "prompt-capabilities" {
        values.push("/missing/control");
    }
    match operation {
        "list" | "inspect" | "watch" => {
            values.extend(["--socket", "/missing/socket"]);
            if operation == "inspect" {
                values.extend(["--pane", "w1:p1"]);
            }
        }
        "start" | "resume" => {
            values.extend([
                "--runtime",
                "djinn",
                "--executable",
                "/missing/djinn",
                "--socket",
                "/missing/socket",
                "--pane",
                "w1:p1",
                "--expected-terminal",
                "terminal-1",
            ]);
            if operation == "resume" {
                values.extend(["--session", "literal ' ; $(selector)"]);
            }
        }
        "prompt" | "prompt-capabilities" => {
            values.extend(["--native-socket", "/missing/native"]);
            if operation == "prompt" {
                values.extend([
                    "--expected-runtime",
                    "6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0",
                    "--session",
                    "ses_fixture",
                    "--request-id",
                    "832492cf-947d-4ba4-8fa6-9c43a347a81b",
                    "--prompt-file",
                    "/missing/prompt",
                ]);
            }
        }
        _ => unreachable!(),
    }
    args(&values)
}

const OPERATIONS: &[&str] = &[
    "list",
    "inspect",
    "watch",
    "start",
    "resume",
    "prompt",
    "prompt-capabilities",
];

#[test]
fn seven_routes_have_exact_flags_types_and_defaults() {
    for operation in OPERATIONS {
        let (_, matches) = parse(&session_args(operation)).unwrap();
        let leaf = matches
            .subcommand_matches("session")
            .unwrap()
            .subcommand_matches(operation)
            .unwrap();
        assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 3.0);
        assert!(!leaf.get_one::<bool>("json").unwrap());
        if *operation != "prompt-capabilities" {
            assert_eq!(
                leaf.get_one::<PathBuf>("workspace").unwrap(),
                &PathBuf::from("/missing/control")
            );
        }
        if ["list", "inspect", "watch"].contains(operation) {
            assert_eq!(leaf.get_one::<String>("runtime").unwrap(), "djinn");
            assert_eq!(*leaf.get_one::<i64>("max_response_bytes").unwrap(), 1048576);
            assert!(!leaf.get_one::<bool>("recognized_only").unwrap());
            assert!(leaf.get_one::<String>("group").is_none());
            assert!(leaf.get_one::<String>("member").is_none());
        } else if *operation != "prompt-capabilities" {
            assert!(!leaf.get_one::<bool>("execute").unwrap());
        }
        if *operation == "watch" {
            assert_eq!(*leaf.get_one::<f64>("duration").unwrap(), 60.0);
            assert_eq!(*leaf.get_one::<f64>("interval").unwrap(), 2.0);
            assert!(leaf.get_one::<String>("pane").is_none());
        }
        let mut repeated = session_args(operation);
        repeated.extend(args(&["--ti", "1.25", "--timeout", "2.5", "--j", "--json"]));
        let (_, matches) = parse(&repeated).unwrap();
        let leaf = matches
            .subcommand_matches("session")
            .unwrap()
            .subcommand_matches(operation)
            .unwrap();
        assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 2.5);
        assert!(*leaf.get_one::<bool>("json").unwrap());
        repeated.push("--unknown-option".into());
        assert!(parse(&repeated).is_err());
    }
}

#[test]
fn required_fields_and_runtime_choices_cannot_be_overridden_away() {
    for operation in OPERATIONS {
        let full = session_args(operation);
        for index in 3..full.len() {
            if !full[index].to_str().unwrap().starts_with("--") {
                continue;
            }
            let mut missing = full.clone();
            missing.drain(index..=index + 1);
            assert!(parse(&missing).is_err(), "{operation}: {:?}", full[index]);
        }
        if *operation != "prompt-capabilities" {
            let mut missing = full.clone();
            missing.remove(3);
            assert!(parse(&missing).is_err(), "{operation}");
        }
        if ["list", "inspect", "watch", "start", "resume"].contains(operation) {
            let mut invalid = full;
            invalid.extend(args(&["--runtime=-1bad", "--runtime=djinn"]));
            assert!(parse(&invalid).is_err());
        }
    }
}

#[test]
fn leaf_interfaces_do_not_leak_other_session_options() {
    for (operation, flag) in [
        ("list", "--pane=x"),
        ("list", "--expected-terminal=x"),
        ("inspect", "--duration=1"),
        ("start", "--session=x"),
        ("resume", "--native-socket=/x"),
        ("prompt", "--socket=/x"),
        ("prompt-capabilities", "--execute"),
        ("prompt-capabilities", "/workspace"),
    ] {
        let mut values = session_args(operation);
        values.push(flag.into());
        assert!(parse(&values).is_err(), "{operation} {flag}");
    }
}

#[test]
fn session_lexical_operands_restore_paths_and_strings_without_rewriting_numbers() {
    for operation in OPERATIONS {
        let mut values = session_args(operation);
        if *operation != "prompt-capabilities" {
            values[3] = "-1workspace".into();
        }
        // Every lexical flag is present in this fixture, or selected explicitly below.
        for index in 4..values.len() {
            if values[index - 1]
                .to_str()
                .is_some_and(|v| v.starts_with("--") && v != "--runtime")
            {
                values[index] = "-.2literal".into();
            }
        }
        if ["list", "inspect", "watch"].contains(operation) {
            values.extend(args(&["--group", "-3group", "--member", "-4member"]));
        }
        let (_, matches) = parse(&values).unwrap();
        let leaf = matches
            .subcommand_matches("session")
            .unwrap()
            .subcommand_matches(operation)
            .unwrap();
        if *operation != "prompt-capabilities" {
            assert_eq!(
                leaf.get_one::<PathBuf>("workspace").unwrap(),
                &PathBuf::from("-1workspace")
            );
        }
        for field in [
            "socket",
            "native_socket",
            "pane",
            "expected_terminal",
            "executable",
            "session",
            "expected_runtime",
            "request_id",
            "prompt_file",
        ] {
            if let Ok(Some(value)) = leaf.try_get_one::<String>(field) {
                assert_eq!(value, "-.2literal", "{operation} {field}");
            }
        }
        values.extend(args(&["--timeout", "-1.25"]));
        let (_, matches) = parse(&values).unwrap();
        assert_eq!(
            *matches
                .subcommand_matches("session")
                .unwrap()
                .subcommand_matches(operation)
                .unwrap()
                .get_one::<f64>("timeout")
                .unwrap(),
            -1.25
        );
        values.extend(args(&["--timeout", "-1bad"]));
        assert_eq!(checkpoint_compat(&values).1.last().unwrap(), "-1bad");
        let error = parse(&values).err().unwrap();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        assert!(!error.to_string().contains("__coven_checkpoint_operand"));
    }
    let mut values = session_args("watch");
    values.extend(args(&[
        "--dur", "-1.5", "--int", "-2.5", "--max-r", "-1024",
    ]));
    let (cli, _) = parse(&values).unwrap();
    let Command::Session {
        command:
            Session::Watch {
                options,
                duration,
                interval,
                ..
            },
    } = cli.command
    else {
        panic!("watch")
    };
    assert_eq!(
        (duration, interval, options.max_response_bytes),
        (-1.5, -2.5, -1024)
    );
}

#[test]
fn previews_are_pure_for_absent_workspace_executable_socket_and_prompt() {
    let temporary = tempfile::tempdir().unwrap();
    let missing = temporary.path().join("not-created");
    for operation in ["start", "resume", "prompt"] {
        let mut values = session_args(operation);
        values[3] = missing.clone().into_os_string();
        values.push("--json".into());
        let output = run(parse(&values).unwrap().0).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.warnings.is_empty());
        assert!(!output.stdout.ends_with('\n'));
        let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(report["outcome"], "preview");
        assert_eq!(report["delivery"], "not_attempted");
        assert_eq!(report["authorization"], "none");
        if operation == "prompt" {
            assert!(report["prompt_sha256"].is_null());
            assert!(report["prompt_bytes"].is_null());
        } else {
            assert_eq!(report["argv"][0], "/missing/djinn");
            assert_eq!(
                report["argv"][1],
                if operation == "start" {
                    "--ui"
                } else {
                    "--session"
                }
            );
            if operation == "resume" {
                assert_eq!(report["argv"][2], "literal ' ; $(selector)");
            }
        }
        assert!(!missing.exists());
    }
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
}

#[test]
fn inspection_selection_and_interactive_validation_remain_family_owned() {
    for (operation, extra) in [
        ("watch", vec!["--expected-terminal=t"]),
        ("inspect", vec!["--member=m"]),
        ("list", vec!["--max-response-bytes=-1"]),
        ("start", vec!["--executable=relative"]),
        ("resume", vec!["--session=--help"]),
        ("prompt", vec!["--prompt-file=relative"]),
    ] {
        let mut values = session_args(operation);
        values.extend(args(&extra));
        let cli = parse(&values).unwrap().0;
        assert!(run(cli).is_err(), "{operation}");
    }
}

#[test]
fn execute_and_selection_flags_repeat_without_admitting_effects_during_parsing() {
    for operation in ["start", "resume", "prompt"] {
        let mut values = session_args(operation);
        values.extend(args(&["--execute", "--execute"]));
        let (_, matches) = parse(&values).unwrap();
        assert!(
            *matches
                .subcommand_matches("session")
                .unwrap()
                .subcommand_matches(operation)
                .unwrap()
                .get_one::<bool>("execute")
                .unwrap()
        );
    }
    let mut values = session_args("inspect");
    values.extend(args(&[
        "--pan=-2pane",
        "--expected-t=-3terminal",
        "--gr=-4group",
        "--mem=-5member",
        "--recognized",
        "--recognized-only",
    ]));
    let (cli, _) = parse(&values).unwrap();
    let Command::Session {
        command:
            Session::Inspect {
                options,
                pane,
                expected_terminal,
            },
    } = cli.command
    else {
        panic!("inspect")
    };
    assert_eq!(pane, "-2pane");
    assert_eq!(expected_terminal.as_deref(), Some("-3terminal"));
    assert_eq!(options.group.as_deref(), Some("-4group"));
    assert_eq!(options.member.as_deref(), Some("-5member"));
    assert!(options.recognized_only);
}

#[test]
fn default_preview_output_uses_family_renderers_without_terminal_lf() {
    for operation in ["start", "resume", "prompt"] {
        let output = run(parse(&session_args(operation)).unwrap().0).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(!output.stdout.ends_with('\n'));
        if operation == "prompt" {
            let report: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(report["outcome"], "preview");
            assert!(output.stdout.contains("\n  "));
        } else {
            assert!(
                output
                    .stdout
                    .starts_with(&format!("preview: {operation} Djinn UI"))
            );
            assert!(output.stdout.contains("Preview only; --execute"));
        }
    }
}

#[test]
fn static_leaf_json_flags_repeat_abbreviate_and_task_list_defaults_to_text() {
    let (cli, _) = parse(&args(&["coven", "task", "list"])).unwrap();
    assert!(matches!(
        cli.command,
        Command::Task {
            command: Task::List { json: false, .. }
        }
    ));
    for route in [
        vec!["task", "list"],
        vec!["action", "inspect", "action-id"],
        vec![
            "handoff",
            "collect",
            "/workspace",
            "--file",
            "/file",
            "--expect-sha256",
            "hash",
            "--expect-tasks-revision",
            "rev",
        ],
        vec!["handoff", "validate", "/file"],
        vec!["handoff", "inspect", "/workspace", "collection"],
        vec!["handoff", "list", "/workspace"],
    ] {
        let values = [&["coven"][..], route.as_slice(), &["--j", "--json"]].concat();
        assert!(parse(&args(&values)).is_ok(), "{route:?}");
    }
    let (cli, _) = parse(&args(&[
        "coven",
        "handoff",
        "list",
        "/workspace",
        "--ta",
        "first",
        "--task",
        "last",
    ]))
    .unwrap();
    assert!(
        matches!(cli.command, Command::Handoff { command: Handoff::List { task: Some(task), .. } } if task == "last")
    );
}
