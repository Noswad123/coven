use super::*;

const ID: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn characterization_legacy_crlf_and_custom_bytes_keep_scope() {
    let raw = b"# Plan\r\nStatus: pending\r\n\r\n## User decision\r\n -\t[X]\tAPPROVED \t\r\n- [ ] Rejected / needs changes\r\n## Notes\r\nCustom\r\n";
    let before = inspect_bytes("plan", Some(raw)).unwrap();
    let (output, final_view) = prepare(
        "plan",
        Some(raw),
        "changes_requested",
        "QA <yes>",
        ID,
        "now",
        "cli",
        None,
    )
    .unwrap();
    assert_eq!(
        field(&before, "scope_revision").unwrap(),
        field(&final_view, "scope_revision").unwrap()
    );
    assert!(contains(&output, b"Status: changes_requested\r\n"));
    assert!(contains(&output, b" -\t[ ]\tAPPROVED \t\r\n"));
    assert!(contains(&output, b"QA \\u003cyes\\u003e"));
    // Frozen bytes independently produced by runner.checkpoints.prepare (-B).
    let expected = concat!(
        "<!-- coven-checkpoint-decision:v1\n",
        "{\"action\":\"changes_requested\",\"checkpoint\":\"plan\",\"decided_at\":\"now\",\"decision_id\":\"0123456789abcdef0123456789abcdef\",\"reviewed_revision\":\"checkpoint-snapshot-v1:773733fc6e03863076379cc4a99856097d0ab6592e64c755afa910c2a3b3f7c3\",\"scope_revision\":\"checkpoint-scope-v1:72ee0b29cfc4bf52a9cdf1a24343c651ae4fada4f6e667284c68aca56b33a71f\",\"source\":\"cli\",\"version\":1}\n-->\n",
        "# Plan\r\nStatus: changes_requested\r\n\r\n## User decision\r\n -\t[ ]\tAPPROVED \t\r\n- [x] Rejected / needs changes\r\n## Notes\r\nCustom\r\n\n",
        "<!-- coven-checkpoint-history:v1\n",
        "{\"action\":\"changes_requested\",\"checkpoint\":\"plan\",\"decided_at\":\"now\",\"decision_id\":\"0123456789abcdef0123456789abcdef\",\"notes\":\"QA \\u003cyes\\u003e\",\"reviewed_revision\":\"checkpoint-snapshot-v1:773733fc6e03863076379cc4a99856097d0ab6592e64c755afa910c2a3b3f7c3\",\"scope_revision\":\"checkpoint-scope-v1:72ee0b29cfc4bf52a9cdf1a24343c651ae4fada4f6e667284c68aca56b33a71f\",\"source\":\"cli\",\"version\":1}\n-->\n"
    );
    assert_eq!(output, expected.as_bytes());
    assert_eq!(
        field(&final_view, "revision").unwrap(),
        "checkpoint-snapshot-v1:14873851f3abe804ca535fe81f75e41793d8590b3b1b4c41d5ab8e08b7d8aaca"
    );
}

#[test]
fn characterization_missing_empty_no_status_history_and_injection_refusal() {
    let (first, view) = prepare(
        "plan",
        None,
        "approved",
        "ok",
        ID,
        "now",
        "review",
        Some(b""),
    )
    .unwrap();
    assert_eq!(field(&view, "effective_status").unwrap(), "approved");
    assert!(contains(&first, b"checkpoint-missing-v1:plan"));
    let (second, view) = prepare(
        "plan",
        Some(&first),
        "approved",
        "again",
        "1123456789abcdef0123456789abcdef",
        "later",
        "cli",
        None,
    )
    .unwrap();
    assert_eq!(
        view.as_object()
            .unwrap()
            .get("history")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        field(
            &inspect_bytes("plan", Some(&first)).unwrap(),
            "scope_revision"
        )
        .unwrap(),
        field(&view, "scope_revision").unwrap()
    );
    assert!(second.len() > first.len());
    for notes in [
        "raw -->",
        "coven-checkpoint-x",
        "## User decision\n",
        "```\nunclosed",
    ] {
        assert!(prepare("plan", Some(b""), "approved", notes, ID, "now", "cli", None).is_err());
    }
}

#[test]
fn characterization_writer_refuses_reader_limit_expansion() {
    assert_eq!(
        prepare(
            "plan",
            Some(&vec![b'a'; MAX_INPUT]),
            "approved",
            "",
            ID,
            "now",
            "cli",
            None
        )
        .unwrap_err()
        .code(),
        "unsupported_checkpoint_output_profile"
    );
}

#[test]
fn characterization_exact_output_ceiling_and_escaped_notes_expansion() {
    let (minimum, _) = prepare("plan", Some(b""), "approved", "", ID, "now", "cli", None).unwrap();
    let padding = vec![b'a'; MAX_INPUT - minimum.len()];
    let (output, _) = prepare(
        "plan",
        Some(&padding),
        "approved",
        "",
        ID,
        "now",
        "cli",
        None,
    )
    .unwrap();
    assert_eq!(output.len(), MAX_INPUT);
    let mut larger = padding;
    larger.push(b'a');
    assert_eq!(
        prepare(
            "plan",
            Some(&larger),
            "approved",
            "",
            ID,
            "now",
            "cli",
            None
        )
        .unwrap_err()
        .code(),
        "unsupported_checkpoint_output_profile"
    );
    let notes = "\u{2603}".repeat(MAX_INPUT / 6);
    assert!(notes.len() < MAX_INPUT);
    assert_eq!(
        prepare(
            "plan",
            Some(b""),
            "approved",
            &notes,
            ID,
            "now",
            "cli",
            None
        )
        .unwrap_err()
        .code(),
        "unsupported_checkpoint_output_profile"
    );
}

#[test]
fn characterization_reapproval_repairs_known_presentation_not_ambiguous_machinery() {
    let (first, _) = prepare(
        "plan",
        Some(b"# Plan\nStatus: pending\n"),
        "approved",
        "",
        ID,
        "now",
        "cli",
        None,
    )
    .unwrap();
    let changed = String::from_utf8(first.clone())
        .unwrap()
        .replace("Status: approved", "Status: pending");
    assert_eq!(
        field(
            &inspect_bytes("plan", Some(changed.as_bytes())).unwrap(),
            "effective_status"
        )
        .unwrap(),
        "decision_invalid"
    );
    let (second, view) = prepare(
        "plan",
        Some(changed.as_bytes()),
        "approved",
        "repair",
        "1123456789abcdef0123456789abcdef",
        "now",
        "cli",
        None,
    )
    .unwrap();
    assert_eq!(field(&view, "effective_status").unwrap(), "approved");
    assert!(second.len() > first.len());
    for invalid in [
        b"Status: pending\nStatus: approved\n".as_slice(),
        b"<!-- coven-checkpoint-decision:v2\n{}\n-->\n",
        b"## User decision\n- [ ] Approved\n- [ ] Approved\n",
    ] {
        assert!(
            prepare(
                "plan",
                Some(invalid),
                "approved",
                "",
                ID,
                "now",
                "cli",
                None
            )
            .is_err()
        );
    }
    assert_eq!(
        prepare(
            "plan",
            Some(&first),
            "approved",
            "duplicate",
            ID,
            "now",
            "cli",
            None
        )
        .unwrap_err()
        .code(),
        "invalid_checkpoint_duplicate_history"
    );
}
