use super::*;
use crate::{
    logs::{MAX_LINES, MAX_LOG, validate_log},
    values::{self, Profile},
};
#[cfg(unix)]
use std::{fs::File, io::Write};

fn parse(raw: &[u8]) -> Value {
    values::parse(raw, Profile::LegacySnapshot).unwrap()
}

#[test]
fn jsonl_physical_lines_legacy_objects_and_python_whitespace() {
    for bytes in [
        b"".as_slice(),
        b"\n\n",
        b"\r\n",
        "\u{1c}\u{a0}\n".as_bytes(),
        b"{\"a\":0,\"a\":1}\n",
        b"{}\r\n\r{}\n",
        "{\"text\":\"literal \u{2028} and \u{1f98a}\"}\n".as_bytes(),
    ] {
        assert!(validate_log(bytes).is_ok(), "{bytes:?}");
    }
    for bytes in [b" ".as_slice(), b"\r", b"{}", b"{}\n ", b"{}\n\r"] {
        assert_eq!(
            validate_log(bytes).unwrap_err().code(),
            "event_log_unterminated_line",
            "{bytes:?}"
        );
    }
    assert_eq!(
        validate_log(b"\xff\n").unwrap_err().code(),
        "invalid_jsonl_utf8"
    );
    for bytes in [
        b"{}\r{}\n".as_slice(),
        b"[]\n",
        b"null\n",
        b"true\n",
        b"1\n",
        b"\"text\"\n",
        b"{\n",
        b"{\"a\":\"\x01\"}\n",
        b"{\"a\":1e}\n",
        b"{\"a\":NaN}\n",
        b"{\"a\":Infinity}\n",
        b"{\"a\":-Infinity}\n",
        b"{\"a\":1e999}\n",
        b"{\"a\":NaN,\"a\":0}\n",
        b"{\"a\":1e999,\"a\":0}\n",
        b"\xef\xbb\xbf{}\n",
        "\u{a0}{}\n".as_bytes(),
        b"{}\x0b\n",
        b"{\"a\":\"\\ud800\"}\n",
    ] {
        assert_eq!(
            validate_log(bytes).unwrap_err().code(),
            "invalid_jsonl_record",
            "{bytes:?}"
        );
    }
    let duplicate = parse(br#"{"a":0,"b":2,"a":1}"#);
    assert_eq!(duplicate.compact().unwrap(), br#"{"a":1,"b":2}"#);
}

#[test]
fn exact_log_byte_line_and_value_limits() {
    // Blank physical lines consume the limits too; a trailing split sentinel
    // does not count as an extra physical line.
    let mut bytes = vec![b' '; MAX_LOG];
    bytes[MAX_LOG - 1] = b'\n';
    assert_eq!(validate_log(&bytes).unwrap(), 1);
    bytes.push(b'\n');
    assert_eq!(
        validate_log(&bytes).unwrap_err().code(),
        "unsupported_event_log_size_profile"
    );
    assert_eq!(validate_log(&vec![b'\n'; MAX_LINES]).unwrap(), MAX_LINES);
    assert_eq!(
        validate_log(&vec![b'\n'; MAX_LINES + 1])
            .unwrap_err()
            .code(),
        "unsupported_event_log_line_profile"
    );
    let valid = format!("{{\"s\":\"{}\"}}\n", "s".repeat(values::MAX_STRING));
    validate_log(valid.as_bytes()).unwrap();
    let invalid = format!("{{\"s\":\"{}\"}}\n", "s".repeat(values::MAX_STRING + 1));
    assert_eq!(
        validate_log(invalid.as_bytes()).unwrap_err().code(),
        "unsupported_json_string_profile"
    );
    let full_array = format!("[{}]", vec!["0"; values::MAX_ITEMS].join(","));
    for (record, code) in [
        (
            format!(
                "{{\"v\":{}{}}}\n",
                "[".repeat(values::MAX_DEPTH),
                "]".repeat(values::MAX_DEPTH)
            ),
            "unsupported_json_depth_profile",
        ),
        (
            format!("{{\"v\":{}}}\n", "1".repeat(values::MAX_INTEGER_DIGITS + 1)),
            "unsupported_integer_profile",
        ),
        (
            format!("{{\"v\":0.{}}}\n", "0".repeat(values::MAX_NUMBER)),
            "unsupported_number_token_profile",
        ),
        (
            format!("{{\"v\":[{}0]}}\n", "0,".repeat(values::MAX_ITEMS)),
            "unsupported_json_array_profile",
        ),
        (
            format!(
                "{{{}}}\n",
                (0..=values::MAX_MEMBERS)
                    .map(|i| format!("\"k{i}\":0"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            "unsupported_json_object_profile",
        ),
        (
            format!("{{\"v\":[{0},{0},{0},{0}]}}\n", full_array),
            "unsupported_json_node_profile",
        ),
    ] {
        assert_eq!(validate_log(record.as_bytes()).unwrap_err().code(), code);
    }
}

#[test]
fn markdown_labels_python_printability_html_then_punctuation() {
    let mut out = Markdown(String::new());
    out.label(" A\u{a0}\u{200d}\u{e000}\u{378}\u{10ffff}\n<&>\\`*_{}[]()#+.!|~-\u{1f98a}")
        .unwrap();
    assert_eq!(
        out.0,
        " A\\\\u00a0\\\\u200d\\\\ue000\\\\u0378\\\\u10ffff\\\\u000a&lt;&amp;&gt;\\\\\\`\\*\\_\\{\\}\\[\\]\\(\\)\\#\\+\\.\\!\\|\\~\\-\u{1f98a}"
    );
}

#[test]
fn output_is_incrementally_bounded_including_label_expansion() {
    let mut out = Markdown("x".repeat(MAX_DASHBOARD - 1));
    out.push("x").unwrap();
    assert_eq!(out.0.len(), MAX_DASHBOARD);
    assert_eq!(
        out.push("x").unwrap_err().code(),
        "unsupported_dashboard_output_profile"
    );
    assert_eq!(out.0.len(), MAX_DASHBOARD);
    let mut out = Markdown("x".repeat(MAX_DASHBOARD - 2));
    assert!(out.label("&").is_err());
    assert_eq!(out.0.len(), MAX_DASHBOARD - 2);
}

#[cfg(unix)]
mod io_tests {
    use super::*;
    use crate::platform;
    use std::{cell::Cell, fs, process::Command, time::Duration};

    fn fixture() -> (tempfile::TempDir, WorkspaceGuard) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("coven.json"), br#"{"name":"Test & coven","agents":["a","b"],"multiplexer":"herdr","display_names":{"a":"A <&>_*|"}}"#).unwrap();
        fs::create_dir(dir.path().join("logs")).unwrap();
        let guard = WorkspaceGuard::acquire(dir.path(), Duration::ZERO).unwrap();
        (dir, guard)
    }

    fn append(root: &Path, guard: &WorkspaceGuard, io: &impl AppendIo) -> Result<()> {
        append_with_metadata(root, "task.assigned", &parse(br#"{"task_id":"t","agent":"a","mutation_id":"m","before_revision":"before","after_revision":"after","extension":"literal \u0000"}"#), guard,
            "12345678-1234-4123-8123-123456789abc", "2026-11-01T00:00:00+00:00", io)
    }

    #[test]
    fn dashboard_input_preflight_checks_workspace_then_goal_without_log_content() {
        let (dir, _guard) = fixture();
        let root = dir.path();
        fs::write(root.join("logs/events.jsonl"), b"corrupt").unwrap();
        fs::write(root.join("logs/messages.jsonl"), b"corrupt").unwrap();
        validate_dashboard_inputs(root).unwrap();
        assert!(!root.join("dashboard.md").exists());
        fs::write(root.join("goal.md"), vec![b'x'; MAX_TEXT + 1]).unwrap();
        assert_eq!(
            validate_dashboard_inputs(root).unwrap_err().code(),
            "unsupported_dashboard_input_profile"
        );
        fs::write(root.join("coven.json"), b"invalid").unwrap();
        let expected = workspace::load(root).err().unwrap().code();
        assert_eq!(
            validate_dashboard_inputs(root).unwrap_err().code(),
            expected
        );
    }

    #[test]
    fn checkpoint_history_parses_whole_selected_log_then_checks_messages_before_events() {
        let (dir, _guard) = fixture();
        let root = dir.path();
        fs::write(
            root.join("logs/messages.jsonl"),
            b"{\"checkpoint\":null}\n{\n",
        )
        .unwrap();
        fs::write(root.join("logs/events.jsonl"), b"\xff\n").unwrap();
        assert_eq!(
            validate_checkpoint_history(root).unwrap_err().code(),
            "invalid_jsonl_record"
        );
        fs::write(root.join("logs/messages.jsonl"), b"{\"checkpoint\":null}\n").unwrap();
        assert_eq!(
            validate_checkpoint_history(root).unwrap_err().code(),
            "invalid_checkpoint_history_reference"
        );
        fs::write(root.join("logs/messages.jsonl"), b"{}\n").unwrap();
        assert_eq!(
            validate_checkpoint_history(root).unwrap_err().code(),
            "invalid_jsonl_utf8"
        );
    }

    #[test]
    fn complete_event_order_metadata_literal_utf8_and_both_snapshot_logs() {
        let (dir, guard) = fixture();
        let root = dir.path();
        validate_snapshot_logs(root).unwrap();
        for relative in ["logs/events.jsonl", "logs/messages.jsonl"] {
            for (bytes, code) in [
                (b"{\n".as_slice(), "invalid_jsonl_record"),
                (b"\xff\n".as_slice(), "invalid_jsonl_utf8"),
            ] {
                fs::write(root.join(relative), bytes).unwrap();
                assert_eq!(validate_snapshot_logs(root).unwrap_err().code(), code);
                fs::remove_file(root.join(relative)).unwrap();
            }
            fs::write(root.join(relative), vec![b'\n'; MAX_LOG + 1]).unwrap();
            assert_eq!(
                validate_snapshot_logs(root).unwrap_err().code(),
                "unsupported_event_log_size_profile"
            );
            fs::remove_file(root.join(relative)).unwrap();
        }
        fs::write(root.join("logs/events.jsonl"), b"{\"old\":0,\"old\":1}\r\n").unwrap();
        append(root, &guard, &SystemAppendIo).unwrap();
        assert_eq!(fs::read(root.join("logs/events.jsonl")).unwrap(), b"{\"old\":0,\"old\":1}\r\n{\"event_id\":\"12345678-1234-4123-8123-123456789abc\",\"ts\":\"2026-11-01T00:00:00+00:00\",\"type\":\"task.assigned\",\"task_id\":\"t\",\"agent\":\"a\",\"mutation_id\":\"m\",\"before_revision\":\"before\",\"after_revision\":\"after\",\"extension\":\"literal \\u0000\"}\n");
        fs::write(root.join("logs/messages.jsonl"), b" \n[]\n").unwrap();
        assert!(validate_snapshot_logs(root).is_err());
        // Event delivery does not validate message content after ledger commit.
        let payload = parse("{\"mutation_id\":\"m\",\"epoch_id\":\"e\",\"sequence\":1,\"frame_digest\":\"d\",\"text\":\"\u{1f98a}\"}".as_bytes());
        append_with_metadata(
            root,
            "task.created",
            &payload,
            &guard,
            "id",
            "ts",
            &SystemAppendIo,
        )
        .unwrap();
        let log = fs::read(root.join("logs/events.jsonl")).unwrap();
        assert!(log.ends_with("{\"event_id\":\"id\",\"ts\":\"ts\",\"type\":\"task.created\",\"mutation_id\":\"m\",\"epoch_id\":\"e\",\"sequence\":1,\"frame_digest\":\"d\",\"text\":\"\u{1f98a}\"}\n".as_bytes()));
        // Python dict unpacking also allows an explicit metadata replacement.
        append_with_metadata(
            root,
            "original",
            &parse(br#"{"type":"override","z":0}"#),
            &guard,
            "id",
            "ts",
            &SystemAppendIo,
        )
        .unwrap();
        assert!(
            fs::read(root.join("logs/events.jsonl"))
                .unwrap()
                .ends_with(b"{\"event_id\":\"id\",\"ts\":\"ts\",\"type\":\"override\",\"z\":0}\n")
        );
    }

    struct NeverOpen;
    impl AppendIo for NeverOpen {
        fn open(&self, _: &Path) -> Result<File> {
            panic!("invalid append reached mutating open")
        }
    }

    #[test]
    fn projected_bytes_and_physical_lines_rejected_before_append_open() {
        let (dir, guard) = fixture();
        let root = dir.path();
        let path = root.join("logs/events.jsonl");
        fs::write(&path, vec![b'\n'; MAX_LINES - 1]).unwrap();
        append(root, &guard, &SystemAppendIo).unwrap();
        assert_eq!(validate_log(&fs::read(&path).unwrap()).unwrap(), MAX_LINES);
        fs::write(&path, vec![b'\n'; MAX_LINES]).unwrap();
        assert_eq!(
            append(root, &guard, &NeverOpen).unwrap_err().code(),
            "unsupported_event_log_line_profile"
        );
        let payload = parse(b"{}");
        let encoded = b"{\"event_id\":\"id\",\"ts\":\"ts\",\"type\":\"t\"}\n";
        let mut bytes = vec![b' '; MAX_LOG - encoded.len()];
        *bytes.last_mut().unwrap() = b'\n';
        fs::write(&path, &bytes).unwrap();
        append_with_metadata(root, "t", &payload, &guard, "id", "ts", &SystemAppendIo).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_LOG as u64);
        assert_eq!(
            append(root, &guard, &NeverOpen).unwrap_err().code(),
            "unsupported_event_log_size_profile"
        );
        bytes.push(b'\n');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            append_with_metadata(root, "t", &payload, &guard, "id", "ts", &NeverOpen)
                .unwrap_err()
                .code(),
            "unsupported_event_log_size_profile"
        );
        for malformed in [b" ".as_slice(), b"[]\n", b"{\"x\":1e999}\n", b"\xff\n"] {
            fs::write(&path, malformed).unwrap();
            assert!(append(root, &guard, &NeverOpen).is_err());
            assert_eq!(fs::read(&path).unwrap(), malformed);
        }
    }

    #[test]
    fn actual_encoded_line_must_fit_legacy_profile_before_open() {
        let (dir, guard) = fixture();
        // The payload alone has the maximum accepted number of members, but
        // event metadata would make the real encoded object unreadable.
        let payload = Value::object(
            (0..values::MAX_MEMBERS)
                .map(|i| (format!("k{i}"), Value::Null))
                .collect(),
        )
        .unwrap();
        assert!(
            append_with_metadata(dir.path(), "t", &payload, &guard, "id", "ts", &NeverOpen)
                .is_err()
        );
        assert!(!dir.path().join("logs/events.jsonl").exists());
        let payload = Value::object(
            (0..9)
                .map(|i| (format!("k{i}"), Value::String("x".repeat(MAX_TEXT))))
                .collect(),
        )
        .unwrap();
        assert_eq!(
            append_with_metadata(dir.path(), "t", &payload, &guard, "id", "ts", &NeverOpen)
                .unwrap_err()
                .code(),
            "unsupported_event_log_size_profile"
        );
        assert!(!dir.path().join("logs/events.jsonl").exists());
    }

    struct Fault {
        partial: bool,
        synced: Cell<bool>,
    }
    impl AppendIo for Fault {
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            if self.partial {
                file.write_all(&bytes[..7])?;
                Err(std::io::Error::other("injected partial write"))
            } else {
                file.write_all(bytes)
            }
        }
        fn sync(&self, _: &File) -> std::io::Result<()> {
            self.synced.set(true);
            Err(std::io::Error::other("injected fsync"))
        }
    }

    #[test]
    fn partial_append_and_fsync_failures_preserve_evidence_never_rollback() {
        for partial in [true, false] {
            let (dir, guard) = fixture();
            let fault = Fault {
                partial,
                synced: Cell::new(false),
            };
            assert_eq!(
                append(dir.path(), &guard, &fault).unwrap_err().code(),
                "event_append_may_be_partial"
            );
            assert_eq!(fault.synced.get(), !partial);
            let path = dir.path().join("logs/events.jsonl");
            let bytes = fs::read(&path).unwrap();
            if partial {
                assert_eq!(bytes, b"{\"event");
                assert!(append(dir.path(), &guard, &NeverOpen).is_err());
            } else {
                validate_log(&bytes).unwrap();
                // A complete but unconfirmed record is not deduplicated.
                append(dir.path(), &guard, &SystemAppendIo).unwrap();
                assert_eq!(fs::read(&path).unwrap(), bytes.repeat(2));
            }
        }
    }

    fn oracle(root: &Path) -> Vec<u8> {
        let output = Command::new("python3").args(["-B", "-c", "import sys; from pathlib import Path; from runner import coven; sys.stdout.buffer.write(coven.render_dashboard(Path(sys.argv[1])).encode('utf-8'))"])
            .arg(root).current_dir(env!("CARGO_MANIFEST_DIR")).env("PYTHONDONTWRITEBYTECODE", "1").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn dashboard_python_golden_bytes_snapshot_ledger_leads_labels_goal_checkpoints() {
        let (dir, guard) = fixture();
        let root = dir.path();
        let mut team = parse(&fs::read(root.join("coven.json")).unwrap());
        team.set_field(
            "display_names",
            Value::object(vec![(
                "a".into(),
                Value::String("A <&>_*| \u{a0}\u{200d}\u{e000}\u{378}\u{10ffff}\u{1f98a}".into()),
            )])
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join("coven.json"), team.compact().unwrap()).unwrap();
        fs::write(
            root.join("state/tasks.json"),
            br#"[{"id":"t","title":"Raw | *title*\nnext","status":"raw|status","owner":null}]"#,
        )
        .unwrap();
        fs::write(root.join("state/agents.json"), br#"[{"id":"a","status":"idle","role":"raw | role"},{"id":"b","status":"ready","role":"builder"}]"#).unwrap();
        fs::write(
            root.join("coven.toml"),
            b"[runtime]\nmultiplexer = ' tmux '\n",
        )
        .unwrap();
        fs::create_dir(root.join("checkpoints")).unwrap();
        fs::write(
            root.join("checkpoints/plan-approval.md"),
            b"# Plan\r\nStatus: approved\r\n## User decision\r\n- [x] Approved\r\n",
        )
        .unwrap();
        fs::write(
            root.join("checkpoints/pre-delivery-review.md"),
            b"# Review\rStatus: pending\r",
        )
        .unwrap();
        for lead in [
            b"null".as_slice(),
            br#"{"agent":"a"}"#,
            br#"{"agent":"b","set_at":"","until":"raw | until"}"#,
        ] {
            fs::write(root.join("state/lead.json"), lead).unwrap();
            for goal in [
                b" \r\n# Goal\r\n\rMixed\r\nnewlines\r ".as_slice(),
                b"# Goal \r\n ",
                b"# Goalkeeper\rtext",
                b"  no heading\r\n",
            ] {
                fs::write(root.join("goal.md"), goal).unwrap();
                refresh_dashboard(root, &guard).unwrap();
                assert_eq!(fs::read(root.join("dashboard.md")).unwrap(), oracle(root));
            }
        }
        fs::remove_file(root.join("goal.md")).unwrap();
        fs::remove_file(root.join("state/lead.json")).unwrap();
        refresh_dashboard(root, &guard).unwrap();
        assert_eq!(fs::read(root.join("dashboard.md")).unwrap(), oracle(root));
        let token = crate::ledger::snapshot_revision(Some(
            &fs::read(root.join("state/tasks.json")).unwrap(),
        ));
        let _ = crate::ledger::import_tasks(root, &token, &guard).unwrap();
        // Dashboard uses ledger authority even when its projection is corrupt.
        fs::write(root.join("state/tasks.json"), b"not a projection").unwrap();
        let before = fs::read(root.join("state/tasks.json")).unwrap();
        refresh_dashboard(root, &guard).unwrap();
        let rendered = fs::read(root.join("dashboard.md")).unwrap();
        assert_eq!(rendered, oracle(root));
        assert!(
            String::from_utf8(rendered)
                .unwrap()
                .contains("Raw | *title*\nnext")
        );
        assert_eq!(fs::read(root.join("state/tasks.json")).unwrap(), before);
    }

    #[test]
    fn checkpoint_effective_stale_invalid_and_raw_newlines_match_python() {
        let (dir, guard) = fixture();
        let root = dir.path();
        fs::create_dir(root.join("checkpoints")).unwrap();
        // Python is only an independent test oracle and fixture generator.
        let approved = Command::new("python3").args(["-B", "-c", concat!(
            "import sys; from runner import checkpoints\n",
            "data=b'# Plan\\r\\nStatus: pending\\r\\n## User decision\\r\\n- [ ] Approved\\r\\n'\n",
            "data,_=checkpoints.prepare('plan-approval',data,'approved','',decision_id='1'*32,decided_at='test',source='cli')\n",
            "sys.stdout.buffer.write(data)"
        )]).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        assert!(
            approved.status.success(),
            "{}",
            String::from_utf8_lossy(&approved.stderr)
        );
        let original = String::from_utf8(approved.stdout).unwrap();
        for (content, status) in [
            (original.clone(), "approved"),
            (
                original.replace("# Plan\r\n", "# Changed plan\r\n"),
                "approval_stale",
            ),
            (
                original.replace("Status: approved", "Status: pending"),
                "decision_invalid",
            ),
            (original.replace("\r\n", "\n"), "approval_stale"),
        ] {
            fs::write(
                root.join("checkpoints/plan-approval.md"),
                content.as_bytes(),
            )
            .unwrap();
            let captured = capture(root).unwrap();
            assert_eq!(captured.statuses[0], status);
            refresh_dashboard(root, &guard).unwrap();
            assert_eq!(fs::read(root.join("dashboard.md")).unwrap(), oracle(root));
        }
    }

    #[test]
    fn failures_preserve_dashboard_and_event_failure_does_not_block_refresh() {
        let (dir, guard) = fixture();
        let root = dir.path();
        let target = root.join("dashboard.md");
        fs::write(&target, b"old dashboard").unwrap();
        fs::write(root.join("goal.md"), vec![b'x'; MAX_TEXT + 1]).unwrap();
        assert_eq!(
            goal(root).unwrap_err().code(),
            "unsupported_dashboard_input_profile"
        );
        for bytes in [vec![0xff], vec![b'x'; MAX_TEXT + 1]] {
            fs::write(root.join("goal.md"), bytes).unwrap();
            assert!(refresh_dashboard(root, &guard).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"old dashboard");
        }
        fs::remove_file(root.join("goal.md")).unwrap();
        fs::create_dir(root.join("checkpoints")).unwrap();
        for relative in [
            "checkpoints/plan-approval.md",
            "checkpoints/pre-delivery-review.md",
        ] {
            fs::write(root.join(relative), vec![b'x'; MAX_TEXT + 1]).unwrap();
            assert_eq!(
                optional_bytes(
                    root,
                    relative,
                    MAX_TEXT,
                    "unsupported_dashboard_input_profile"
                )
                .unwrap_err()
                .code(),
                "unsupported_dashboard_input_profile"
            );
            fs::remove_file(root.join(relative)).unwrap();
        }
        fs::write(
            root.join("checkpoints/plan-approval.md"),
            b"Status: pending\nStatus: approved\n",
        )
        .unwrap();
        assert!(refresh_dashboard(root, &guard).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old dashboard");
        fs::remove_file(root.join("checkpoints/plan-approval.md")).unwrap();
        fs::remove_dir(root.join("logs")).unwrap();
        let fault = Fault {
            partial: true,
            synced: Cell::new(false),
        };
        assert_eq!(
            append(root, &guard, &fault).unwrap_err().code(),
            "event_append_may_be_partial"
        );
        assert_eq!(
            fs::read(root.join("logs/events.jsonl")).unwrap(),
            b"{\"event"
        );
        // Dashboard remains independently publishable after event failure.
        refresh_dashboard(root, &guard).unwrap();
        assert_ne!(fs::read(&target).unwrap(), b"old dashboard");
    }

    #[test]
    fn missing_logs_created_only_by_admitted_guarded_append_for_both_authorities() {
        use std::os::unix::fs::PermissionsExt;
        let unlocked = tempfile::tempdir().unwrap();
        fs::write(
            unlocked.path().join("coven.json"),
            br#"{"name":"Test","agents":["a"]}"#,
        )
        .unwrap();
        validate_snapshot_logs(unlocked.path()).unwrap();
        assert!(!unlocked.path().join("logs").exists());
        assert!(!unlocked.path().join("state").exists());
        struct OpenProbe<'a>(&'a WorkspaceGuard);
        impl AppendIo for OpenProbe<'_> {
            fn open(&self, path: &Path) -> Result<File> {
                self.0.validate_for(self.0.root())?;
                assert!(
                    matches!(WorkspaceGuard::existing(self.0.root(), Duration::ZERO), Err(error) if error.code() == "workspace_lock_busy")
                );
                assert!(path.parent().unwrap().is_dir());
                platform::open_append(path)
            }
        }
        fn names(path: &Path) -> Vec<std::ffi::OsString> {
            let mut names = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>();
            names.sort();
            names
        }
        for active in [false, true] {
            let (dir, guard) = fixture();
            let root = dir.path();
            if active {
                let revision = crate::ledger::snapshot_revision(None);
                let _ = crate::ledger::import_tasks(root, &revision, &guard).unwrap();
            }
            fs::remove_dir(root.join("logs")).unwrap();
            validate_snapshot_logs(root).unwrap();
            workspace::load(root).unwrap();
            refresh_dashboard(root, &guard).unwrap();
            assert!(!root.join("logs").exists());
            let payload = Value::object(
                (0..values::MAX_MEMBERS)
                    .map(|i| (format!("k{i}"), Value::Null))
                    .collect(),
            )
            .unwrap();
            assert!(
                append_with_metadata(root, "t", &payload, &guard, "id", "ts", &NeverOpen).is_err()
            );
            assert!(!root.join("logs").exists());
            let mut expected = names(root);
            expected.push("logs".into());
            expected.sort();
            let state_names = names(&root.join("state"));
            let reference = tempfile::tempdir().unwrap();
            fs::create_dir(reference.path().join("mkdir-default")).unwrap();
            append(root, &guard, &OpenProbe(&guard)).unwrap();
            assert_eq!(names(root), expected);
            assert_eq!(names(&root.join("state")), state_names);
            assert_eq!(
                names(&root.join("logs")),
                vec![std::ffi::OsString::from("events.jsonl")]
            );
            assert_eq!(
                validate_log(&fs::read(root.join("logs/events.jsonl")).unwrap()).unwrap(),
                1
            );
            assert_eq!(
                fs::metadata(root.join("logs"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                fs::metadata(reference.path().join("mkdir-default"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            );
            guard.validate_for(root).unwrap();
        }
    }

    #[test]
    fn unsafe_logs_parent_is_never_followed_replaced_or_adopted() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        for active in [false, true] {
            for kind in ["file", "symlink", "dangling", "fifo"] {
                let (dir, guard) = fixture();
                let root = dir.path();
                if active {
                    let _ = crate::ledger::import_tasks(
                        root,
                        &crate::ledger::snapshot_revision(None),
                        &guard,
                    )
                    .unwrap();
                }
                fs::remove_dir(root.join("logs")).unwrap();
                let external = tempfile::tempdir().unwrap();
                let logs = root.join("logs");
                match kind {
                    "file" => fs::write(&logs, b"preserve").unwrap(),
                    "symlink" => symlink(external.path(), &logs).unwrap(),
                    "dangling" => symlink(external.path().join("absent"), &logs).unwrap(),
                    "fifo" => {
                        let path = std::ffi::CString::new(logs.as_os_str().as_bytes()).unwrap();
                        // Test-only namespace fault: mkfifo consumes the valid
                        // NUL-terminated path and creates no open descriptor.
                        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                    }
                    _ => unreachable!(),
                }
                let before = fs::symlink_metadata(&logs).unwrap().file_type();
                assert!(validate_snapshot_logs(root).is_err());
                assert!(append(root, &guard, &NeverOpen).is_err());
                assert_eq!(fs::symlink_metadata(&logs).unwrap().file_type(), before);
                assert!(fs::read_dir(external.path()).unwrap().next().is_none());
                if kind == "file" {
                    assert_eq!(fs::read(&logs).unwrap(), b"preserve");
                }
            }
        }
    }

    #[test]
    fn wrong_guard_and_unsafe_managed_paths_never_mutate() {
        use std::os::unix::fs::symlink;
        let (dir, guard) = fixture();
        let (other, _) = fixture();
        fs::remove_dir(other.path().join("logs")).unwrap();
        fs::write(other.path().join("dashboard.md"), b"old").unwrap();
        assert!(append(other.path(), &guard, &NeverOpen).is_err());
        assert!(!other.path().join("logs").exists());
        assert!(refresh_dashboard(other.path(), &guard).is_err());
        assert_eq!(fs::read(other.path().join("dashboard.md")).unwrap(), b"old");
        let external = tempfile::NamedTempFile::new().unwrap();
        fs::write(external.path(), b"external").unwrap();
        symlink(external.path(), dir.path().join("logs/events.jsonl")).unwrap();
        assert!(append(dir.path(), &guard, &NeverOpen).is_err());
        assert_eq!(fs::read(external.path()).unwrap(), b"external");
    }

    #[test]
    fn dashboard_render_overflow_preserves_old_file() {
        let (dir, guard) = fixture();
        // Capture can legitimately contain more than 32 MiB from ledger replay.
        // Exercise the pure renderer without constructing a giant ledger history.
        let mut captured = capture(dir.path()).unwrap();
        captured.state.tasks = Value::Array(
            (0..33)
                .map(|index| {
                    parse(
                        format!(
                            "{{\"id\":\"t{index}\",\"title\":\"{}\",\"status\":\"pending\"}}",
                            "x".repeat(MAX_TEXT)
                        )
                        .as_bytes(),
                    )
                })
                .collect(),
        );
        fs::write(dir.path().join("dashboard.md"), b"old").unwrap();
        assert_eq!(
            render(&captured).unwrap_err().code(),
            "unsupported_dashboard_output_profile"
        );
        assert_eq!(fs::read(dir.path().join("dashboard.md")).unwrap(), b"old");
        guard.validate_for(dir.path()).unwrap();
    }

    struct DashboardProbe {
        capture_seen: Cell<bool>,
        published: Cell<bool>,
        uncertain: bool,
        overflow: bool,
    }
    impl DashboardIo for DashboardProbe {
        fn capture(&self, root: &Path, guard: &WorkspaceGuard) -> Result<CapturedDashboard> {
            guard.validate_for(root)?;
            assert!(
                matches!(WorkspaceGuard::existing(root, Duration::ZERO), Err(error) if error.code() == "workspace_lock_busy")
            );
            self.capture_seen.set(true);
            // Change data after the earlier preflights: the secondary writer
            // must capture fresh state, not reuse a task caller's old snapshot.
            fs::write(
                root.join("state/tasks.json"),
                br#"[{"id":"fresh","status":"pending","title":"fresh capture"}]"#,
            )?;
            let mut captured = capture(root)?;
            if self.overflow {
                captured.goal = "x".repeat(MAX_DASHBOARD);
            }
            Ok(captured)
        }
        fn publish(&self, guard: &WorkspaceGuard, bytes: &[u8]) -> Result<Delivery> {
            assert!(self.capture_seen.get());
            assert!(
                matches!(WorkspaceGuard::existing(guard.root(), Duration::ZERO), Err(error) if error.code() == "workspace_lock_busy")
            );
            assert!(std::str::from_utf8(bytes)?.contains("fresh capture"));
            self.published.set(true);
            let delivery = storage::atomic_replace_bytes(guard, Path::new("dashboard.md"), bytes)?;
            assert_eq!(delivery, Delivery::Committed);
            Ok(if self.uncertain {
                Delivery::DurabilityUncertain
            } else {
                delivery
            })
        }
    }

    #[test]
    fn capture_render_publish_share_guard_and_uncertainty_never_rolls_back() {
        for (uncertain, overflow) in [(false, false), (true, false), (false, true)] {
            let (dir, guard) = fixture();
            fs::write(dir.path().join("dashboard.md"), b"old dashboard").unwrap();
            let io = DashboardProbe {
                capture_seen: Cell::new(false),
                published: Cell::new(false),
                uncertain,
                overflow,
            };
            let result = refresh_with(dir.path(), &guard, &io);
            assert!(io.capture_seen.get());
            assert_eq!(io.published.get(), !overflow);
            match (uncertain, overflow) {
                (_, true) => {
                    assert_eq!(
                        result.unwrap_err().code(),
                        "unsupported_dashboard_output_profile"
                    );
                    assert_eq!(
                        fs::read(dir.path().join("dashboard.md")).unwrap(),
                        b"old dashboard"
                    );
                }
                (true, false) => {
                    assert_eq!(
                        result.unwrap_err().code(),
                        "dashboard_synchronization_uncertain"
                    );
                    assert_ne!(
                        fs::read(dir.path().join("dashboard.md")).unwrap(),
                        b"old dashboard"
                    );
                }
                (false, false) => result.unwrap(),
            }
            guard.validate_for(dir.path()).unwrap();
        }
    }
}
