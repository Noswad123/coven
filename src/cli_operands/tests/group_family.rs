use super::*;

#[test]
fn new_observation_options_repeat_abbreviate_and_restore_typed_strings() {
    for (base, fields) in [
        (
            vec!["coven", "observe", "kitsune", "--socket", "/socket"],
            &["socket", "process_pane"][..],
        ),
        (live_args(), &["file", "runtime", "instance", "session"][..]),
    ] {
        for field in fields {
            let flag = format!("--{}", field.replace('_', "-"));
            let abbreviation = format!("--{}", &field[..2]);
            let (_, matches) = parse(&args(
                &[
                    base.as_slice(),
                    &[&abbreviation, "first", &flag, "-1literal"],
                ]
                .concat(),
            ))
            .unwrap();
            let leaf = matches
                .subcommand_matches("observe")
                .unwrap()
                .subcommand()
                .unwrap()
                .1;
            assert_eq!(leaf.get_one::<String>(field).unwrap(), "-1literal");
        }
        let (_, matches) = parse(&args(
            &[base.as_slice(), &["--t", "0.1", "--timeout", "0.5"]].concat(),
        ))
        .unwrap();
        let leaf = matches
            .subcommand_matches("observe")
            .unwrap()
            .subcommand()
            .unwrap()
            .1;
        assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 0.5);
    }
    let (_, matches) = parse(&args(&[
        "coven",
        "observe",
        "kitsune",
        "--socket",
        "/socket",
        "--max",
        "2048",
        "--max-response-bytes",
        "4096",
    ]))
    .unwrap();
    assert_eq!(
        *matches
            .subcommand_matches("observe")
            .unwrap()
            .subcommand_matches("kitsune")
            .unwrap()
            .get_one::<i64>("max_response_bytes")
            .unwrap(),
        4096
    );
}

#[cfg(unix)]
#[test]
fn group_paths_keep_non_utf8_but_observer_and_group_strings_reject_it() {
    use std::os::unix::ffi::OsStringExt;
    let invalid = OsString::from_vec(vec![b'p', 0xff]);
    let mut argv = group_args("create", &["W", "g"]);
    argv[3] = invalid.clone();
    let (_, matches) = parse(&argv).unwrap();
    assert_eq!(
        matches
            .subcommand_matches("group")
            .unwrap()
            .subcommand_matches("create")
            .unwrap()
            .get_one::<PathBuf>("workspace")
            .unwrap()
            .as_os_str(),
        invalid
    );
    for index in [4, 6] {
        let mut argv = group_args("create", &["W", "g", "--name", "name"]);
        argv[index] = invalid.clone();
        assert_eq!(parse(&argv).err().unwrap().kind(), ErrorKind::InvalidUtf8);
    }
    for field in ["--file", "--runtime", "--instance", "--session"] {
        let mut argv = args(&live_args());
        let i = argv.iter().position(|v| v == field).unwrap() + 1;
        argv[i] = invalid.clone();
        assert_eq!(parse(&argv).err().unwrap().kind(), ErrorKind::InvalidUtf8);
    }
}

#[test]
fn group_semantic_numeric_errors_are_not_clap_parse_errors() {
    for tail in [
        vec![
            "coven",
            "observe",
            "kitsune",
            "--socket",
            "/socket",
            "--timeout",
            "0",
        ],
        vec![
            "coven",
            "observe",
            "kitsune",
            "--socket",
            "/socket",
            "--timeout",
            "NaN",
        ],
        vec![
            "coven",
            "observe",
            "kitsune",
            "--socket",
            "/socket",
            "--max-response-bytes",
            "-1",
        ],
        vec![
            "coven",
            "group",
            "publish",
            "W",
            "g",
            "--socket",
            "/socket",
            "--source",
            "coven",
            "--duration",
            "-1",
        ],
        vec!["coven", "group", "status", "W", "g", "--timeout", "0"],
    ] {
        let (cli, _) = parse(&args(&tail)).unwrap();
        assert!(super::super::super::run(cli).is_err());
    }
}

const ROUTES: &[(&str, &[&str])] = &[
    ("create", &["W", "g"]),
    (
        "add",
        &[
            "W", "g", "--member", "m", "--role", "review", "--socket", "/socket", "--pane", "p",
        ],
    ),
    ("remove", &["W", "g", "--member", "m"]),
    ("list", &["W"]),
    (
        "attach-live",
        &[
            "W",
            "g",
            "--member",
            "m",
            "--file",
            "/file",
            "--runtime",
            "r",
            "--instance",
            "i",
            "--session",
            "s",
        ],
    ),
    ("detach-live", &["W", "g", "--member", "m"]),
    ("status", &["W", "g"]),
    (
        "publish",
        &["W", "g", "--socket", "/socket", "--source", "coven"],
    ),
];

