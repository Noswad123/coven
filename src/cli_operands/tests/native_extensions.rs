use super::*;

#[test]
fn native_extension_operands_are_literal_and_effects_explicit() {
    for provider in ["pi", "opencode"] {
        for operation in ["plan", "install", "uninstall", "rollback"] {
            let (_, matches) = parse(&args(&[
                "coven",
                "agent",
                "native-extensions",
                operation,
                "--provider",
                provider,
                "--extension-root",
                "-1root",
                "--executable",
                "-2binary",
                "--socket",
                "-3socket",
                "--json",
            ]))
            .unwrap();
            let leaf = matches
                .subcommand_matches("agent")
                .unwrap()
                .subcommand_matches("native-extensions")
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
            assert!(leaf.get_flag("json"));
        }
    }
    for extra in [
        vec!["--provider", "codex", "--socket", "/socket"],
        vec!["--provider", "copilot", "--socket", "/socket"],
        vec!["--provider", "pi", "--socket", "/socket", "--from-kitsune"],
    ] {
        assert!(
            parse(&args(
                &[
                    vec![
                        "coven",
                        "agent",
                        "native-extensions",
                        "plan",
                        "--extension-root",
                        "/root"
                    ],
                    extra.clone()
                ]
                .concat()
            ))
            .is_err(),
            "unexpected acceptance: {extra:?}"
        );
    }
}
