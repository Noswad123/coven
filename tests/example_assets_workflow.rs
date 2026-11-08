//! Native custom examples use only child-local environment and read-only fixtures.
#![cfg(unix)]
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(root: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_coven"));
    command
        .args(args)
        .env_remove("COVEN_HOME")
        .env_remove("COVEN_EXAMPLES_DIR")
        .env_remove("COVEN_COMMAND")
        .env_remove("COVEN_MULTIPLEXER");
    if let Some(root) = root {
        command.env("COVEN_EXAMPLES_DIR", root);
    }
    command.output().unwrap()
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn fixture(root: &Path) -> std::path::PathBuf {
    let example = root.join("custom");
    fs::create_dir_all(&example).unwrap();
    fs::write(
        example.join("example.json"),
        r#"{"id": "custom", "seed_message": [true, null, "ready"]}"#,
    )
    .unwrap();
    fs::write(
        example.join("goal.md"),
        "# Unicode \u{96ea}\r\nDo work\rEnd",
    )
    .unwrap();
    fs::write(example.join("prompt-context.md"), "Context \u{03bb}\r\n").unwrap();
    fs::write(
        example.join("tasks.json"),
        r#"[
        {"title":"Draft plan", "owner":"Clippy", "notes":"Keep me", "ignored":true},
        {"id":"review", "title":"", "owner":"Missing", "notes":null},
        {"id":null, "owner":null}
    ]"#,
    )
    .unwrap();
    example
}

#[test]
fn custom_init_matches_python_and_refresh_preserves_authored_state_and_source_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let examples = temp.path().join("examples");
    let example = fixture(&examples);
    let before_source = tree(&examples);
    let workspace = temp.path().join("workspace");
    let w = workspace.to_str().unwrap();
    let output = success(cli(
        Some(&examples),
        &[
            "init",
            w,
            "--example",
            "../Custom",
            "--agents",
            "Clippy,Reviewer",
        ],
    ));
    assert!(String::from_utf8_lossy(&output.stdout).contains("[True, None, 'ready']"));
    // Reviewed against runner/coven.py starter_tasks/example_goal_text at
    // e389a08669204d4e51a49ffd13cbe01b90b5d8ba. Expectations are not native-derived.
    let expected = serde_json::json!([
        {"id":"draft-plan", "title":"Draft plan", "status":"assigned", "owner":"clippy", "notes":"Keep me"},
        {"id":"review", "title":"review", "status":"pending", "owner":null, "notes":"Starter example task."},
        {"id":"task", "title":"task", "status":"pending", "owner":null, "notes":"Starter example task."}
    ]);
    let mut tasks: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("state/tasks.json")).unwrap()).unwrap();
    for task in tasks.as_array_mut().unwrap() {
        for field in ["created_at", "updated_at"] {
            let captured = task.as_object_mut().unwrap().remove(field).unwrap();
            let stamp = captured.as_str().unwrap();
            assert!(stamp.ends_with("+00:00"));
            assert_eq!(stamp.as_bytes().get(10), Some(&b'T'));
        }
    }
    assert_eq!(tasks, expected);
    assert_eq!(
        fs::read_to_string(workspace.join("goal.md")).unwrap(),
        "# Unicode \u{96ea}\nDo work\nEnd\n"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("spec.md")).unwrap(),
        "# Spec\n\nDescribe the desired behavior here.\n"
    );
    assert!(
        fs::read_to_string(workspace.join("prompts/clippy.md"))
            .unwrap()
            .contains("Context \u{03bb}\n")
    );
    let manifest = fs::read_to_string(workspace.join("tmux/coven.toml")).unwrap();
    assert!(manifest.contains("coven events"));
    assert!(!manifest.contains(env!("CARGO_MANIFEST_DIR")));
    assert_eq!(tree(&examples), before_source);
    fs::write(example.join("prompt-context.md"), "Updated context\r\n").unwrap();
    fs::write(
        example.join("tasks.json"),
        "not JSON: refresh must not read seed tasks",
    )
    .unwrap();
    fs::write(workspace.join("prompts/clippy.md"), b"authored role\xff").unwrap();
    fs::remove_file(workspace.join("prompts/reviewer.md")).unwrap();
    let before_workspace = tree(&workspace);
    let before_source = tree(&examples);
    success(cli(Some(&examples), &["refresh", w]));
    let after_workspace = tree(&workspace);
    for (path, bytes) in before_workspace {
        if path == "dashboard.md" || path == "tmux/coven.toml" || path.ends_with(".start.txt") {
            continue;
        }
        assert_eq!(after_workspace[&path], bytes, "{path}");
    }
    assert!(
        fs::read_to_string(workspace.join("prompts/reviewer.md"))
            .unwrap()
            .contains("Updated context\n")
    );
    assert_eq!(tree(&examples), before_source);
}