fn group_args(route: &str, tail: &[&str]) -> Vec<OsString> {
    args(&[&["coven", "group", route][..], tail].concat())
}

#[test]
fn all_eight_group_routes_require_explicit_workspace_and_accept_json() {
    for (route, tail) in ROUTES {
        for extra in [&[][..], &["--json"][..], &["--js", "--json"][..]] {
            let (_, matches) = parse(&group_args(route, &[*tail, extra].concat())).unwrap();
            let leaf = matches
                .subcommand_matches("group")
                .unwrap()
                .subcommand_matches(route)
                .unwrap();
            assert_eq!(
                leaf.get_one::<PathBuf>("workspace").unwrap(),
                &PathBuf::from("W")
            );
            assert_eq!(*leaf.get_one::<bool>("json").unwrap(), !extra.is_empty());
        }
        assert!(parse(&group_args(route, &tail[1..])).is_err(), "{route}");
        assert!(
            parse(&group_args(route, &[*tail, &["extra"]].concat())).is_err(),
            "{route}"
        );
    }
}

#[test]
fn group_defaults_and_new_observation_defaults() {
    for (route, tail) in ROUTES {
        let (_, matches) = parse(&group_args(route, tail)).unwrap();
        let leaf = matches
            .subcommand_matches("group")
            .unwrap()
            .subcommand_matches(route)
            .unwrap();
        match *route {
            "create" => assert!(leaf.get_one::<String>("name").is_none()),
            "add" => {
                assert!(!leaf.get_one::<bool>("rebind").unwrap());
                assert!(!leaf.get_one::<bool>("process_evidence").unwrap());
            }
            "attach-live" => {
                assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 1.0);
                assert!(!leaf.get_one::<bool>("reattach").unwrap());
            }
            "status" => {
                assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 1.0);
                assert!(leaf.get_one::<String>("member").is_none());
            }
            "publish" => {
                assert_eq!(*leaf.get_one::<i64>("duration").unwrap(), 60);
                assert!(!leaf.get_one::<bool>("live").unwrap());
            }
            _ => {}
        }
    }
    let (_, matches) = parse(&args(&[
        "coven", "observe", "kitsune", "--socket", "/socket",
    ]))
    .unwrap();
    let leaf = matches
        .subcommand_matches("observe")
        .unwrap()
        .subcommand_matches("kitsune")
        .unwrap();
    assert_eq!(*leaf.get_one::<f64>("timeout").unwrap(), 3.0);
    assert_eq!(
        *leaf.get_one::<i64>("max_response_bytes").unwrap(),
        1_048_576
    );
    assert!(leaf.get_one::<String>("process_pane").is_none());
    let (_, matches) = parse(&args(&live_args())).unwrap();
    assert_eq!(
        *matches
            .subcommand_matches("observe")
            .unwrap()
            .subcommand_matches("djinn-live")
            .unwrap()
            .get_one::<f64>("timeout")
            .unwrap(),
        1.0
    );
}

fn live_args() -> Vec<&'static str> {
    vec![
        "coven",
        "observe",
        "djinn-live",
        "--file",
        "/file",
        "--runtime",
        "r",
        "--instance",
        "i",
        "--session",
        "s",
    ]
}

#[test]
fn group_every_optional_flag_repeats_abbreviates_and_last_value_wins() {
    for (route, tail) in ROUTES {
        let flags: &[(&str, &str, bool)] = match *route {
            "create" => &[
                ("name", "na", false),
                ("expect_revision", "expect-r", false),
            ],
            "add" => &[
                ("member", "mem", false),
                ("role", "ro", false),
                ("socket", "soc", false),
                ("pane", "pa", false),
                ("expect_revision", "expect-r", false),
                ("rebind", "reb", true),
                ("process_evidence", "process-e", true),
            ],
            "remove" | "detach-live" => &[
                ("member", "mem", false),
                ("expect_revision", "expect-r", false),
            ],
            "attach-live" => &[
                ("member", "mem", false),
                ("file", "fi", false),
                ("runtime", "ru", false),
                ("instance", "in", false),
                ("session", "se", false),
                ("expect_revision", "expect-r", false),
                ("reattach", "reat", true),
            ],
            "status" => &[("member", "mem", false)],
            "publish" => &[
                ("socket", "soc", false),
                ("source", "sou", false),
                ("live", "li", true),
            ],
            _ => &[],
        };
        for (field, abbreviation, boolean) in flags {
            let flag = format!("--{}", field.replace('_', "-"));
            let short = format!("--{abbreviation}");
            let extra = if *boolean {
                vec![short.as_str(), flag.as_str()]
            } else {
                vec![short.as_str(), "first", flag.as_str(), "last"]
            };
            let (_, matches) = parse(&group_args(route, &[*tail, &extra].concat())).unwrap();
            let leaf = matches
                .subcommand_matches("group")
                .unwrap()
                .subcommand_matches(route)
                .unwrap();
            if *boolean {
                assert!(*leaf.get_one::<bool>(field).unwrap());
            } else {
                assert_eq!(leaf.get_one::<String>(field).unwrap(), "last");
            }
        }
    }
}

