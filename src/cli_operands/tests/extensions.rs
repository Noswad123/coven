use super::*;

#[test]
fn opencode_extension_provider_and_both_event_choices_preserve_literal_operands() {
    for event in ["session.created", "session.updated"] {
        let (_, matches) = parse(&args(&[
            "coven",
            "agent",
            "report-extension",
            "--provider",
            "opencode",
            "--event",
            event,
            "--socket",
            "-1socket",
            "--json",
        ]))
        .unwrap();
        let leaf = matches
            .subcommand_matches("agent")
            .unwrap()
            .subcommand_matches("report-extension")
            .unwrap();
        assert_eq!(leaf.get_one::<String>("provider").unwrap(), "opencode");
        assert_eq!(leaf.get_one::<String>("event").unwrap(), event);
        assert_eq!(leaf.get_one::<String>("socket").unwrap(), "-1socket");
    }
    for operation in ["plan", "install", "uninstall", "rollback"] {
        let (_, matches) = parse(&args(&[
            "coven",
            "agent",
            "extensions",
            operation,
            "--provider",
            "opencode",
            "--extension-root",
            "-1root",
            "--executable",
            "-2binary",
            "--socket",
            "-3socket",
        ]))
        .unwrap();
        let leaf = matches
            .subcommand_matches("agent")
            .unwrap()
            .subcommand_matches("extensions")
            .unwrap()
            .subcommand_matches(operation)
            .unwrap();
        assert_eq!(
            leaf.get_one::<PathBuf>("extension_root").unwrap(),
            &PathBuf::from("-1root")
        );
        assert_eq!(leaf.get_one::<String>("executable").unwrap(), "-2binary");
        assert_eq!(leaf.get_one::<String>("socket").unwrap(), "-3socket");
        assert!(!leaf.get_flag("apply"));
    }
    assert!(
        parse(&args(&[
            "coven",
            "agent",
            "report",
            "--provider",
            "opencode",
            "--socket",
            "/abs"
        ]))
        .is_err()
    );
}

#[test]
fn pi_extension_routes_restore_literal_paths_and_keep_effects_explicit() {
    for operation in ["plan", "install", "uninstall", "rollback"] {
        let (_, matches) = parse(&args(&[
            "coven",
            "agent",
            "extensions",
            operation,
            "--provider",
            "pi",
            "--extension-root",
            "-1e3/extensions",
            "--executable",
            "-2coven",
            "--socket",
            "-3socket",
            "--migrate-kitsune",
            "--json",
        ]))
        .unwrap();
        let leaf = matches
            .subcommand_matches("agent")
            .unwrap()
            .subcommand_matches("extensions")
            .unwrap()
            .subcommand_matches(operation)
            .unwrap();
        assert_eq!(
            leaf.get_one::<PathBuf>("extension_root").unwrap(),
            &PathBuf::from("-1e3/extensions")
        );
        assert_eq!(leaf.get_one::<String>("executable").unwrap(), "-2coven");
        assert_eq!(leaf.get_one::<String>("socket").unwrap(), "-3socket");
        assert!(!leaf.get_flag("apply"));
        assert!(leaf.get_flag("migrate_kitsune") && leaf.get_flag("json"));
        for bad in ["--provider=claude", "--config-root=/abs", "--unknown"] {
            assert!(
                parse(&args(&[
                    "coven",
                    "agent",
                    "extensions",
                    operation,
                    "--provider",
                    "pi",
                    "--extension-root",
                    "/abs",
                    bad
                ]))
                .is_err()
            );
        }
    }
}

#[test]
fn extension_report_requires_explicit_pi_event_and_stays_separate_from_native_report() {
    let (_, matches) = parse(&args(&[
        "coven",
        "agent",
        "report-extension",
        "--provider",
        "pi",
        "--event",
        "session_start",
        "--socket",
        "-1socket",
        "--json",
    ]))
    .unwrap();
    let leaf = matches
        .subcommand_matches("agent")
        .unwrap()
        .subcommand_matches("report-extension")
        .unwrap();
    assert_eq!(leaf.get_one::<String>("socket").unwrap(), "-1socket");
    assert_eq!(leaf.get_one::<String>("event").unwrap(), "session_start");
    for operands in [
        vec!["--provider", "pi", "--socket", "/abs"],
        vec!["--event", "session_start", "--socket", "/abs"],
        vec![
            "--provider",
            "claude",
            "--event",
            "session_start",
            "--socket",
            "/abs",
        ],
        vec![
            "--provider",
            "pi",
            "--event",
            "SessionStart",
            "--socket",
            "/abs",
        ],
        vec![
            "--provider",
            "pi",
            "--event",
            "session_start",
            "--socket",
            "/abs",
            "--apply",
        ],
    ] {
        assert!(
            parse(&args(
                &[vec!["coven", "agent", "report-extension"], operands].concat()
            ))
            .is_err()
        );
    }
    assert!(
        parse(&args(&[
            "coven",
            "agent",
            "report",
            "--provider",
            "pi",
            "--socket",
            "/abs"
        ]))
        .is_err()
    );
}
