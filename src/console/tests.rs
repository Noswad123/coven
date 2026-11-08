use super::*;
use checkpoint_decisions::{CommandResult, DecisionRequest};
use std::io::Cursor;

#[derive(Default)]
struct FakeServices {
    calls: Vec<String>,
    decisions: Vec<DecisionRequest>,
    conflict: bool,
    decision_code: u8,
    ordinary_error: bool,
}

impl Services for FakeServices {
    fn initial(&mut self, _: &Path) -> Result<(String, Vec<String>)> {
        self.calls.push("initial".into());
        Ok(("test".into(), vec!["a".into()]))
    }
    fn standup(&mut self, _: &Path, out: &mut impl Write, _: &mut impl Write) -> Result<()> {
        self.calls.push("standup".into());
        writeln!(out, "standup output")?;
        Ok(())
    }
    fn status(&mut self, _: &Path, out: &mut impl Write, _: &mut impl Write) -> Result<()> {
        self.calls.push("status".into());
        write!(out, "dashboard\n\n")?;
        Ok(())
    }
    fn next(&mut self, _: &Path, out: &mut impl Write, _: &mut impl Write) -> Result<()> {
        self.calls.push("next".into());
        writeln!(out, "next action")?;
        Ok(())
    }
    fn agents(&mut self, _: &Path, out: &mut impl Write, _: &mut impl Write) -> Result<()> {
        self.calls.push("agents".into());
        writeln!(out, "agents output")?;
        Ok(())
    }
    fn send(
        &mut self,
        _: &Path,
        selector: &str,
        text: &str,
        _: &mut impl Write,
        _: &mut impl Write,
    ) -> Result<()> {
        self.calls.push(format!("send:{selector}:{text}"));
        if self.ordinary_error {
            return Err(Error("unknown_agent"));
        }
        Ok(())
    }
    fn lead(
        &mut self,
        _: &Path,
        tokens: &[&str],
        _: &mut impl Write,
        _: &mut impl Write,
    ) -> Result<()> {
        self.calls.push(format!("lead:{}", tokens.join("|")));
        Ok(())
    }
    fn display(&mut self, root: &Path, id: &str, out: &mut impl Write) -> Result<String> {
        self.calls.push(format!("display:{id}"));
        checkpoint_decisions::display_io(root, id, out)
    }
    fn decide(&mut self, root: &Path, request: DecisionRequest) -> Result<CommandResult> {
        let bytes = std::fs::read(root.join(format!("checkpoints/{}.md", request.checkpoint))).ok();
        let current = crate::checkpoints::inspect_bytes(&request.checkpoint, bytes.as_deref())?;
        let current = current
            .as_object()
            .unwrap()
            .get("revision")
            .unwrap()
            .as_str()
            .unwrap();
        let stale = request.expected_revision.as_deref() != Some(current);
        self.decisions.push(request);
        if self.conflict || stale {
            return Err(Error("stale_checkpoint_revision"));
        }
        Ok(CommandResult {
            body: "[INFO] Recorded decision".into(),
            exit_code: self.decision_code,
        })
    }
    fn review(
        &mut self,
        root: &Path,
        ids: &[String],
        input: &mut impl BufRead,
        out: &mut impl Write,
    ) -> Result<u8> {
        self.calls.push(format!("review:{}", ids.join(",")));
        checkpoint_decisions::review_with_for_test(
            root,
            ids,
            "all",
            "",
            input,
            out,
            &mut |request| self.decide(root, request),
        )
    }
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["a"]}"#,
    )
    .unwrap();
    std::fs::create_dir(root.path().join("checkpoints")).unwrap();
    std::fs::write(
        root.path().join("checkpoints/one.md"),
        "# Plan\nStatus: pending\n",
    )
    .unwrap();
    root
}

