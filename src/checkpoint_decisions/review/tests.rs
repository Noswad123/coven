use super::*;
use std::io::Cursor;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["a"]}"#,
    )
    .unwrap();
    std::fs::create_dir(root.path().join("checkpoints")).unwrap();
    for id in ["one", "two"] {
        std::fs::write(
            root.path().join(format!("checkpoints/{id}.md")),
            "# Plan\nStatus: pending\n",
        )
        .unwrap();
    }
    root
}

#[test]
fn characterization_displayed_revision_no_refresh_retry_and_conflict_survives_eof() {
    for input in ["a\n", "s\nMore QA\n.\n"] {
        let root = fixture();
        let revision = field(&view(root.path(), "one").unwrap(), "revision")
            .unwrap()
            .to_owned();
        let mut output = Vec::new();
        let mut calls = 0;
        let status = review_with(
            root.path(),
            &["one".into(), "two".into()],
            "all",
            "",
            &mut Cursor::new(input),
            &mut output,
            &mut |request| {
                calls += 1;
                assert_eq!(
                    request.expected_revision.as_deref(),
                    Some(revision.as_str())
                );
                assert_eq!(request.source, "review");
                std::fs::write(
                    root.path().join("checkpoints/one.md"),
                    "Changed during user input\n",
                )
                .unwrap();
                Err(Error("stale_checkpoint_revision"))
            },
        )
        .unwrap();
        assert_eq!(status, 1);
        assert_eq!(calls, 1);
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains(&revision));
        assert!(text.contains("decision skipped"));
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn characterization_uncertainty_survives_quit_and_multiline_eof() {
    for input in ["a\nq\n", "s\nMore QA"] {
        let root = fixture();
        let status = review_with(
            root.path(),
            &["one".into(), "two".into()],
            "all",
            "",
            &mut Cursor::new(input),
            &mut Vec::new(),
            &mut |_| {
                Ok(CommandResult {
                    body: "uncertain".into(),
                    exit_code: 2,
                })
            },
        )
        .unwrap();
        assert_eq!(status, 2);
    }
}

#[test]
fn characterization_bad_batch_rejected_before_prompt_and_request_discovery_order() {
    let root = fixture();
    let mut out = Vec::new();
    assert!(
        review_with(
            root.path(),
            &["one".into(), "../bad".into()],
            "all",
            "",
            &mut Cursor::new("a\n"),
            &mut out,
            &mut |_| panic!("no submission")
        )
        .is_err()
    );
    assert!(out.is_empty());
    std::fs::create_dir(root.path().join("logs")).unwrap();
    std::fs::write(root.path().join("logs/events.jsonl"), b"{\"checkpoint\":\"two\",\"type\":\"checkpoint.request\",\"ts\":\"2024-01-02\"}\n{\"checkpoint\":\"one\",\"type\":\"checkpoint.request\",\"ts\":\"2024-01-01\"}\n").unwrap();
    assert_eq!(candidates(root.path()).unwrap(), vec!["one", "two"]);
}
