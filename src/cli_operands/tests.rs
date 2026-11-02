use super::super::{
    Attempt, Checkpoint, Command, Execution, Task, validate_checkpoint_option_order,
    validate_execution_option_order,
};
use super::*;
use clap::{FromArgMatches, error::ErrorKind};

fn parse(arguments: &[OsString]) -> Result<(Cli, clap::ArgMatches), clap::Error> {
    let (command, parsed) = checkpoint_compat(arguments);
    assert_eq!(parsed.len(), arguments.len());
    let matches = command.try_get_matches_from(parsed)?;
    validate_checkpoint_option_order(&matches, arguments.iter().any(|value| value == "--"))?;
    validate_execution_option_order(&matches, arguments.iter().any(|value| value == "--"))?;
    Ok((Cli::from_arg_matches(&matches)?, matches))
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

const INSPECTION_ROUTES: &[&[&str]] = &[
    &["execution", "inspect"],
    &["task", "attempt", "list"],
    &["task", "attempt", "inspect"],
    &["task", "graph", "inspect"],
    &["task", "graph", "export"],
];

fn inspection_args(route: &[&str], operands: &[&str]) -> Vec<OsString> {
    args(&[&["coven"][..], route, operands].concat())
}

fn inspection_leaf<'a>(matches: &'a clap::ArgMatches, route: &[&str]) -> &'a clap::ArgMatches {
    route.iter().fold(matches, |matches, name| {
        matches.subcommand_matches(name).unwrap()
    })
}

const CANCEL_ROUTE: &[&str] = &["task", "attempt", "cancel"];
const RECOVER_ROUTE: &[&str] = &["execution", "recover"];
const CANCEL_FLAGS: &[&str] = &[
    "--reason",
    "because",
    "--expect-execution-revision",
    "revision",
    "--request-id",
    "request",
];

#[test]
fn cancel_restores_all_strings_paths_repetitions_and_raw_indices() {
    let arguments = inspection_args(
        CANCEL_ROUTE,
        &[
            "--rea",
            "-1anything/../reason/",
            "--expect",
            "-.\u{0661}revision",
            "--request",
            "-\u{1e5f1}request",
            "--reason=__coven_checkpoint_operand_0",
            "--expect-execution-revision=__coven_checkpoint_operand_1",
            "--request-id=__coven_checkpoint_operand_2",
            "--reason=-2last",
            "--expect=-3last",
            "--request=-4last",
            "--j",
            "--js",
            "-5workspace/../exact/",
            "-.\u{0661}attempt",
            "--jso",
            "--json",
        ],
    );
    let original = arguments.clone();
    let (_, parsed) = checkpoint_compat(&arguments);
    assert_eq!(parsed.len(), arguments.len());
    assert_eq!(parsed[5], "__coven_checkpoint_operand_3");
    assert_eq!(parsed[7], "__coven_checkpoint_operand_4");
    assert_eq!(parsed[9], "__coven_checkpoint_operand_5");
    let (cli, matches) = parse(&arguments).unwrap();
    assert_eq!(arguments, original);
    let Command::Task {
        command:
            Task::Attempt {
                command:
                    Attempt::Cancel {
                        items,
                        reason,
                        expect_execution_revision,
                        request_id,
                        json,
                    },
            },
    } = cli.command
    else {
        panic!("wrong command");
    };
    assert_eq!(
        items,
        ["-5workspace/../exact/", "-.\u{0661}attempt"].map(PathBuf::from)
    );
    assert_eq!(
        reason,
        [
            "-1anything/../reason/",
            "__coven_checkpoint_operand_0",
            "-2last"
        ]
    );
    assert_eq!(
        expect_execution_revision,
        [
            "-.\u{0661}revision",
            "__coven_checkpoint_operand_1",
            "-3last"
        ]
    );
    assert_eq!(
        request_id,
        [
            "-\u{1e5f1}request",
            "__coven_checkpoint_operand_2",
            "-4last"
        ]
    );
    assert_eq!(json, [true; 4]);
    let leaf = inspection_leaf(&matches, CANCEL_ROUTE);
    for (field, indices, raw) in [
        (
            "reason",
            vec![2, 8, 14],
            vec![
                "__coven_checkpoint_operand_3",
                "__coven_checkpoint_operand_0",
                "-2last",
            ],
        ),
        (
            "expect_execution_revision",
            vec![4, 10, 16],
            vec![
                "__coven_checkpoint_operand_4",
                "__coven_checkpoint_operand_1",
                "-3last",
            ],
        ),
        (
            "request_id",
            vec![6, 12, 18],
            vec![
                "__coven_checkpoint_operand_5",
                "__coven_checkpoint_operand_2",
                "-4last",
            ],
        ),
    ] {
        assert_eq!(leaf.indices_of(field).unwrap().collect::<Vec<_>>(), indices);
        assert_eq!(leaf.get_raw(field).unwrap().collect::<Vec<_>>(), raw);
    }
    assert_eq!(
        leaf.indices_of("items").unwrap().collect::<Vec<_>>(),
        [23, 24]
    );
    assert_eq!(
        leaf.indices_of("json").unwrap().collect::<Vec<_>>(),
        [20, 22, 26, 28]
    );
}

