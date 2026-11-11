use super::*;

#[test]
fn discovery_restores_literal_selectors_and_keeps_numeric_options_typed() {
    let (_, matches) = parse(&args(&[
        "coven",
        "agent",
        "discover",
        "--socket",
        "-1socket",
        "--pane",
        "-2pane",
        "--expected-terminal",
        "-3terminal",
        "--timeout",
        "-.5",
        "--max-response-bytes",
        "-1",
        "--watch",
        "--duration",
        ".5",
        "--interval",
        ".1",
        "--present",
        "--source",
        "-4source",
        "--json",
    ]))
    .unwrap();
    let leaf = matches
        .subcommand_matches("agent")
        .unwrap()
        .subcommand_matches("discover")
        .unwrap();
    for (key, expected) in [
        ("socket", "-1socket"),
        ("pane", "-2pane"),
        ("expected_terminal", "-3terminal"),
        ("source", "-4source"),
    ] {
        assert_eq!(leaf.get_one::<String>(key).unwrap(), expected);
    }
    assert_eq!(leaf.get_one::<f64>("timeout"), Some(&-0.5));
    assert_eq!(leaf.get_one::<i64>("max_response_bytes"), Some(&-1));
    assert_eq!(leaf.get_one::<f64>("duration"), Some(&0.5));
    assert_eq!(leaf.get_one::<f64>("interval"), Some(&0.1));
    assert!(leaf.get_flag("watch") && leaf.get_flag("present") && leaf.get_flag("json"));
}

#[test]
fn discovery_has_no_workspace_or_implicit_watch_presentation() {
    let (_, matches) = parse(&args(&["coven", "agent", "discover", "--socket", "/abs"])).unwrap();
    let leaf = matches
        .subcommand_matches("agent")
        .unwrap()
        .subcommand_matches("discover")
        .unwrap();
    assert!(!leaf.get_flag("watch") && !leaf.get_flag("present"));
    assert_eq!(leaf.get_one::<f64>("timeout"), Some(&3.0));
    assert_eq!(leaf.get_one::<i64>("max_response_bytes"), Some(&1_048_576));
    for extra in [
        vec!["workspace"],
        vec!["--duration", "1"],
        vec!["--interval", "1"],
        vec!["--expected-terminal", "t"],
        vec!["--source", "coven-discovery"],
        vec!["--present", "--source", "coven-discovery"],
        vec!["--watch", "--present"],
        vec!["--apply"],
        vec!["--runtime", "claude"],
    ] {
        let argv = [
            vec!["coven", "agent", "discover", "--socket", "/abs"],
            extra,
        ]
        .concat();
        assert!(parse(&args(&argv)).is_err(), "accepted {argv:?}");
    }
}
