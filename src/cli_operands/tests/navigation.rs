use super::*;

#[test]
fn navigation_operands_preserve_exact_values() {
    for (terminal_args, expected) in [
        (vec!["--terminal", "-1term"], "-1term"),
        (vec!["--terminal=-1term"], "-1term"),
        (vec!["--terminal", "-\u{0661}term"], "-\u{0661}term"),
        (vec!["--terminal=-\u{0661}term"], "-\u{0661}term"),
    ] {
        let mut values = vec![
            "coven",
            "agent",
            "associate",
            "-1obs",
            "--from-kitsune",
            "--kitsune-socket",
            "/kit",
            "--expected-epoch",
            "-1epoch",
            "--expected-revision=-1revision",
        ];
        values.extend(terminal_args);
        let (_, matches) = parse(&args(&values)).unwrap();
        let leaf = matches
            .subcommand_matches("agent")
            .unwrap()
            .subcommand_matches("associate")
            .unwrap();
        for (field, expected) in [
            ("terminal", expected),
            ("observation", "-1obs"),
            ("expected_epoch", "-1epoch"),
            ("expected_revision", "-1revision"),
        ] {
            assert_eq!(leaf.get_one::<String>(field).unwrap(), expected);
        }
        assert!(!leaf.get_flag("apply"));
    }
    for observation in ["-1obs", "-\u{0661}obs", "--json"] {
        let (_, matches) = parse(&args(&[
            "coven",
            "agent",
            "associate",
            "--from-kitsune",
            "--kitsune-socket",
            "/kit",
            "--terminal=-1term",
            "--expected-epoch",
            "epoch",
            "--",
            observation,
        ]))
        .unwrap();
        let leaf = matches
            .subcommand_matches("agent")
            .unwrap()
            .subcommand_matches("associate")
            .unwrap();
        assert_eq!(leaf.get_one::<String>("observation").unwrap(), observation);
        assert_eq!(leaf.get_one::<String>("terminal").unwrap(), "-1term");
        assert!(!leaf.get_flag("json"));
        assert!(!leaf.get_flag("apply"));
    }
}

#[test]
fn checked_navigation_and_attention_cli_selectors_and_previews() {
    for values in [
        vec!["agent", "observations", "--from-kitsune", "--json"],
        vec![
            "agent",
            "attention",
            "list",
            "--socket",
            "/receiver",
            "--json",
        ],
        vec![
            "agent",
            "attention",
            "acknowledge",
            "obs",
            "--from-kitsune",
            "--expected-epoch",
            "epoch",
            "--expected-revision",
            "rev",
        ],
        vec![
            "agent",
            "associate",
            "obs",
            "--from-kitsune",
            "--kitsune-socket",
            "/kit",
            "--terminal",
            "term",
            "--expected-epoch",
            "epoch",
        ],
        vec![
            "agent",
            "dissociate",
            "obs",
            "--socket",
            "/receiver",
            "--expected-epoch",
            "epoch",
            "--expected-revision",
            "rev",
        ],
        vec![
            "agent",
            "focus",
            "obs",
            "--socket",
            "/receiver",
            "--expected-epoch",
            "epoch",
            "--expected-revision",
            "rev",
            "--execute",
        ],
    ] {
        let mut input = vec!["coven"];
        input.extend(values);
        parse(&args(&input)).unwrap();
    }
    assert!(
        parse(&args(&[
            "coven",
            "agent",
            "observations",
            "--socket",
            "/receiver",
            "--from-kitsune"
        ]))
        .is_err()
    );
    assert!(
        parse(&args(&[
            "coven",
            "agent",
            "associate",
            "obs",
            "--from-kitsune",
            "--kitsune-socket",
            "/kit",
            "--terminal",
            "term"
        ]))
        .is_err()
    );
}