#[test]
fn bad_custom_seeds_and_missing_authoritative_roots_have_no_init_effects() {
    for raw in [
        r#"{}"#,
        r#"[7]"#,
        r#"[{"title": 7}]"#,
        r#"[{"id":"UPPER"}]"#,
        r#"[{"id":"x"},{"id":"x"}]"#,
        r#"[{"owner":false}]"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let examples = temp.path().join("examples");
        let example = fixture(&examples);
        fs::write(example.join("tasks.json"), raw).unwrap();
        let workspace = temp.path().join("workspace");
        let before = tree(&examples);
        let output = cli(
            Some(&examples),
            &["init", workspace.to_str().unwrap(), "--example", "custom"],
        );
        assert!(!output.status.success(), "{raw}");
        assert!(!workspace.exists(), "{raw}");
        assert_eq!(tree(&examples), before);
    }
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let missing = temp.path().join("missing");
    assert!(
        !cli(Some(&missing), &["init", workspace.to_str().unwrap()])
            .status
            .success()
    );
    assert!(!workspace.exists());
    assert!(
        !cli(
            None,
            &["init", workspace.to_str().unwrap(), "--example", ""]
        )
        .status
        .success()
    );
    assert!(!workspace.exists());
    success(cli(
        Some(&missing),
        &["init", workspace.to_str().unwrap(), "--blank"],
    ));
}

#[test]
fn blank_init_and_refresh_ignore_unused_invalid_asset_roots() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("not-a-directory");
    fs::write(&file, b"not assets").unwrap();
    let missing = temp.path().join("missing");
    let real_workspace = temp.path().join("real-example");
    success(cli(None, &["init", real_workspace.to_str().unwrap()]));
    let real_before = tree(&real_workspace);
    for key in ["COVEN_EXAMPLES_DIR", "COVEN_HOME"] {
        for (index, root) in [
            missing.as_path(),
            Path::new("relative-invalid-root"),
            file.as_path(),
        ]
        .into_iter()
        .enumerate()
        {
            let workspace = temp.path().join(format!("blank-{key}-{index}"));
            let invoke = |args: &[&str]| {
                Command::new(env!("CARGO_BIN_EXE_coven"))
                    .args(args)
                    .env_remove("COVEN_EXAMPLES_DIR")
                    .env_remove("COVEN_HOME")
                    .env_remove("COVEN_COMMAND")
                    .env_remove("COVEN_MULTIPLEXER")
                    .env(key, root)
                    .output()
                    .unwrap()
            };
            success(invoke(&["init", workspace.to_str().unwrap(), "--blank"]));
            success(invoke(&["refresh", workspace.to_str().unwrap()]));
            let team: serde_json::Value =
                serde_json::from_slice(&fs::read(workspace.join("coven.json")).unwrap()).unwrap();
            assert!(team["example"].is_null());
            let refused = temp.path().join(format!("refused-{key}-{index}"));
            assert!(
                !invoke(&["init", refused.to_str().unwrap()])
                    .status
                    .success()
            );
            assert!(!refused.exists());
            assert!(
                !invoke(&["refresh", real_workspace.to_str().unwrap()])
                    .status
                    .success()
            );
            assert_eq!(tree(&real_workspace), real_before);
        }
    }
}

