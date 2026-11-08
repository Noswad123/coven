use super::*;
use std::fs;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    fs::create_dir(root.path().join("logs")).unwrap();
    root
}

fn capture(root: &Path, kind: HistoryKind, raw: bool) -> String {
    let result = history_arguments(root, kind, raw).unwrap();
    assert!(result.warnings.is_empty());
    assert_eq!(result.exit_code, 0);
    result.stdout
}

#[test]
fn independent_python_field_defaults_truthiness_repr_and_raw_newlines() {
    let oracle = std::process::Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/workspace_commands/history/oracle.py"
        ))
        .output()
        .unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let root = fixture();
    let mut count = 0;
    for line in oracle
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row = values::parse(line, values::Profile::LegacySnapshot).unwrap();
        let row = row.as_object().unwrap();
        if let Some(record) = row.get("record") {
            for (kind, key) in [
                (HistoryKind::Events, "events"),
                (HistoryKind::Messages, "messages"),
            ] {
                let mut bytes = record.compact().unwrap();
                bytes.push(b'\n');
                fs::write(root.path().join(kind.log().relative()), bytes).unwrap();
                assert_eq!(
                    capture(root.path(), kind, false),
                    row.get(key).unwrap().as_str().unwrap(),
                    "case {count}, {kind:?}"
                );
            }
        } else {
            let path = root.path().join("logs/events.jsonl");
            match row.get("raw").unwrap() {
                Value::Null => {
                    if path.exists() {
                        fs::remove_file(&path).unwrap();
                    }
                }
                Value::String(text) => fs::write(&path, text).unwrap(),
                _ => panic!("invalid oracle raw case"),
            }
            assert_eq!(
                capture(root.path(), HistoryKind::Events, true),
                row.get("text").unwrap().as_str().unwrap()
            );
        }
        count += 1;
    }
    assert_eq!(count, 232);
}

#[test]
fn missing_empty_blank_and_literal_controls_are_exact_and_read_only() {
    let root = fixture();
    for kind in [HistoryKind::Events, HistoryKind::Messages] {
        let path = root.path().join(kind.log().relative());
        assert_eq!(capture(root.path(), kind, false), kind.empty());
        assert_eq!(
            capture(root.path(), kind, true),
            format!("{}\n", kind.empty())
        );
        assert!(!path.exists());
        for bytes in [b"".as_slice(), b"\n\n", "\u{1c}\u{a0}\n".as_bytes()] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(capture(root.path(), kind, false), kind.empty());
            assert_eq!(capture(root.path(), kind, true).as_bytes(), bytes);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("dashboard.md").exists());
}

#[test]
fn whole_tail_is_refused_without_output_but_raw_accepts_arbitrary_utf8() {
    let root = fixture();
    for kind in [HistoryKind::Events, HistoryKind::Messages] {
        for tail in ["{}", "{\n", "[]\n", "{\"v\":NaN}\n", "{\"v\":1e999}\n"] {
            let bytes = format!("{{\"type\":\"valid first record\"}}\n{tail}");
            let path = root.path().join(kind.log().relative());
            fs::write(&path, &bytes).unwrap();
            assert!(history_arguments(root.path(), kind, false).is_err());
            assert_eq!(capture(root.path(), kind, true), bytes);
            assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        }
        fs::write(root.path().join(kind.log().relative()), b"{}\n\xff").unwrap();
        assert!(history_arguments(root.path(), kind, true).is_err());
        assert!(history_arguments(root.path(), kind, false).is_err());
    }
}

#[test]
fn raw_bypasses_physical_line_and_json_depth_profiles() {
    let root = fixture();
    let path = root.path().join("logs/events.jsonl");
    let lines = vec![b'\n'; logs::MAX_LINES + 1];
    fs::write(&path, &lines).unwrap();
    assert_eq!(
        history_arguments(root.path(), HistoryKind::Events, false)
            .err()
            .unwrap()
            .code(),
        "unsupported_event_log_line_profile"
    );
    assert_eq!(
        capture(root.path(), HistoryKind::Events, true).as_bytes(),
        lines
    );
    let deep = format!(
        "{{\"v\":{}null{}}}\n",
        "[".repeat(values::MAX_DEPTH),
        "]".repeat(values::MAX_DEPTH)
    );
    fs::write(&path, &deep).unwrap();
    assert_eq!(
        history_arguments(root.path(), HistoryKind::Events, false)
            .err()
            .unwrap()
            .code(),
        "unsupported_json_depth_profile"
    );
    assert_eq!(capture(root.path(), HistoryKind::Events, true), deep);
}