fn session(root: &Path, input: &str, services: &mut FakeServices) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = run_with(root, &mut Cursor::new(input), &mut out, &mut err, services).unwrap();
    (
        status,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn complete_dispatch_help_usage_exact_aliases_and_literal_quotes() {
    let root = fixture();
    let mut services = FakeServices::default();
    let (status, out, err) = session(
        root.path(),
        "\nhelp\nh\n?\nstandup\nstatus\nnext\nagents\nagent status\nsend\nsend a\nsuggest\nsuggest one\nsend a \"hello  world\"\na 'keep quotes'  here\nall hi\nlead\nlead set a ignored\nlead --clear\nagent  status\nstandup extra\na\nquit extra\nquit\n",
        &mut services,
    );
    assert_eq!(status, 0);
    assert!(err.is_empty());
    assert!(out.starts_with(&format!(
        "coven console: test ({})\n{HELP}\n",
        root.path().display()
    )));
    assert_eq!(out.matches("Commands:").count(), 4);
    assert_eq!(out.matches("Usage: send <agent|all> <message>").count(), 2);
    assert_eq!(
        out.matches("Usage: suggest <checkpoint> <requested changes>")
            .count(),
        2
    );
    assert_eq!(out.matches("Unknown command. Type `help`.").count(), 4);
    assert_eq!(
        services.calls,
        [
            "initial",
            "standup",
            "status",
            "next",
            "agents",
            "agents",
            "send:a:\"hello  world\"",
            "send:a:'keep quotes'  here",
            "send:all:hi",
            "lead:",
            "lead:set|a|ignored",
            "lead:--clear"
        ]
    );
}

#[test]
fn python_whitespace_and_maxsplit_preserve_message_remainder() {
    let root = fixture();
    let mut services = FakeServices::default();
    session(
        root.path(),
        "\u{1c}send\u{85}a\t\"one\"  two\u{1f}\na\u{2003}one\t two\nexit\n",
        &mut services,
    );
    assert_eq!(
        services.calls,
        ["initial", "send:a:\"one\"  two", "send:a:one\t two"]
    );
}

#[test]
fn confirmation_defaults_no_and_eof_never_submits() {
    for input in [
        "approve\n\nquit\n",
        "approve one\nno\nexit\n",
        "suggest one changes\n",
        "approve one\ntrue\nq\n",
    ] {
        let root = fixture();
        let mut services = FakeServices::default();
        let (status, out, _) = session(root.path(), input, &mut services);
        assert_eq!(status, 0);
        assert!(services.decisions.is_empty());
        assert!(out.contains("Snapshot: checkpoint-"));
        assert!(out.contains("[y/N] > "));
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn decisions_bind_exact_displayed_revision_source_and_literal_notes() {
    let root = fixture();
    let mut services = FakeServices::default();
    let (_, out, _) = session(
        root.path(),
        "suggest one \"keep quotes\"  and  spaces\n YES \napprove one ignored\ny\nquit\n",
        &mut services,
    );
    assert_eq!(services.decisions.len(), 2);
    for request in &services.decisions {
        assert_eq!(request.source, "console");
        assert_eq!(request.agent, "all");
        assert_eq!(request.checkpoint, "one");
        assert!(out.contains(&format!(
            "Snapshot: {}\n",
            request.expected_revision.as_ref().unwrap()
        )));
    }
    assert_eq!(services.decisions[0].notes, "\"keep quotes\"  and  spaces");
    assert!(matches!(
        services.decisions[0].action,
        checkpoint_decisions::DecisionAction::Suggest
    ));
    assert_eq!(
        services.decisions[1].notes,
        "Approved displayed checkpoint content from console."
    );
}

#[test]
fn max_checkpoint_status_survives_quit_eof_and_confirmation_eof() {
    for ending in ["quit\n", "", "approve one\n"] {
        let root = fixture();
        let mut services = FakeServices {
            decision_code: 2,
            ..Default::default()
        };
        let (status, _, _) = session(
            root.path(),
            &format!("approve one\ny\n{ending}"),
            &mut services,
        );
        assert_eq!(status, 2);
        assert_eq!(services.decisions.len(), 1);
    }
}

#[test]
fn conflict_warns_only_stderr_once_no_refresh_retry_and_survives_eof() {
    let root = fixture();
    let mut services = FakeServices {
        conflict: true,
        ..Default::default()
    };
    let (status, out, err) = session(root.path(), "approve one\ny\n", &mut services);
    assert_eq!(status, 1);
    assert!(!out.contains("[WARN]"));
    assert_eq!(
        err,
        "[WARN] Stale checkpoint revision. Reinspect and confirm explicitly; no retry was made.\n"
    );
    assert_eq!(services.calls, ["initial", "display:one"]);
    assert_eq!(services.decisions.len(), 1);
}

#[test]
fn post_display_edit_conflicts_against_the_original_snapshot_with_no_input_guard() {
    struct EditingInput<'a> {
        root: &'a Path,
        text: Cursor<Vec<u8>>,
        edited: bool,
    }
    impl io::Read for EditingInput<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.text.read(bytes)
        }
    }
    impl BufRead for EditingInput<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.text.position() == b"approve one\n".len() as u64 && !self.edited {
                let _guard = crate::storage::WorkspaceGuard::acquire(
                    self.root,
                    std::time::Duration::from_millis(50),
                )
                .unwrap();
                std::fs::write(
                    self.root.join("checkpoints/one.md"),
                    "Changed after display\n",
                )
                .unwrap();
                self.edited = true;
            }
            self.text.fill_buf()
        }
        fn consume(&mut self, amount: usize) {
            self.text.consume(amount);
        }
    }
    let root = fixture();
    let mut before = Vec::new();
    let revision = checkpoint_decisions::display_io(root.path(), "one", &mut before).unwrap();
    let mut input = EditingInput {
        root: root.path(),
        text: Cursor::new(b"approve one\ny\n".to_vec()),
        edited: false,
    };
    let mut services = FakeServices::default();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        run_with(root.path(), &mut input, &mut out, &mut err, &mut services).unwrap(),
        1
    );
    assert!(input.edited);
    assert_eq!(services.decisions.len(), 1);
    assert_eq!(
        services.decisions[0].expected_revision.as_deref(),
        Some(revision.as_str())
    );
    assert!(out.windows(before.len()).any(|window| window == before));
    assert!(
        !String::from_utf8(out)
            .unwrap()
            .contains("Changed after display")
    );
    assert!(
        String::from_utf8(err)
            .unwrap()
            .contains("no retry was made")
    );
}