#[test]
fn cancel_requires_flags_and_one_or_two_selectors() {
    for operands in [vec!["-1attempt"], vec!["-1workspace", "-.2attempt"]] {
        let arguments = inspection_args(CANCEL_ROUTE, &[CANCEL_FLAGS, &operands].concat());
        assert!(parse(&arguments).is_ok());
    }
    for omitted in 0..3 {
        let mut flags = CANCEL_FLAGS.to_vec();
        flags.drain(2 * omitted..2 * omitted + 2);
        flags.push("-1attempt");
        assert_eq!(
            parse(&inspection_args(CANCEL_ROUTE, &flags))
                .err()
                .unwrap()
                .kind(),
            ErrorKind::MissingRequiredArgument
        );
    }
    assert_eq!(
        parse(&inspection_args(CANCEL_ROUTE, CANCEL_FLAGS))
            .err()
            .unwrap()
            .kind(),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        parse(&inspection_args(
            CANCEL_ROUTE,
            &[CANCEL_FLAGS, &["-1root", "-2attempt", "-3extra"]].concat()
        ))
        .err()
        .unwrap()
        .kind(),
        ErrorKind::TooManyValues
    );
}

#[test]
fn cancel_all_flag_groups_respect_operand_and_terminator_boundaries() {
    for flag in [
        vec!["--reason", "last"],
        vec!["--expect", "last"],
        vec!["--request", "last"],
        vec!["--j"],
    ] {
        for suffix in [vec!["-2attempt"], vec!["--"], vec!["--", "-2attempt"]] {
            let operands = [CANCEL_FLAGS, &["-1root"], &flag, &suffix].concat();
            assert_eq!(
                parse(&inspection_args(CANCEL_ROUTE, &operands))
                    .err()
                    .unwrap()
                    .kind(),
                ErrorKind::UnknownArgument,
                "{operands:?}"
            );
        }
        let operands = [CANCEL_FLAGS, &["-1root", "-2attempt"], &flag].concat();
        assert!(parse(&inspection_args(CANCEL_ROUTE, &operands)).is_ok());
    }
    for operands in [vec!["--", "-1root", "--json"], vec!["-1root", "--", "--"]] {
        let arguments = inspection_args(CANCEL_ROUTE, &[CANCEL_FLAGS, &operands].concat());
        let (_, parsed) = checkpoint_compat(&arguments);
        let terminator = arguments.iter().position(|value| value == "--").unwrap();
        assert_eq!(&parsed[terminator..], &arguments[terminator..]);
        let (_, matches) = parse(&arguments).unwrap();
        let leaf = inspection_leaf(&matches, CANCEL_ROUTE);
        assert_eq!(
            leaf.get_many::<PathBuf>("items")
                .unwrap()
                .cloned()
                .collect::<Vec<_>>(),
            ["-1root", operands.last().unwrap()].map(PathBuf::from)
        );
    }
}

