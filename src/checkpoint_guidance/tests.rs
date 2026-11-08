use super::*;
use crate::values::{self, Profile};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("coven.json"),
        br#"{"name":"test","agents":["a"]}"#,
    )
    .unwrap();
    for name in ["checkpoints", "logs", "state"] {
        fs::create_dir(dir.path().join(name)).unwrap();
    }
    dir
}

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    fs::write(root.join(name), bytes).unwrap();
}

fn chosen(root: &Path) -> Option<String> {
    select(root, &mut |_| {}).unwrap()
}

fn parse(text: &str) -> Value {
    values::parse(text.as_bytes(), Profile::LegacySnapshot).unwrap()
}

fn approved(id: &str) -> String {
    // Test-only fixture preparation; production never invokes Python/providers.
    let output = std::process::Command::new("python3")
        .args(["-B", "-c", concat!(
            "import sys; from runner import checkpoints\n",
            "data=b'# Plan\\nStatus: pending\\n## User decision\\n- [ ] Approved\\n'\n",
            "data,_=checkpoints.prepare(sys.argv[1],data,'approved','',decision_id='1'*32,decided_at='test',source='cli')\n",
            "sys.stdout.buffer.write(data)"
        ), id])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn structural_request_predicate_matches_finite_python_repr() {
    for raw in [
        r#""changes.requested""#,
        r#"[false,{"x":["request"]}]"#,
        r#"{"requested":null}"#,
    ] {
        assert!(contains_request(&parse(raw)), "{raw}");
    }
    for raw in [
        "null",
        "true",
        "false",
        "20240101",
        "1.23e20",
        r#""REQUEST""#,
        r#"["req","uest"]"#,
        r#"{"req":"uest"}"#,
    ] {
        assert!(!contains_request(&parse(raw)), "{raw}");
    }
}

#[test]
fn stable_ties_messages_first_duplicates_and_last_request() {
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        "logs/messages.jsonl",
        concat!(
            "{\"checkpoint\":\"late\",\"type\":\"request\",\"ts\":\"2025-01-01\"}\n",
            "{\"checkpoint\":\"message\",\"type\":{\"requested\":null}}\n",
            "{\"checkpoint\":\"message\",\"type\":[\"changes.requested\"]}\n"
        ),
    );
    write(
        root,
        "logs/events.jsonl",
        concat!(
            "{\"checkpoint\":\"event\",\"type\":\"request\",\"ts\":\"invalid\"}\n",
            "{\"checkpoint\":\"event-late\",\"type\":\"request\",\"ts\":\"2025-01-01T01:00:00+01:00\"}\n"
        ),
    );
    let mut seen = Vec::new();
    let selected = select(root, &mut |id| seen.push(id.to_owned())).unwrap();
    assert_eq!(seen, ["message", "message", "event", "late", "event-late"]);
    assert_eq!(selected.as_deref(), Some("event-late"));
}

#[test]
fn approved_duplicates_filtered_but_all_requests_inspected() {
    let dir = fixture();
    let root = dir.path();
    write(root, "checkpoints/yes.md", approved("yes"));
    write(
        root,
        "logs/messages.jsonl",
        concat!(
            "{\"checkpoint\":\"missing\",\"type\":\"request\"}\n",
            "{\"checkpoint\":\"yes\",\"type\":\"request\"}\n",
            "{\"checkpoint\":\"yes\",\"type\":\"request\"}\n"
        ),
    );
    let mut seen = Vec::new();
    assert_eq!(
        select(root, &mut |id| seen.push(id.to_owned()))
            .unwrap()
            .as_deref(),
        Some("missing")
    );
    assert_eq!(seen, ["missing", "yes", "yes"]);
}