#[test]
fn explicit_workspace_does_not_require_marker_operand_selection() {
    let root = fixture();
    // Resolve the explicit root itself; never fall back to the process cwd.
    let child = root.path().join("not-a-workspace");
    fs::create_dir(&child).unwrap();
    assert!(history_arguments(&child, HistoryKind::Events, false).is_err());
    let equivalent = root.path().join("logs/..");
    assert_eq!(
        capture(&equivalent, HistoryKind::Events, false),
        "No events yet\n"
    );
}

#[cfg(unix)]
#[test]
fn canonical_root_must_be_utf8() {
    use std::os::unix::ffi::OsStringExt;
    let parent = tempfile::tempdir().unwrap();
    let path = parent
        .path()
        .join(std::ffi::OsString::from_vec(b"invalid-\xff".to_vec()));
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("coven.json"),
        br#"{"name":"test","agents":["operator"]}"#,
    )
    .unwrap();
    for raw in [false, true] {
        assert_eq!(
            history_arguments(&path, HistoryKind::Events, raw)
                .err()
                .unwrap()
                .code(),
            "non_unicode_path_unsupported"
        );
    }
}

#[test]
fn preflight_precedes_selected_log_but_does_not_parse_unselected_log() {
    let root = fixture();
    fs::write(root.path().join("logs/messages.jsonl"), b"malformed tail").unwrap();
    assert_eq!(
        capture(root.path(), HistoryKind::Events, false),
        "No events yet\n"
    );
    assert_eq!(
        capture(root.path(), HistoryKind::Events, true),
        "No events yet\n\n"
    );
    fs::create_dir(root.path().join("state")).unwrap();
    fs::write(root.path().join("state/agents.json"), b"null").unwrap();
    let expected = workspace::load(root.path()).err().unwrap();
    for raw in [false, true] {
        assert_eq!(
            history_arguments(root.path(), HistoryKind::Messages, raw)
                .err()
                .unwrap(),
            expected
        );
    }
    assert!(!root.path().join("state/.workspace.lock").exists());
}

#[test]
fn history_does_not_acquire_an_existing_held_workspace_lock() {
    let root = fixture();
    let guard =
        crate::storage::WorkspaceGuard::acquire(root.path(), std::time::Duration::ZERO).unwrap();
    for kind in [HistoryKind::Events, HistoryKind::Messages] {
        assert_eq!(capture(root.path(), kind, false), kind.empty());
        assert_eq!(
            capture(root.path(), kind, true),
            format!("{}\n", kind.empty())
        );
    }
    guard.validate_for(root.path()).unwrap();
    assert!(!root.path().join("dashboard.md").exists());
}

#[test]
fn pretty_aggregate_expansion_and_raw_complete_output_are_bounded() {
    let root = fixture();
    let path = root.path().join("logs/events.jsonl");
    // Each selected field contains a nested string, so repr escapes each NUL.
    let text = "\0".repeat(250_000);
    let value = Value::Array(vec![Value::String(text)]);
    let record = Value::object(
        ["ts", "type", "agent", "checkpoint", "task_id"]
            .into_iter()
            .map(|key| (key.to_owned(), value.clone()))
            .collect(),
    )
    .unwrap();
    let mut bytes = record.compact().unwrap();
    bytes.push(b'\n');
    assert!(bytes.len() < logs::MAX_LOG);
    // Use a pure multi-record capture to exercise output admission independently
    // of the stricter whole-file input bound.
    assert_eq!(
        pretty(&vec![record; 8], HistoryKind::Events)
            .unwrap_err()
            .code(),
        "unsupported_history_output_profile"
    );
    fs::write(&path, vec![b'x'; values::MAX_OUTPUT]).unwrap();
    assert_eq!(
        capture(root.path(), HistoryKind::Events, true).len(),
        values::MAX_OUTPUT
    );
    fs::write(&path, vec![b'x'; values::MAX_OUTPUT + 1]).unwrap();
    assert_eq!(
        history_arguments(root.path(), HistoryKind::Events, true)
            .err()
            .unwrap()
            .code(),
        "unsupported_history_output_profile"
    );
}

#[cfg(unix)]
#[test]
fn unsafe_managed_paths_are_refused_for_both_modes() {
    use std::os::unix::fs::symlink;
    for raw in [false, true] {
        let root = fixture();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), b"outside\n").unwrap();
        symlink(outside.path(), root.path().join("logs/events.jsonl")).unwrap();
        assert!(history_arguments(root.path(), HistoryKind::Events, raw).is_err());
        assert_eq!(fs::read(outside.path()).unwrap(), b"outside\n");
        // Full workspace preflight also rejects unsafe unrelated managed files.
        assert!(history_arguments(root.path(), HistoryKind::Messages, raw).is_err());
    }
}