#[test]
fn recover_restores_revision_strings_and_keeps_modes_repeats_and_delimiters() {
    for mode in ["--confirm", "--archive"] {
        let arguments = inspection_args(
            RECOVER_ROUTE,
            &[
                mode,
                mode,
                "--expect",
                "-.\u{0661}anything/../revision/",
                "--expect=__coven_checkpoint_operand_0",
                "--expect=-1last",
                "--j",
                "-\u{1e5f1}workspace/../exact/",
                "--json",
            ],
        );
        let (_, parsed) = checkpoint_compat(&arguments);
        assert_eq!(parsed[6], "__coven_checkpoint_operand_1");
        let (cli, matches) = parse(&arguments).unwrap();
        let Command::Execution {
            command:
                Execution::Recover {
                    workspace,
                    confirm_authority,
                    archive_pending,
                    expect_execution_revision,
                    json,
                },
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(workspace.as_os_str(), "-\u{1e5f1}workspace/../exact/");
        assert_eq!(confirm_authority, mode == "--confirm");
        assert_eq!(archive_pending, mode == "--archive");
        assert_eq!(
            expect_execution_revision,
            [
                "-.\u{0661}anything/../revision/",
                "__coven_checkpoint_operand_0",
                "-1last"
            ]
        );
        assert_eq!(json, [true, true]);
        let leaf = inspection_leaf(&matches, RECOVER_ROUTE);
        // Clap collapses the repeated bool override in its logical indices;
        // the adapter still preserves the original argv length.
        assert_eq!(
            leaf.indices_of("expect_execution_revision")
                .unwrap()
                .collect::<Vec<_>>(),
            [4, 6, 8]
        );
        assert_eq!(
            leaf.get_raw("expect_execution_revision")
                .unwrap()
                .collect::<Vec<_>>(),
            [
                "__coven_checkpoint_operand_1",
                "__coven_checkpoint_operand_0",
                "-1last"
            ]
        );
    }
    for operands in [
        vec!["--confirm"],
        vec!["--archive", "--", "--json"],
        vec!["-1root", "--confirm", "--expect", "-2rev", "--json"],
    ] {
        assert!(parse(&inspection_args(RECOVER_ROUTE, &operands)).is_ok());
    }
    assert_eq!(
        parse(&inspection_args(RECOVER_ROUTE, &[]))
            .err()
            .unwrap()
            .kind(),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        parse(&inspection_args(RECOVER_ROUTE, &["--confirm", "--archive"]))
            .err()
            .unwrap()
            .kind(),
        ErrorKind::ArgumentConflict
    );
    assert!(
        parse(&inspection_args(
            RECOVER_ROUTE,
            &["--confirm", "-1root", "-2extra"]
        ))
        .is_err()
    );
}

#[test]
fn maintenance_routes_do_not_capture_unknown_flags_or_string_values() {
    for (route, flags) in [
        (CANCEL_ROUTE, CANCEL_FLAGS),
        (RECOVER_ROUTE, &["--confirm"][..]),
    ] {
        for unknown in [
            "--jsonish",
            "--reasoning",
            "--request-id-extra",
            "--expect-execution-revisions",
            "-inf",
            "-..1",
            "-\u{2165}",
            "--1",
        ] {
            let arguments = inspection_args(route, &[flags, &[unknown]].concat());
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            assert_eq!(
                parse(&arguments).err().unwrap().kind(),
                ErrorKind::UnknownArgument
            );
        }
        let string_flags = if route == CANCEL_ROUTE {
            &["--reason", "--expect", "--request"][..]
        } else {
            &["--expect"][..]
        };
        for flag in string_flags {
            for value in ["--jsonish", "-inf", "-..1", "-\u{2165}"] {
                assert!(
                    parse(&inspection_args(
                        route,
                        &[flags, &[*flag, value, "selector"]].concat()
                    ))
                    .is_err()
                );
            }
        }
    }
    for route in [
        &["execution", "history"][..],
        &["execution", "source"],
        &["task", "attempt", "unknown"],
    ] {
        let arguments = inspection_args(route, &["-1anything", "--reason", "-.2value"]);
        assert_eq!(checkpoint_compat(&arguments).1, arguments);
    }
}

#[cfg(unix)]
#[test]
fn maintenance_paths_preserve_raw_bytes_and_strings_reject_invalid_utf8() {
    use std::os::unix::ffi::OsStringExt;

    for (route, flags, field) in [
        (CANCEL_ROUTE, CANCEL_FLAGS, "items"),
        (RECOVER_ROUTE, &["--confirm"][..], "workspace"),
    ] {
        let literal = OsString::from_vec(b"-1raw\xff/../exact/".to_vec());
        let mut arguments = inspection_args(route, &[flags, &["--"]].concat());
        arguments.push(literal.clone());
        assert_eq!(checkpoint_compat(&arguments).1, arguments);
        let (_, matches) = parse(&arguments).unwrap();
        assert_eq!(
            inspection_leaf(&matches, route)
                .get_one::<PathBuf>(field)
                .unwrap()
                .as_os_str(),
            literal
        );
        arguments.remove(arguments.len() - 2);
        assert_eq!(
            parse(&arguments).err().unwrap().kind(),
            ErrorKind::UnknownArgument
        );
        for flag in if route == CANCEL_ROUTE {
            &["--reason", "--expect", "--request"][..]
        } else {
            &["--expect"][..]
        } {
            let mut arguments = inspection_args(route, &[flags, &["selector", flag]].concat());
            arguments.push(OsString::from_vec(b"value\xff".to_vec()));
            assert_eq!(
                parse(&arguments).err().unwrap().kind(),
                ErrorKind::InvalidUtf8
            );
        }
    }
}

#[test]
fn new_inspection_roots_restore_unicode_prefixes_markers_and_raw_indices() {
    for route in INSPECTION_ROUTES {
        let field = if route.len() == 2 || route[2] == "list" {
            "workspace"
        } else {
            "items"
        };
        for root in [
            "-1anything/../root/",
            "-.\u{0661}suffix",
            "-\u{1e5f1}root",
            "__coven_checkpoint_operand_0",
        ] {
            let arguments = inspection_args(route, &["--j", root, "--json"]);
            let original = arguments.clone();
            let (_, parsed) = checkpoint_compat(&arguments);
            assert_eq!(parsed.len(), arguments.len());
            assert_eq!(&parsed[..route.len() + 2], &arguments[..route.len() + 2]);
            assert_eq!(parsed.last(), arguments.last());
            let (_, matches) = parse(&arguments).unwrap();
            assert_eq!(arguments, original);
            let leaf = inspection_leaf(&matches, route);
            assert_eq!(leaf.get_one::<PathBuf>(field).unwrap().as_os_str(), root);
            assert_eq!(
                leaf.get_raw(field).unwrap().collect::<Vec<_>>(),
                [&parsed[route.len() + 2]]
            );
            assert_eq!(leaf.indices_of(field).unwrap().collect::<Vec<_>>(), [3]);
            assert_eq!(leaf.indices_of("json").unwrap().collect::<Vec<_>>(), [2, 5]);
            assert_eq!(
                leaf.get_many::<bool>("json")
                    .unwrap()
                    .copied()
                    .collect::<Vec<_>>(),
                [true, true]
            );
        }
    }
    let (cli, _) = parse(&inspection_args(
        &["execution", "inspect"],
        &["-1root", "--json"],
    ))
    .unwrap();
    let Command::Execution {
        command: Execution::Inspect { workspace, json },
    } = cli.command
    else {
        panic!("wrong command");
    };
    assert_eq!(workspace.as_os_str(), "-1root");
    assert_eq!(json, [true]);
}

#[test]
fn attempt_list_restores_each_task_string_without_changing_raw_binding() {
    let route = ["task", "attempt", "list"];
    let arguments = inspection_args(
        &route,
        &[
            "--task",
            "-1anything",
            "--task",
            "-.\u{0661}/../exact/",
            "--task=__coven_checkpoint_operand_0",
            "--task=-\u{1e5f1}last",
            "--j",
            "-2workspace",
            "--json",
        ],
    );
    let original = arguments.clone();
    let (_, parsed) = checkpoint_compat(&arguments);
    assert_eq!(parsed.len(), arguments.len());
    assert_eq!(parsed[5], "__coven_checkpoint_operand_1");
    assert_eq!(parsed[7], "__coven_checkpoint_operand_2");
    let (cli, matches) = parse(&arguments).unwrap();
    assert_eq!(arguments, original);
    let Command::Task {
        command:
            Task::Attempt {
                command:
                    Attempt::List {
                        workspace,
                        task,
                        json,
                    },
            },
    } = cli.command
    else {
        panic!("wrong command");
    };
    assert_eq!(workspace.as_os_str(), "-2workspace");
    assert_eq!(
        task,
        [
            "-1anything",
            "-.\u{0661}/../exact/",
            "__coven_checkpoint_operand_0",
            "-\u{1e5f1}last"
        ]
    );
    assert_eq!(task.last().unwrap(), "-\u{1e5f1}last");
    assert_eq!(json, [true, true]);
    let leaf = inspection_leaf(&matches, &route);
    assert_eq!(
        leaf.get_raw("task").unwrap().collect::<Vec<_>>(),
        [
            "__coven_checkpoint_operand_1",
            "__coven_checkpoint_operand_2",
            "__coven_checkpoint_operand_0",
            "-\u{1e5f1}last",
        ]
    );
    assert_eq!(
        leaf.indices_of("task").unwrap().collect::<Vec<_>>(),
        [2, 4, 6, 8]
    );
    assert_eq!(
        leaf.indices_of("workspace").unwrap().collect::<Vec<_>>(),
        [11]
    );
    assert_eq!(
        leaf.indices_of("json").unwrap().collect::<Vec<_>>(),
        [10, 13]
    );
}

#[test]
fn new_variadic_inspections_keep_option_groups_and_terminator_boundaries() {
    for route in &INSPECTION_ROUTES[2..] {
        for operands in [
            vec!["--j", "--js", "-1root", "-.2selector", "--jso", "--json"],
            vec!["--json", "--", "-1root", "--json"],
            vec!["-1root", "--", "__coven_checkpoint_operand_0"],
        ] {
            let arguments = inspection_args(route, &operands);
            let (_, parsed) = checkpoint_compat(&arguments);
            if let Some(index) = arguments.iter().position(|value| value == "--") {
                assert_eq!(&parsed[index..], &arguments[index..]);
            }
            let (_, matches) = parse(&arguments).unwrap();
            let leaf = inspection_leaf(&matches, route);
            let mut after_terminator = false;
            let expected = operands
                .iter()
                .filter_map(|value| {
                    if after_terminator {
                        return Some(PathBuf::from(value));
                    }
                    if *value == "--" {
                        after_terminator = true;
                        None
                    } else if value.starts_with("--") {
                        None
                    } else {
                        Some(PathBuf::from(value))
                    }
                })
                .collect::<Vec<_>>();
            let items = leaf
                .get_many::<PathBuf>("items")
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(items, expected.iter().collect::<Vec<_>>());
        }
        let arguments = inspection_args(
            route,
            &["-1root", "--", "__coven_checkpoint_operand_0", "-3selector"],
        );
        if *route == ["task", "graph", "inspect"] {
            let (_, matches) = parse(&arguments).unwrap();
            assert_eq!(
                inspection_leaf(&matches, route)
                    .get_many::<PathBuf>("items")
                    .unwrap()
                    .cloned()
                    .collect::<Vec<_>>(),
                ["-1root", "__coven_checkpoint_operand_0", "-3selector"].map(PathBuf::from)
            );
        } else {
            assert_eq!(
                parse(&arguments).err().unwrap().kind(),
                ErrorKind::TooManyValues
            );
        }
        for operands in [
            vec!["-1root", "--json", "-.2selector"],
            vec!["--json", "-1root", "--j", "-.2selector", "--json"],
            vec!["-1root", "--json", "--"],
        ] {
            assert_eq!(
                parse(&inspection_args(route, &operands))
                    .err()
                    .unwrap()
                    .kind(),
                ErrorKind::UnknownArgument
            );
        }
    }
}

#[test]
fn new_inspection_scope_rejects_unknown_prefixes_and_leaves_old_routes_alone() {
    for route in INSPECTION_ROUTES {
        for operand in [
            "-inf",
            "-nan",
            "-\u{2165}",
            "-\u{00bd}",
            "--1",
            "-..1",
            "--jsonish",
        ] {
            let arguments = inspection_args(route, &[operand]);
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            assert_eq!(
                parse(&arguments).err().unwrap().kind(),
                ErrorKind::UnknownArgument
            );
        }
        assert_eq!(
            parse(&inspection_args(route, &["-1root", "--help"]))
                .err()
                .unwrap()
                .kind(),
            ErrorKind::DisplayHelp
        );
    }
    for route in [
        &["execution", "history"][..],
        &["task", "list"],
        &["task", "add"],
        &["task", "assign"],
        &["state", "inspect"],
        &["task", "graph", "unknown"],
    ] {
        let arguments = inspection_args(route, &["-1anything", "--json"]);
        assert_eq!(checkpoint_compat(&arguments).1, arguments);
    }
    for value in ["--jsonish", "-inf", "-..1", "-\u{2165}"] {
        assert!(
            parse(&inspection_args(
                &["task", "attempt", "list"],
                &["--task", value]
            ))
            .is_err()
        );
    }
}

#[cfg(unix)]
#[test]
fn new_inspection_non_utf8_paths_are_never_normalized_or_lossily_restored() {
    use std::os::unix::ffi::OsStringExt;

    for route in INSPECTION_ROUTES {
        let field = if route.len() == 2 || route[2] == "list" {
            "workspace"
        } else {
            "items"
        };
        for bytes in [
            b"root\xff".to_vec(),
            b"-1\xff".to_vec(),
            b"-.1\xff".to_vec(),
        ] {
            let literal = OsString::from_vec(bytes);
            let mut arguments = inspection_args(route, &["--"]);
            arguments.push(literal.clone());
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            let (_, matches) = parse(&arguments).unwrap();
            let path = inspection_leaf(&matches, route)
                .get_one::<PathBuf>(field)
                .unwrap();
            assert_eq!(
                path.as_os_str().as_encoded_bytes(),
                literal.as_encoded_bytes()
            );
            arguments.remove(route.len() + 1);
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            if literal.as_encoded_bytes().starts_with(b"-") {
                assert_eq!(
                    parse(&arguments).err().unwrap().kind(),
                    ErrorKind::UnknownArgument
                );
            } else {
                let (_, matches) = parse(&arguments).unwrap();
                assert_eq!(
                    inspection_leaf(&matches, route)
                        .get_one::<PathBuf>(field)
                        .unwrap()
                        .as_os_str(),
                    literal
                );
            }
        }
    }
    let mut arguments = inspection_args(&["task", "attempt", "list"], &["--task"]);
    arguments.push(OsString::from_vec(b"task\xff".to_vec()));
    assert_eq!(checkpoint_compat(&arguments).1, arguments);
    assert_eq!(
        parse(&arguments).err().unwrap().kind(),
        ErrorKind::InvalidUtf8
    );
}

#[test]
fn prefix_operands_restore_exact_paths_and_indices() {
    for operand in [
        "-.5",
        "-1e+2",
        "-1_000",
        "-1garbage",
        "-1anything",
        "-1/../root",
        "-\u{0661}",
        "-.\u{0661}",
        "-.\u{0665}",
        "-\u{1e5f1}suffix",
    ] {
        let arguments = args(&["coven", "checkpoint", "inspect", operand, "--json"]);
        let before = arguments.clone();
        let (cli, matches) = parse(&arguments).unwrap();
        assert_eq!(arguments, before);
        let Command::Checkpoint {
            command: Checkpoint::Inspect { items, json },
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(items, [PathBuf::from(operand)]);
        assert_eq!(json, [true]);
        let inspect = matches
            .subcommand_matches("checkpoint")
            .unwrap()
            .subcommand_matches("inspect")
            .unwrap();
        assert_eq!(
            inspect.indices_of("items").unwrap().collect::<Vec<_>>(),
            [1]
        );
        // Append's implicit true value has its own Clap index after the flag.
        assert_eq!(inspect.indices_of("json").unwrap().collect::<Vec<_>>(), [3]);
        let (cli, _) = parse(&args(&["coven", "next", operand])).unwrap();
        let Command::Next { workspace } = cli.command else {
            panic!("wrong command")
        };
        assert_eq!(workspace, PathBuf::from(operand));
    }
}

#[test]
fn tokens_do_not_collide_even_with_literals_after_terminator() {
    let arguments = args(&[
        "coven",
        "checkpoint",
        "inspect",
        "__coven_checkpoint_operand_0",
        "-1x",
        "-.2",
        "--",
        "__coven_checkpoint_operand_1",
        "-3x",
        "/exact/../root/",
    ]);
    let (_, parsed) = checkpoint_compat(&arguments);
    assert_eq!(parsed[4], "__coven_checkpoint_operand_2");
    assert_eq!(parsed[5], "__coven_checkpoint_operand_3");
    assert_eq!(&parsed[6..], &arguments[6..]);
    let (cli, _) = parse(&arguments).unwrap();
    let Command::Checkpoint {
        command: Checkpoint::Inspect { items, .. },
    } = cli.command
    else {
        panic!("wrong command");
    };
    let expected: Vec<_> = arguments[3..]
        .iter()
        .filter(|value| *value != "--")
        .map(PathBuf::from)
        .collect();
    for (actual, expected) in items.iter().zip(&expected) {
        assert_eq!(actual.as_os_str(), expected.as_os_str());
    }
    assert_eq!(items.len(), expected.len());
}

#[test]
fn large_argument_list_preserves_exact_operands_indices_and_every_json_flag() {
    const PAIRS: usize = 1024;
    let flags = ["--j", "--js", "--jso", "--json"];
    let mut arguments = args(&["coven", "checkpoint", "inspect"]);
    arguments.extend(args(&flags));
    let mut expected = Vec::new();
    for index in 0..PAIRS {
        let occupied = OsString::from(format!("__coven_checkpoint_operand_{index}"));
        let negative = OsString::from(format!("-.1prefix/{index}/../\u{0661}/"));
        expected.extend([occupied.clone(), negative.clone()]);
        arguments.extend([occupied, negative]);
    }
    arguments.extend(args(&flags));
    let original = arguments.clone();
    let (_, parsed) = checkpoint_compat(&arguments);
    assert_eq!(arguments, original);
    assert_eq!(arguments.len(), 3 + 2 * flags.len() + 2 * PAIRS);
    assert_eq!(parsed.len(), arguments.len());
    assert_eq!(&parsed[..3 + flags.len()], &arguments[..3 + flags.len()]);
    assert_eq!(
        &parsed[parsed.len() - flags.len()..],
        &arguments[arguments.len() - flags.len()..]
    );
    for index in 0..PAIRS {
        let position = 3 + flags.len() + 2 * index;
        assert_eq!(parsed[position], arguments[position]);
        assert_eq!(
            parsed[position + 1],
            OsString::from(format!("__coven_checkpoint_operand_{}", PAIRS + index))
        );
    }
    let (cli, matches) = parse(&arguments).unwrap();
    let Command::Checkpoint {
        command: Checkpoint::Inspect { items, json },
    } = cli.command
    else {
        panic!("wrong command");
    };
    assert_eq!(items.len(), expected.len());
    for (actual, expected) in items.iter().zip(&expected) {
        assert_eq!(
            actual.as_os_str().as_encoded_bytes(),
            expected.as_encoded_bytes()
        );
    }
    assert_eq!(json, vec![true; 2 * flags.len()]);
    let inspect = matches
        .subcommand_matches("checkpoint")
        .unwrap()
        .subcommand_matches("inspect")
        .unwrap();
    assert_eq!(
        inspect.indices_of("items").unwrap().collect::<Vec<_>>(),
        (2 * flags.len() + 1..=2 * flags.len() + expected.len()).collect::<Vec<_>>()
    );
    let expected_flags: Vec<_> = (1..=flags.len())
        .map(|index| 2 * index)
        .chain((1..=flags.len()).map(|index| 2 * flags.len() + expected.len() + 2 * index))
        .collect();
    assert_eq!(
        inspect.indices_of("json").unwrap().collect::<Vec<_>>(),
        expected_flags
    );
}

#[test]
fn unknown_flags_and_other_commands_remain_untouched() {
    for operand in [
        "-inf",
        "-nan",
        "-\u{2165}",
        "-\u{00bd}",
        "--1",
        "-..1",
        "--jsonish",
    ] {
        for prefix in [
            vec!["coven", "next"],
            vec!["coven", "checkpoint", "inspect"],
        ] {
            let arguments = args(&[prefix, vec![operand]].concat());
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            assert_eq!(
                parse(&arguments).err().unwrap().kind(),
                ErrorKind::UnknownArgument
            );
        }
    }
    for command in ["goal", "message", "messages", "status", "refresh", "init"] {
        let arguments = args(&["coven", command, "-1garbage"]);
        assert_eq!(checkpoint_compat(&arguments).1, arguments);
    }
}

#[test]
fn json_prefixes_repetition_and_all_occurrence_ordering_are_preserved() {
    for operands in [
        vec!["--j", "--js", "--jso", "--json", "-1anything"],
        vec!["-1anything", "--j", "--json"],
        vec!["--json", "-1anything", "--json"],
    ] {
        let arguments = args(&[vec!["coven", "checkpoint", "inspect"], operands.clone()].concat());
        let (cli, _) = parse(&arguments).unwrap();
        let Command::Checkpoint {
            command: Checkpoint::Inspect { items, json },
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(items, [PathBuf::from("-1anything")]);
        assert_eq!(json, vec![true; operands.len() - 1]);
    }
    for operands in [
        vec!["-1anything", "--json", "-.2"],
        vec!["--json", "-1anything", "--j", "-.2", "--json"],
        vec!["-1anything", "--json", "--"],
    ] {
        let arguments = args(&[vec!["coven", "checkpoint", "inspect"], operands].concat());
        assert_eq!(
            parse(&arguments).err().unwrap().kind(),
            ErrorKind::UnknownArgument
        );
    }
}

#[test]
fn help_defaults_and_first_original_terminator_keep_their_meaning() {
    for command in [
        vec!["coven", "next"],
        vec!["coven", "checkpoint", "inspect"],
    ] {
        let help = args(&[command.clone(), vec!["-1anything", "--help"]].concat());
        assert_eq!(parse(&help).err().unwrap().kind(), ErrorKind::DisplayHelp);
        let arguments = args(&[command, vec!["--", "--help"]].concat());
        assert_eq!(checkpoint_compat(&arguments).1, arguments);
        assert!(parse(&arguments).is_ok());
    }
    let (cli, _) = parse(&args(&["coven", "next"])).unwrap();
    let Command::Next { workspace } = cli.command else {
        panic!("wrong command")
    };
    assert_eq!(workspace, PathBuf::from("."));
    let arguments = args(&[
        "coven",
        "checkpoint",
        "inspect",
        "--",
        "-1anything",
        "--",
        "--json",
    ]);
    assert_eq!(checkpoint_compat(&arguments).1, arguments);
    let (cli, _) = parse(&arguments).unwrap();
    let Command::Checkpoint {
        command: Checkpoint::Inspect { items, json },
    } = cli.command
    else {
        panic!("wrong command");
    };
    assert_eq!(items, ["-1anything", "--", "--json"].map(PathBuf::from));
    assert!(json.is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_is_not_rewritten_and_restores_without_loss() {
    use std::os::unix::ffi::OsStringExt;
    for bytes in [
        b"root\xff".to_vec(),
        b"-1\xff".to_vec(),
        b"-.1\xff".to_vec(),
    ] {
        let literal = OsString::from_vec(bytes);
        for prefix in [
            vec!["coven", "checkpoint", "inspect"],
            vec!["coven", "next"],
        ] {
            let mut arguments = args(&prefix);
            let terminator = arguments.len();
            arguments.push(OsString::from("--"));
            arguments.push(literal.clone());
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            let (cli, _) = parse(&arguments).unwrap();
            let path = match cli.command {
                Command::Checkpoint {
                    command: Checkpoint::Inspect { mut items, .. },
                } => {
                    assert_eq!(items.len(), 1);
                    items.remove(0)
                }
                Command::Next { workspace } => workspace,
                _ => panic!("wrong command"),
            };
            assert_eq!(
                path.as_os_str().as_encoded_bytes(),
                literal.as_encoded_bytes()
            );
            arguments.remove(terminator);
            assert_eq!(checkpoint_compat(&arguments).1, arguments);
            if literal.as_encoded_bytes().starts_with(b"-") {
                let error = parse(&arguments)
                    .err()
                    .expect("non-UTF-8 negative prefix before -- must be rejected");
                assert_eq!(error.kind(), ErrorKind::UnknownArgument);
                assert_eq!(error.exit_code(), 2);
            } else {
                let (cli, _) = parse(&arguments).unwrap();
                let path = match cli.command {
                    Command::Checkpoint {
                        command: Checkpoint::Inspect { mut items, .. },
                    } => {
                        assert_eq!(items.len(), 1);
                        items.remove(0)
                    }
                    Command::Next { workspace } => workspace,
                    _ => panic!("wrong command"),
                };
                assert_eq!(
                    path.as_os_str().as_encoded_bytes(),
                    literal.as_encoded_bytes()
                );
            }
        }
    }
}

#[test]
fn frozen_ranges_are_sorted_disjoint_and_contain_760_scalars() {
    assert_eq!(UNICODE_16_ND.len(), 71);
    assert!(UNICODE_16_ND.iter().all(|&(start, end)| start <= end));
    assert!(
        UNICODE_16_ND
            .windows(2)
            .all(|pair| pair[0].1 + 1 < pair[1].0)
    );
    assert_eq!(
        UNICODE_16_ND
            .iter()
            .map(|&(start, end)| end - start + 1)
            .sum::<u32>(),
        760
    );
}

#[test]
fn python_unicode_16_oracle() {
    use std::{
        io::{Read, Seek, SeekFrom},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    struct OracleChild(Child);
    impl Drop for OracleChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // File-backed output cannot fill a pipe while waiting; the guard also reaps
    // the oracle if polling or any deadline assertion panics.
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut child = OracleChild(Command::new("python3").args([
        "-B",
        "-c",
        "import argparse, sys, unicodedata\nassert sys.version_info[:2] == (3, 14)\nassert unicodedata.unidata_version == '16.0.0'\nm = argparse.ArgumentParser()._negative_number_matcher\nassert m.pattern == r'-\\.?\\d'\nfor n in range(0x110000):\n if 0xD800 <= n <= 0xDFFF: continue\n c = chr(n)\n nd = unicodedata.category(c) == 'Nd'\n assert bool(m.match('-' + c + 'anything')) == nd\n assert bool(m.match('-.' + c + 'anything')) == nd\n if nd: print(n)",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::from(stdout.try_clone().unwrap()))
    .stderr(Stdio::from(stderr.try_clone().unwrap()))
    .spawn()
    .unwrap());
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    drop(child);
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut diagnostics = String::new();
    stderr.read_to_string(&mut diagnostics).unwrap();
    assert!(
        status.is_some(),
        "Unicode oracle exceeded 60 seconds: {diagnostics}"
    );
    assert!(status.unwrap().success(), "{diagnostics}");
    stdout.seek(SeekFrom::Start(0)).unwrap();
    let mut scalars = String::new();
    stdout.read_to_string(&mut scalars).unwrap();
    let expected: Vec<u32> = scalars.lines().map(|line| line.parse().unwrap()).collect();
    let actual: Vec<u32> = (0..=0x10FFFF)
        .filter(|&n| {
            char::from_u32(n).is_some()
                && UNICODE_16_ND
                    .iter()
                    .any(|&(start, end)| (start..=end).contains(&n))
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 760);
}