#[test]
fn stale_and_corrupt_bindings_are_displayed_before_declining_not_silently_approved() {
    let root = fixture();
    let original = std::fs::read(root.path().join("checkpoints/one.md")).unwrap();
    let (approved, _) = crate::checkpoints::prepare(
        "one",
        Some(&original),
        "approved",
        "",
        &"1".repeat(32),
        "test",
        "cli",
        None,
    )
    .unwrap();
    let approved = String::from_utf8(approved).unwrap();
    for (content, effective) in [
        (
            approved.replace("# Plan\n", "# Changed\n"),
            "approval_stale",
        ),
        (
            approved.replace("Status: approved", "Status: pending"),
            "decision_invalid",
        ),
    ] {
        std::fs::write(root.path().join("checkpoints/one.md"), content).unwrap();
        let mut services = FakeServices::default();
        let (_, out, _) = session(root.path(), "approve one\n\nquit\n", &mut services);
        assert!(
            out.contains(&format!("Effective status: {effective};")),
            "{out}"
        );
        assert!(services.decisions.is_empty());
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn nested_review_uses_borrowed_io_multiline_then_returns_to_console() {
    let root = fixture();
    let mut services = FakeServices {
        decision_code: 2,
        ..Default::default()
    };
    let (status, out, err) = session(
        root.path(),
        "review one ignored\ns\n\"literal\"\nsecond line\n.\nstatus\nquit\n",
        &mut services,
    );
    assert_eq!(status, 2);
    assert!(err.is_empty());
    assert!(out.contains("Finish with a single '.' on its own line."));
    assert!(out.contains("coven> dashboard\n\n"));
    assert_eq!(services.calls, ["initial", "review:one", "status"]);
    assert_eq!(services.decisions[0].notes, "\"literal\"\nsecond line");
    assert_eq!(services.decisions[0].source, "review");
}

#[test]
fn nested_review_empty_changes_and_public_borrowed_seam_need_no_global_stdin() {
    let root = fixture();
    let mut services = FakeServices::default();
    let (status, out, _) = session(root.path(), "review\ns\n.\nhelp\nq\n", &mut services);
    assert_eq!(status, 0);
    assert!(services.decisions.is_empty());
    assert!(out.contains("No change request saved"));
    let mut out = Vec::new();
    assert_eq!(
        checkpoint_decisions::review_io(
            root.path(),
            &["one".into()],
            "all",
            "",
            &mut Cursor::new("n\n"),
            &mut out
        )
        .unwrap(),
        0
    );
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("Review checkpoint: one")
    );
}

#[test]
fn cached_shortcuts_are_not_service_authority_and_errors_propagate() {
    let root = fixture();
    let mut services = FakeServices {
        ordinary_error: true,
        ..Default::default()
    };
    // A newly added agent is not recognized as a shortcut, but explicit send
    // reaches the service. A removed cached agent still must be revalidated.
    std::fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["b"]}"#,
    )
    .unwrap();
    let error = run_with(
        root.path(),
        &mut Cursor::new("b hi\na stale roster\n"),
        &mut Vec::new(),
        &mut Vec::new(),
        &mut services,
    )
    .unwrap_err();
    assert_eq!(error.code(), "unknown_agent");
    assert_eq!(services.calls, ["initial", "send:a:stale roster"]);
    let mut services = FakeServices {
        ordinary_error: true,
        ..Default::default()
    };
    assert!(
        run_with(
            root.path(),
            &mut Cursor::new("send b hi\n"),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut services
        )
        .is_err()
    );
    assert_eq!(services.calls, ["initial", "send:b:hi"]);
}