#[test]
fn effective_statuses_and_exact_fallback_set() {
    let dir = fixture();
    let root = dir.path();
    let original = approved("test");
    for (content, effective, fallback) in [
        (Some(original.clone()), "approved", false),
        (
            Some(original.replace("# Plan\n", "# Changed\n")),
            "approval_stale",
            true,
        ),
        (
            Some(original.replace("Status: approved", "Status: pending")),
            "decision_invalid",
            true,
        ),
        (Some("Status: approved\n".into()), "approved_unbound", true),
        (Some("Status: pending\n".into()), "pending", false),
        (
            Some("Status: changes_requested\n".into()),
            "changes_requested",
            false,
        ),
        (None, "missing", false),
    ] {
        let path = root.join("checkpoints/test.md");
        if let Some(content) = content {
            write(root, "checkpoints/test.md", content);
        } else if path.exists() {
            fs::remove_file(path).unwrap();
        }
        assert_eq!(status(root, "test").unwrap(), effective);
        write(root, "logs/messages.jsonl", b"");
        assert_eq!(chosen(root).is_some(), fallback, "{effective}");
        write(
            root,
            "logs/messages.jsonl",
            b"{\"checkpoint\":\"test\",\"type\":\"request\"}\n",
        );
        assert_eq!(
            chosen(root).is_some(),
            effective != "approved",
            "{effective}"
        );
    }
}

#[test]
fn fallback_last_case_sensitive_lower_extension_only_and_no_pending_discovery() {
    let dir = fixture();
    let root = dir.path();
    for name in ["A.md", "Z.md", "a.md", "z.MD"] {
        write(root, &format!("checkpoints/{name}"), "Status: approved\n");
    }
    write(root, "checkpoints/zz.md", "Status: pending\n");
    assert_eq!(chosen(root).as_deref(), Some("a"));
    let out = next_arguments(root).unwrap();
    assert!(out.stdout.contains("checkpoint `a`"));
    // Preflight still checks uppercase-extension content, unlike fallback.
    write(root, "checkpoints/z.MD", b"\xff");
    assert!(next_arguments(root).is_err());
}

#[test]
fn whole_messages_then_references_then_events_and_full_tail_errors() {
    let dir = fixture();
    let root = dir.path();
    write(root, "logs/events.jsonl", b"broken\n");
    for reference in ["null", "false", "0", "[]", "{}"] {
        write(
            root,
            "logs/messages.jsonl",
            format!("{{\"checkpoint\":{reference},\"type\":\"note\"}}\n"),
        );
        assert_eq!(
            next_arguments(root).err().unwrap().code(),
            "invalid_checkpoint_history_reference"
        );
    }
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"../bad\"}\n",
    );
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "invalid_checkpoint_identifier"
    );
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":42}\nbroken\n",
    );
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "invalid_jsonl_record"
    );
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"missing\",\"type\":\"request\"}\n",
    );
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "invalid_jsonl_record"
    );
    write(root, "logs/events.jsonl", b"{\"checkpoint\":false}\n");
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "invalid_checkpoint_history_reference"
    );
}

#[test]
fn nonrequest_reference_content_checked_before_events_parse_and_missing_type_ignored() {
    let dir = fixture();
    let root = dir.path();
    write(root, "logs/messages.jsonl", b"{\"checkpoint\":\"note\"}\n");
    write(root, "logs/events.jsonl", b"broken\n");
    write(root, "checkpoints/note.md", b"\xff");
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "invalid_utf8"
    );
    write(root, "checkpoints/note.md", vec![b'x'; MAX_TEXT + 1]);
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "unsupported_checkpoint_history_profile"
    );
    write(root, "checkpoints/note.md", b"Status: pending\n");
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "invalid_jsonl_record"
    );
    write(root, "logs/events.jsonl", b"");
    assert_eq!(chosen(root), None);
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"\",\"type\":\"note\"}\n",
    );
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "invalid_checkpoint_identifier"
    );
}