#[test]
fn group_numeric_fields_stay_typed_and_malformed_values_are_clap_errors() {
    for (route, field) in [
        ("attach-live", "timeout"),
        ("status", "timeout"),
        ("publish", "duration"),
    ] {
        let tail = ROUTES.iter().find(|r| r.0 == route).unwrap().1;
        for value in ["bad", "-1invalid"] {
            let flag = format!("--{field}");
            let error = parse(&group_args(route, &[tail, &[&flag, value]].concat()))
                .err()
                .unwrap();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
            assert_eq!(error.exit_code(), 2);
        }
        let flag = format!("--{field}");
        assert!(parse(&group_args(route, &[tail, &[&flag, "-1"]].concat())).is_ok());
        assert!(
            parse(&group_args(
                route,
                &[tail, &[&flag, "1", &flag, "2"]].concat()
            ))
            .is_ok()
        );
    }
    for (route, field) in [
        ("kitsune", "timeout"),
        ("kitsune", "max-response-bytes"),
        ("djinn-live", "timeout"),
    ] {
        let base = if route == "kitsune" {
            vec!["coven", "observe", "kitsune", "--socket", "/socket"]
        } else {
            live_args()
        };
        let flag = format!("--{field}");
        for value in ["bad", "-1invalid"] {
            let error = parse(&args(&[base.as_slice(), &[&flag, value]].concat()))
                .err()
                .unwrap();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
        }
    }
}

#[test]
fn group_negative_prefix_operands_restore_exact_string_and_path_types() {
    for value in ["-1literal", "-.1literal", "-\u{0661}literal"] {
        let (_, matches) = parse(&group_args(
            "add",
            &[
                value,
                value,
                "--member",
                value,
                "--role",
                value,
                "--socket",
                value,
                "--pane",
                value,
                "--expect-revision",
                value,
            ],
        ))
        .unwrap();
        let leaf = matches
            .subcommand_matches("group")
            .unwrap()
            .subcommand_matches("add")
            .unwrap();
        assert_eq!(
            leaf.get_one::<PathBuf>("workspace").unwrap(),
            &PathBuf::from(value)
        );
        for field in [
            "group",
            "member",
            "role",
            "socket",
            "pane",
            "expect_revision",
        ] {
            assert_eq!(leaf.get_one::<String>(field).unwrap(), value);
        }
        let (_, matches) = parse(&args(&[
            "coven",
            "observe",
            "djinn-live",
            "--file",
            value,
            "--runtime",
            value,
            "--instance",
            value,
            "--session",
            value,
        ]))
        .unwrap();
        let leaf = matches
            .subcommand_matches("observe")
            .unwrap()
            .subcommand_matches("djinn-live")
            .unwrap();
        for field in ["file", "runtime", "instance", "session"] {
            assert_eq!(leaf.get_one::<String>(field).unwrap(), value);
        }
    }
}

#[test]
fn group_terminator_unknown_flags_and_required_options_remain_narrow() {
    for (route, tail) in ROUTES {
        assert!(parse(&group_args(route, &[*tail, &["--unknown"]].concat())).is_err());
        assert!(
            parse(&group_args(route, &[*tail, &["--name", "x"]].concat())).is_ok()
                == (*route == "create")
        );
    }
    for tail in [vec!["--", "--json", "--name"], vec!["W", "--", "--json"]] {
        let (_, matches) = parse(&group_args("create", &tail)).unwrap();
        let leaf = matches
            .subcommand_matches("group")
            .unwrap()
            .subcommand_matches("create")
            .unwrap();
        assert!(!leaf.get_one::<bool>("json").unwrap());
        assert_eq!(
            leaf.get_one::<String>("group").unwrap(),
            *tail.last().unwrap()
        );
    }
    for (route, tail) in ROUTES {
        for flag in [
            "--member",
            "--role",
            "--socket",
            "--pane",
            "--file",
            "--runtime",
            "--instance",
            "--session",
            "--source",
        ] {
            if let Some(i) = tail.iter().position(|v| *v == flag) {
                let missing = [&tail[..i], &tail[i + 2..]].concat();
                assert!(
                    parse(&group_args(route, &missing)).is_err(),
                    "{route} {flag}"
                );
            }
        }
    }
}