#[test]
fn refresh_rejects_unsafe_context_before_lock_or_any_generated_effect() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let examples = temp.path().join("examples");
    let example = fixture(&examples);
    let workspace = temp.path().join("workspace");
    let w = workspace.to_str().unwrap();
    success(cli(Some(&examples), &["init", w, "--example", "custom"]));
    fs::remove_file(example.join("prompt-context.md")).unwrap();
    symlink("/dev/zero", example.join("prompt-context.md")).unwrap();
    let before = tree(&workspace);
    assert!(!cli(Some(&examples), &["refresh", w]).status.success());
    assert_eq!(tree(&workspace), before);
    for value in [serde_json::json!(false), serde_json::json!("")] {
        let path = workspace.join("coven.json");
        let mut team: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        team["example"] = value;
        fs::write(path, serde_json::to_vec(&team).unwrap()).unwrap();
        let before = tree(&workspace);
        assert!(!cli(None, &["refresh", w]).status.success());
        assert_eq!(tree(&workspace), before);
    }
}

#[test]
fn relocated_binary_uses_install_assets_and_embedded_portable_demo() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("coven");
    fs::copy(env!("CARGO_BIN_EXE_coven"), &executable).unwrap();
    let alias = temp.path().join("coven-alias");
    symlink(&executable, &alias).unwrap();
    let invoke = |workspace: &Path, selector: &str| {
        Command::new(&alias)
            .args(["init", workspace.to_str().unwrap(), "--example", selector])
            .env_remove("COVEN_HOME")
            .env_remove("COVEN_EXAMPLES_DIR")
            .env_remove("COVEN_COMMAND")
            .env_remove("COVEN_MULTIPLEXER")
            .current_dir(temp.path())
            .output()
            .unwrap()
    };
    let portable = temp.path().join("portable");
    success(invoke(&portable, "interactive-demo"));
    assert_eq!(
        fs::read(portable.join("goal.md")).unwrap(),
        include_bytes!("../examples/interactive-demo/goal.md")
    );
    assert!(
        fs::read_to_string(portable.join("tmux/coven.toml"))
            .unwrap()
            .contains("coven events")
    );
    let examples = temp.path().join("share/coven/examples");
    let example = fixture(&examples);
    let before = tree(&examples);
    for entry in fs::read_dir(&example).unwrap() {
        fs::set_permissions(entry.unwrap().path(), fs::Permissions::from_mode(0o444)).unwrap();
    }
    fs::set_permissions(&example, fs::Permissions::from_mode(0o555)).unwrap();
    let custom = temp.path().join("custom-workspace");
    success(invoke(&custom, "custom"));
    assert_eq!(tree(&examples), before);
    // An implicit installed demo must not replace the embedded authoritative seed.
    let demo = examples.join("interactive-demo");
    fs::create_dir(&demo).unwrap();
    fs::write(demo.join("example.json"), "{}").unwrap();
    fs::write(demo.join("goal.md"), "edited installed demo").unwrap();
    let still_embedded = temp.path().join("embedded");
    success(invoke(&still_embedded, "interactive-demo"));
    assert_eq!(
        fs::read(still_embedded.join("goal.md")).unwrap(),
        include_bytes!("../examples/interactive-demo/goal.md")
    );
    let explicit = temp.path().join("explicit");
    success(cli(Some(&examples), &["init", explicit.to_str().unwrap()]));
    assert_eq!(
        fs::read(explicit.join("goal.md")).unwrap(),
        b"edited installed demo\n"
    );
    fs::set_permissions(&example, fs::Permissions::from_mode(0o755)).unwrap();
}