#[test]
fn sorted_inspection_errors_do_not_short_circuit_or_inspect_only_selected() {
    let dir = fixture();
    let root = dir.path();
    write(
        root,
        "logs/messages.jsonl",
        concat!(
            "{\"checkpoint\":\"later\",\"type\":\"request\",\"ts\":\"2025-01-01\"}\n",
            "{\"checkpoint\":\"earlier\",\"type\":\"request\"}\n"
        ),
    );
    // Reference validation permits malformed decision grammar; inspection does not.
    write(
        root,
        "checkpoints/earlier.md",
        "Status: pending\nStatus: approved\n",
    );
    write(root, "checkpoints/later.md", "Status: pending\n");
    let mut seen = Vec::new();
    let error = select(root, &mut |id| seen.push(id.to_owned())).unwrap_err();
    assert_eq!(error.code(), "invalid_checkpoint_ambiguous_presentation");
    assert_eq!(seen, ["earlier"]);
    write(root, "checkpoints/earlier.md", "Status: pending\n");
    write(
        root,
        "checkpoints/later.md",
        "Status: pending\nStatus: approved\n",
    );
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "invalid_checkpoint_ambiguous_presentation"
    );
}

#[test]
fn exact_literal_output_lf_workspace_omission_and_bounds() {
    assert_eq!(
        render("/root", true, None).unwrap(),
        "Next action: inspect status/dashboard\nSuggested: coven status\n"
    );
    assert_eq!(
        render("/root", false, None).unwrap(),
        "Next action: inspect status/dashboard\nSuggested: coven status /root\n"
    );
    assert_eq!(
        render("/root", true, Some("plan")).unwrap(),
        concat!(
            "Next action: approve checkpoint `plan`\n",
            "Reason: one or more agents are blocked waiting for user approval.\n",
            "Review file: /root/checkpoints/plan.md\nSuggested: coven approve plan\n"
        )
    );
    let root = "/literal \u{1f98a}\n\t$(never-execute)";
    assert!(
        render(root, false, Some("x"))
            .unwrap()
            .ends_with(&format!("Suggested: coven approve {root} x\n"))
    );
    assert!(
        render("/", false, Some("x"))
            .unwrap()
            .contains("Review file: /checkpoints/x.md\n")
    );
    let overhead = render("", false, None).unwrap().len();
    let root = "r".repeat(MAX_OUTPUT - overhead);
    assert_eq!(render(&root, false, None).unwrap().len(), MAX_OUTPUT);
    assert_eq!(
        render(&(root + "r"), false, None).unwrap_err().code(),
        "unsupported_checkpoint_guidance_output_profile"
    );
    assert!(render(&"r".repeat(MAX_OUTPUT / 2), false, Some("x")).is_err());
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)> {
    let mut result = BTreeMap::new();
    fn visit(
        root: &Path,
        path: &Path,
        result: &mut BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)>,
    ) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().into(),
                    (
                        fs::read(&path).unwrap(),
                        fs::metadata(path).unwrap().modified().unwrap(),
                    ),
                );
            }
        }
    }
    visit(root, root, &mut result);
    result
}

#[test]
fn held_lock_no_acquisition_zero_effects_and_canonical_aliases() {
    let dir = fixture();
    let root = dir.path();
    let guard = crate::storage::WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
    write(root, "dashboard.md", b"authored");
    let before = snapshot(root);
    let out = next_arguments(&root.join("checkpoints/..")).unwrap();
    assert_eq!(
        out.stdout,
        render(root.to_str().unwrap(), false, None).unwrap()
    );
    assert!(out.warnings.is_empty());
    assert_eq!(out.exit_code, 0);
    assert_eq!(snapshot(root), before);
    drop(guard);
    fs::remove_file(root.join("state/.workspace.lock")).unwrap();
    next_arguments(root).unwrap();
    assert!(!root.join("state/.workspace.lock").exists());
}