#[test]
fn new_observers_require_all_selectors_and_reject_workspace_inference() {
    for base in [
        vec!["coven", "observe", "kitsune", "--socket", "/socket"],
        live_args(),
    ] {
        assert!(parse(&args(&[base.as_slice(), &["W"]].concat())).is_err());
        for i in (3..base.len()).step_by(2) {
            assert!(parse(&args(&[&base[..i], &base[i + 2..]].concat())).is_err());
        }
        assert!(parse(&args(&[base.as_slice(), &["--js", "--json"]].concat())).is_ok());
    }
}

#[test]
fn group_cli_integration_preserves_authority_and_does_not_load_team() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("coven.json"), "not team JSON").unwrap();
    std::fs::create_dir(root.path().join("state")).unwrap();
    let workspace = root.path().to_str().unwrap();
    for (tail, expected) in [
        (vec![workspace, "g", "--name", "caf\u{e9}", "--json"], 0),
        (vec![workspace, "g", "--expect-revision", "stale"], 1),
    ] {
        let (cli, _) = parse(&group_args("create", &tail)).unwrap();
        let output = super::super::super::run(cli);
        if expected == 0 {
            let output = output.unwrap();
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("caf\\u00e9"));
            assert!(!output.stdout.contains('\n'));
        } else {
            assert_eq!(output.err().unwrap().code(), "groups_revision_conflict");
        }
    }
    let binding = coven::values::parse(br#"{"presentation":{"adapter":"kitsune","endpoint":{"path":"/not/socket","device":184467440737095516160000000000000000000,"inode":0},"terminal_id":"term","pane_id_at_binding":"p"},"evidence":{"provenance":"kitsune.session.snapshot","observed_at":"2024-W01T12Z","agent_kind_hint":null,"native_reference":null,"native_binding":"unestablished"}}"#, coven::values::Profile::Authority).unwrap();
    coven::groups::add_member(
        root.path(),
        coven::groups::AddMember {
            group: "g",
            member: "m",
            role: "role",
            binding: &binding,
            rebind: false,
            expected_revision: None,
        },
    )
    .unwrap();
    for json in [true, false] {
        let mut tail = vec![workspace];
        if json {
            tail.push("--json");
        }
        let (cli, _) = parse(&group_args("list", &tail)).unwrap();
        let output = super::super::super::run(cli).unwrap();
        assert!(
            output
                .stdout
                .contains("184467440737095516160000000000000000000")
        );
        assert!(!output.stdout.contains("publication_counters"));
        let mut bytes = Vec::new();
        super::super::super::write_stdout(&mut bytes, &output.stdout, false).unwrap();
        assert!(bytes.ends_with(b"}\n"));
        assert!(!bytes.ends_with(b"\n\n"));
        assert!(bytes.is_ascii());
    }
}

#[test]
fn group_add_preflight_precedes_socket_access_and_lock_creation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("coven.json"), "not JSON").unwrap();
    std::fs::create_dir(root.path().join("state")).unwrap();
    let workspace = root.path().to_str().unwrap();
    for (group, member, role, pane, expected, code) in [
        ("INVALID", "m", "role", "p", None, "groups_invalid_selector"),
        ("g", "INVALID", "role", "p", None, "groups_invalid_selector"),
        ("g", "m", "", "p", None, "groups_invalid_string"),
        ("g", "m", "role", "bad pane", None, "groups_invalid_pane"),
        (
            "g",
            "m",
            "role",
            "p",
            Some("stale"),
            "groups_revision_conflict",
        ),
        ("g", "m", "role", "p", None, "groups_uninitialized"),
    ] {
        let mut tail = vec![
            workspace, group, "--member", member, "--role", role, "--socket", "invalid", "--pane",
            pane,
        ];
        if let Some(expected) = expected {
            tail.extend(["--expect-revision", expected]);
        }
        let (cli, _) = parse(&group_args("add", &tail)).unwrap();
        assert_eq!(super::super::super::run(cli).err().unwrap().code(), code);
        assert_eq!(
            std::fs::read_dir(root.path().join("state"))
                .unwrap()
                .count(),
            0
        );
    }
}
