use super::*;

#[test]
fn negative_prefixes_restore_workspace_and_all_string_fields() {
    for literal in [
        "-2026-demo",
        "-1team",
        "-.5thing",
        "-1e3",
        "-1.",
        "-0xFF",
        "-\u{0661}suffix",
        "-.\u{ff19}suffix",
        "-\u{10d40}suffix",
        "-.\u{1e5f1}suffix",
        "__coven_checkpoint_operand_0",
    ] {
        for option in [
            None,
            Some("--name"),
            Some("--na"),
            Some("--ag"),
            Some("--op"),
            Some("--ex"),
        ] {
            let mut operands = vec!["--blank"];
            operands.extend(option);
            operands.push(literal);
            let (_, matches) = parse(&args(&[vec!["coven", "init"], operands].concat())).unwrap();
            let init = matches.subcommand_matches("init").unwrap();
            match option {
                None => assert_eq!(
                    init.get_one::<PathBuf>("workspace").unwrap().as_os_str(),
                    literal
                ),
                Some(option) => {
                    let field = match option {
                        "--name" | "--na" => "name",
                        "--ag" => "agents",
                        "--op" => "opencode_command",
                        _ => "example",
                    };
                    assert_eq!(init.get_one::<String>(field).unwrap(), literal);
                }
            }
        }
    }
}

#[test]
fn attached_values_repeats_and_terminator_remain_literal() {
    for operands in [
        vec![
            "--name=-inf",
            "--op=--unknown",
            "--ag=-1agent",
            "--ex=-.5example",
        ],
        vec!["--na", "first", "-2026-demo", "--name=-1team", "--blank"],
        vec!["--blank", "--", "--unknown"],
        vec!["--", "-inf"],
        vec!["--name=__coven_checkpoint_operand_0", "-1workspace"],
    ] {
        let (_, matches) =
            parse(&args(&[vec!["coven", "init"], operands.clone()].concat())).unwrap();
        let init = matches.subcommand_matches("init").unwrap();
        if operands[0] == "--name=-inf" {
            for (field, value) in [
                ("name", "-inf"),
                ("opencode_command", "--unknown"),
                ("agents", "-1agent"),
                ("example", "-.5example"),
            ] {
                assert_eq!(init.get_one::<String>(field).unwrap(), value);
            }
        } else if operands[0] == "--na" {
            assert_eq!(init.get_one::<String>("name").unwrap(), "-1team");
            assert_eq!(
                init.get_one::<PathBuf>("workspace").unwrap(),
                &PathBuf::from("-2026-demo")
            );
        } else if operands[0] == "--name=__coven_checkpoint_operand_0" {
            assert_eq!(
                init.get_one::<String>("name").unwrap(),
                "__coven_checkpoint_operand_0"
            );
            assert_eq!(
                init.get_one::<PathBuf>("workspace").unwrap(),
                &PathBuf::from("-1workspace")
            );
        } else {
            assert_eq!(
                init.get_one::<PathBuf>("workspace").unwrap().as_os_str(),
                *operands.last().unwrap()
            );
        }
    }
}

#[test]
fn unknown_flags_missing_values_and_nondecimal_prefixes_are_rejected() {
    for literal in [
        "-inf",
        "-nan",
        "-unknown",
        "-.thing",
        "--unknown",
        "--json",
        "-\u{2165}",
        "-\u{00bd}",
        "-\u{00b2}",
        "--1",
        "-..1",
    ] {
        for option in [
            None,
            Some("--name"),
            Some("--na"),
            Some("--ag"),
            Some("--op"),
            Some("--ex"),
        ] {
            let mut operands = vec!["--blank"];
            operands.extend(option);
            operands.push(literal);
            assert!(
                parse(&args(&[vec!["coven", "init"], operands].concat())).is_err(),
                "{literal} {option:?}"
            );
        }
    }
    for operands in [
        vec!["--name", "--"],
        vec!["--na", "--blank"],
        vec!["--unknown=value"],
        vec!["--blank=false"],
        vec!["one", "two"],
        vec!["--name"],
        vec!["--", "one", "two"],
    ] {
        assert!(parse(&args(&[vec!["coven", "init"], operands].concat())).is_err());
    }
}

#[test]
fn multiplexer_choices_keep_invalid_raw_values_and_reject_before_override() {
    for option in ["--multiplexer", "--m", "--mul"] {
        for value in ["-1mux", "-.5mux", "-\u{0661}mux", "invalid"] {
            for attached in [false, true] {
                let attached_option = format!("{option}={value}");
                let operands = if attached {
                    vec![attached_option.as_str()]
                } else {
                    vec![option, value]
                };
                let arguments = args(
                    &[
                        vec!["coven", "init"],
                        operands,
                        vec!["--multiplexer", "tmux"],
                    ]
                    .concat(),
                );
                assert_eq!(checkpoint_compat(&arguments).1, arguments);
                let error = parse(&arguments).err().unwrap();
                // This build omits Clap's error-context feature. Check the raw
                // adapter arguments above and the validator's error category.
                assert_eq!(
                    error.kind(),
                    if attached || !value.starts_with('-') {
                        ErrorKind::InvalidValue
                    } else {
                        ErrorKind::UnknownArgument
                    }
                );
                assert!(!error.to_string().contains("__coven_checkpoint_operand_"));
            }
        }
    }
}