#[test]
fn pending_import_active_bad_projection_and_runtime_control_are_permitted() {
    for active in [false, true] {
        let dir = fixture();
        let root = dir.path();
        if active {
            let guard = crate::storage::WorkspaceGuard::acquire(root, Duration::ZERO).unwrap();
            let activated =
                crate::ledger::import_tasks(root, &crate::ledger::snapshot_revision(None), &guard)
                    .unwrap();
            assert!(activated.view().tasks().as_array().unwrap().is_empty());
            drop(guard);
            // Even an unsafe projection is ignored once ledger authority exists.
            if root.join("state/tasks.json").exists() {
                fs::remove_file(root.join("state/tasks.json")).unwrap();
            }
            fs::create_dir(root.join("state/tasks.json")).unwrap();
        } else {
            fs::create_dir(root.join("state/task-ledger")).unwrap();
            write(root, "state/task-ledger/orphan", b"pending");
        }
        let location = platform::workspace_location(root).unwrap();
        let control = serde_json::json!({
            "version": 1, "action": "runtime_launch",
            "workspace_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "epoch_id": "cccccccccccccccccccccccccccccccc",
            "location": { "root": location.root, "device": location.device, "inode": location.inode },
            "head": null, "claims": {}
        });
        write(
            root,
            "state/runtime-launch-control.json",
            serde_json::to_vec(&control).unwrap(),
        );
        assert_eq!(next_arguments(root).unwrap().exit_code, 0);
        write(root, "state/runtime-launch-control.json", b"null");
        assert!(next_arguments(root).is_err());
    }
}

#[test]
fn second_scan_and_actual_inspection_reads_enforce_caps_again() {
    let dir = fixture();
    let root = dir.path();
    write(root, "checkpoints/yes.md", approved("yes"));
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"yes\",\"type\":\"request\"}\n",
    );
    workspace::load(root).unwrap();
    let error = select(root, &mut |_| {
        for i in 0..4096 {
            write(root, &format!("checkpoints/unknown.{i}"), b"");
        }
    })
    .unwrap_err();
    assert_eq!(error.code(), "unsupported_checkpoint_scan_profile");

    let dir = fixture();
    let root = dir.path();
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"missing\",\"type\":\"request\"}\n",
    );
    let error = select(root, &mut |_| {
        write(root, "checkpoints/missing.md", vec![b'x'; MAX_TEXT + 1]);
    })
    .unwrap_err();
    assert_eq!(error.code(), "unsupported_checkpoint_input_profile");
}

#[cfg(unix)]
#[test]
fn reads_preserve_inode_mode_mtime_and_accept_root_symlink() {
    use std::os::unix::fs::MetadataExt;
    let dir = fixture();
    let root = dir.path();
    let alias_dir = tempfile::tempdir().unwrap();
    let alias = alias_dir.path().join("alias");
    std::os::unix::fs::symlink(root, &alias).unwrap();
    let path = root.join("coven.json");
    let before = fs::metadata(&path).unwrap();
    next_arguments(&alias).unwrap();
    let after = fs::metadata(path).unwrap();
    assert_eq!(
        (
            before.ino(),
            before.mode(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (after.ino(), after.mode(), after.mtime(), after.mtime_nsec())
    );
    assert!(!root.join("state/.workspace.lock").exists());
}

#[test]
fn checkpoint_size_and_scan_caps_include_unknown_entries() {
    let dir = fixture();
    let root = dir.path();
    write(root, "checkpoints/x.md", vec![b'x'; MAX_TEXT]);
    next_arguments(root).unwrap();
    write(root, "checkpoints/x.md", vec![b'x'; MAX_TEXT + 1]);
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "unsupported_checkpoint_workspace_input_profile"
    );
    fs::remove_file(root.join("checkpoints/x.md")).unwrap();
    for i in 0..4096 {
        write(root, &format!("checkpoints/unknown.{i}"), b"");
    }
    next_arguments(root).unwrap();
    write(root, "checkpoints/one-too-many", b"");
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "unsupported_checkpoint_scan_profile"
    );
    assert_eq!(
        select(root, &mut |_| {}).unwrap_err().code(),
        "unsupported_checkpoint_scan_profile"
    );
}

#[cfg(unix)]
#[test]
fn managed_symlinks_refused_incidental_markdown_links_inert() {
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "checkpoints/plan.md",
        "Status: pending\n[incidental](../../outside)\n",
    );
    write(
        root,
        "logs/messages.jsonl",
        b"{\"checkpoint\":\"plan\",\"type\":\"request\"}\n",
    );
    next_arguments(root).unwrap();
    fs::remove_file(root.join("checkpoints/plan.md")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("checkpoints/plan.md")).unwrap();
    assert_eq!(
        next_arguments(root).err().unwrap().code(),
        "unsafe_archive_path"
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}