#[test]
fn invalid_checkpoint_preflight_fails_before_confirmation_without_mutation() {
    let root = fixture();
    std::fs::write(root.path().join("checkpoints/one.md"), [0xff]).unwrap();
    for input in ["approve ../bad\ny\n", "suggest one changes\ny\n"] {
        let mut services = FakeServices::default();
        let mut out = Vec::new();
        assert!(
            run_with(
                root.path(),
                &mut Cursor::new(input),
                &mut out,
                &mut Vec::new(),
                &mut services
            )
            .is_err()
        );
        assert!(!String::from_utf8(out).unwrap().contains("[y/N]"));
        assert!(services.decisions.is_empty());
        assert!(!root.path().join("state").exists());
    }
}

#[test]
fn line_profile_is_bounded_and_accepts_exact_limit_crlf() {
    let exact = "x".repeat(1_048_576);
    assert_eq!(
        read_line(&mut Cursor::new(format!("{exact}\r\n"))).unwrap(),
        Some(exact.clone())
    );
    assert_eq!(
        read_line(&mut Cursor::new(exact.clone())).unwrap(),
        Some(exact.clone())
    );
    assert_eq!(
        read_line(&mut Cursor::new(format!("{exact}x")))
            .unwrap_err()
            .code(),
        "unsupported_console_input_profile"
    );
    assert_eq!(
        read_line(&mut Cursor::new([0xff, b'\n']))
            .unwrap_err()
            .code(),
        "invalid_console_utf8"
    );
}

#[test]
fn prompt_is_flushed_before_read() {
    use std::{cell::Cell, rc::Rc};
    struct Output {
        flushed: Rc<Cell<bool>>,
    }
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.flushed.set(false);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushed.set(true);
            Ok(())
        }
    }
    struct Input {
        flushed: Rc<Cell<bool>>,
        text: Cursor<Vec<u8>>,
    }
    impl io::Read for Input {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.text.read(bytes)
        }
    }
    impl BufRead for Input {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            assert!(self.flushed.get());
            self.text.fill_buf()
        }
        fn consume(&mut self, amount: usize) {
            self.text.consume(amount);
        }
    }
    let root = fixture();
    let flushed = Rc::new(Cell::new(false));
    let mut input = Input {
        flushed: flushed.clone(),
        text: Cursor::new(b"approve one\nno\nreview one\nn\nquit\n".to_vec()),
    };
    let mut out = Output { flushed };
    run_with(
        root.path(),
        &mut input,
        &mut out,
        &mut Vec::new(),
        &mut FakeServices::default(),
    )
    .unwrap();
}
