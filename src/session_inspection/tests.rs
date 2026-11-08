use super::*;

#[test]
fn endpoint_comparison_never_rounds_or_requires_bounded_authority_integers() {
    for n in [0, 1, u64::MAX] {
        let value = Native::integer(&n.to_string()).unwrap();
        assert!(integer_matches(&value, n));
        assert!(!integer_matches(&value, n.wrapping_add(1)));
    }
    assert!(!integer_matches(
        &Native::integer(&"9".repeat(4300)).unwrap(),
        u64::MAX
    ));
    assert!(!integer_matches(&Native::float(1.).unwrap(), 1));
    assert!(!integer_matches(&Native::Bool(true), 1));
}

#[test]
fn mapping_ignores_order_but_preserves_the_entire_locator_cohort() {
    let before = json!({"panes":[{"pane_id":"p1","terminal_id":"t1","tab_id":"tab","workspace_id":"w"},
        {"pane_id":"p2","terminal_id":"t2","tab_id":"tab","workspace_id":"w"}]});
    let mut after = before.clone();
    after["panes"].as_array_mut().unwrap().reverse();
    assert_eq!(mapping(&before), mapping(&after));
    for key in ["pane_id", "terminal_id", "tab_id", "workspace_id"] {
        let mut changed = after.clone();
        changed["panes"][0][key] = json!("changed");
        assert_ne!(mapping(&before), mapping(&changed));
    }
}

#[test]
fn human_output_escapes_control_bytes_and_does_not_claim_absence() {
    let report = json!({"assessment":"observed","targets":[{"pane_id":"p","terminal_id":"terminal",
        "recognition":"unknown","associations":[{"group_key":"team","member_key":"member","role":"Review\u{1b}[31m\nsecret","binding_check":"legacy_evidence_not_revalidated"}]}],
        "declarations":{"unmatched":[]},"diagnostics":[]});
    let output = human(&report);
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains("\nsecret"));
    assert!(output.contains("unknown"));
    assert!(!output.contains("absent"));
}
