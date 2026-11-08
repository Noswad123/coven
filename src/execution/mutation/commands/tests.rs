use super::*;

fn options() -> WriteOptions<'static> {
    WriteOptions {
        expected_execution_revision: DISABLED,
        expected_tasks_revision: None,
        request_id: "enable",
        json: true,
    }
}

#[test]
fn selectors_use_arity_not_workspace_markers() {
    let one = [PathBuf::from("raw-task")];
    assert_eq!(selector(&one).unwrap(), (Path::new("."), "raw-task"));
    let two = [
        PathBuf::from("not-a-workspace"),
        PathBuf::from("raw-revision"),
    ];
    assert_eq!(
        selector(&two).unwrap(),
        (Path::new("not-a-workspace"), "raw-revision")
    );
    assert_eq!(
        selector(&[]).unwrap_err().code(),
        "invalid_execution_selector_arguments"
    );
    assert!(selector(&[PathBuf::new(), PathBuf::new(), PathBuf::new()]).is_err());
}

#[test]
fn selected_documents_fail_before_workspace_or_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("document.json");
    fs::write(&path, b"{\"schema_version\":1,\"schema_version\":1}").unwrap();
    let bad_root = dir.path().join("absent");
    let items = [bad_root.clone(), PathBuf::from("task")];
    let expected = document(&path, false).unwrap_err().code();
    assert_eq!(
        prepare_arguments(&items, PreparationInput::Intent(&path), options())
            .err()
            .unwrap()
            .code(),
        expected
    );
    let expected = document(&path, true).unwrap_err().code();
    assert_eq!(
        propose_arguments(&bad_root, &path, options())
            .err()
            .unwrap()
            .code(),
        expected
    );
    assert!(!bad_root.exists());
}

#[test]
fn bounded_regular_documents_are_strictly_validated() {
    let dir = tempfile::tempdir().unwrap();
    assert!(document(dir.path(), false).is_err());
    let path = dir.path().join("intent.json");
    fs::write(&path, vec![b' '; 1_048_577]).unwrap();
    assert_eq!(
        document(&path, false).unwrap_err().code(),
        "handoff_size_limit"
    );
    fs::write(&path, br#"{"schema_version":1,"kind":"task-intent","task_id":"task","owner":"worker","acceptance_criteria":[{"id":"one","text":"ship"}],"unknown":true}"#).unwrap();
    assert_eq!(
        document(&path, false).unwrap_err().code(),
        "invalid_execution_fields"
    );
}

#[cfg(unix)]
#[test]
fn selected_document_alias_is_explicitly_resolved_and_binds_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("intent.json");
    fs::write(&path, br#"{"schema_version":1,"kind":"task-intent","task_id":"task","owner":"worker","acceptance_criteria":[{"id":"one","text":"ship"}]}"#).unwrap();
    let alias = dir.path().join("alias.json");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert_eq!(
        document(&path, false).unwrap().canonical().unwrap(),
        document(&alias, false).unwrap().canonical().unwrap()
    );
}

#[test]
fn raw_text_profile_bounds_precede_clones_without_artifacts_or_semantic_policy() {
    let dir = tempfile::tempdir().unwrap();
    let oversized = "x".repeat(values::MAX_STRING + 1);
    let boundary = &oversized[..values::MAX_STRING];
    assert_eq!(text(&bounded_text(boundary).unwrap()).unwrap(), boundary);
    assert_eq!(
        text(&bounded_text("  unchanged\n").unwrap()).unwrap(),
        "  unchanged\n"
    );
    let task = [dir.path().to_owned(), PathBuf::from(&oversized)];
    let normal = [dir.path().to_owned(), PathBuf::from("task")];
    let mut outputs = vec![
        enroll_arguments(&task, false, options()),
        prepare_arguments(
            &task,
            PreparationInput::GraphRevision("revision"),
            options(),
        ),
        prepare_arguments(
            &normal,
            PreparationInput::GraphRevision(&oversized),
            options(),
        ),
        decide_arguments(&task, "approve", "reason", "digest", options()),
    ];
    for (decision, reason, digest) in [
        (oversized.as_str(), "reason", "digest"),
        ("approve", oversized.as_str(), "digest"),
        ("approve", "reason", oversized.as_str()),
    ] {
        outputs.push(decide_arguments(
            &normal,
            decision,
            reason,
            digest,
            options(),
        ));
    }
    for output in outputs {
        assert_eq!(
            output.err().unwrap().code(),
            "unsupported_json_string_profile"
        );
    }
    assert!(!dir.path().join("state").exists());
}

#[test]
fn upgrade_binding_precedes_missing_workspace() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        upgrade_arguments(&dir.path().join("absent"), options())
            .err()
            .unwrap()
            .code(),
        "invalid_execution_revision"
    );
}
