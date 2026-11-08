//! Real-binary installation smoke, independent of Python oracles and runners.
#![cfg(unix)]

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert!(kind.is_file(), "non-regular fixture: {:?}", entry.path());
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink(), "unexpected symlink: {path:?}");
            if kind.is_dir() {
                visit(root, &path, files);
            } else {
                assert!(kind.is_file());
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn installer(script: &Path, binary: &Path, prefix: &Path, examples: Option<&Path>) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .arg(script)
        .arg("--binary")
        .arg(binary)
        .arg("--prefix")
        .arg(prefix)
        .env_clear()
        // Host installation tools only. Product invocations below have no PATH.
        .env("PATH", "/usr/bin:/bin");
    if let Some(examples) = examples {
        command.arg("--examples").arg(examples);
    }
    command
}

fn install(script: &Path, binary: &Path, prefix: &Path, examples: Option<&Path>) -> Output {
    installer(script, binary, prefix, examples)
        .output()
        .unwrap()
}

fn cli(binary: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("HOME", cwd)
        .env("PATH", "")
        .output()
        .unwrap()
}

#[test]
fn relocated_native_install_runs_without_checkout_runner_python_or_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let distribution = root.join("prebuilt distribution");
    fs::create_dir_all(distribution.join("scripts")).unwrap();
    let script = distribution.join("scripts/install-native.sh");
    fs::copy(source.join("scripts/install-native.sh"), &script).unwrap();
    let prebuilt = distribution.join("coven");
    fs::copy(env!("CARGO_BIN_EXE_coven"), &prebuilt).unwrap();
    copy_tree(&source.join("examples"), &distribution.join("examples"));
    copy_tree(
        &source.join("tests/fixtures/native_install/examples"),
        &distribution.join("examples"),
    );
    let assets = tree(&distribution.join("examples"));
    let prefix = root.join("initial prefix");
    success(install(&script, &prebuilt, &prefix, None));
    let installed = tree(&prefix);
    let mut top_level: Vec<_> = fs::read_dir(&prefix)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    top_level.sort();
    assert_eq!(top_level, ["bin", "share"]);
    assert_eq!(installed.len(), assets.len() + 1);
    assert_eq!(
        installed[Path::new("bin/coven")],
        fs::read(&prebuilt).unwrap()
    );
    assert_eq!(tree(&prefix.join("share/coven/examples")), assets);
    for path in installed.keys() {
        assert!(path == Path::new("bin/coven") || path.starts_with("share/coven/examples"));
    }

    // This is not an upgrade mechanism: even our own populated prefix is refused.
    assert!(!install(&script, &prebuilt, &prefix, None).status.success());
    assert_eq!(tree(&prefix), installed);
    let relocated = root.join("relocated prefix");
    fs::rename(&prefix, &relocated).unwrap();
    fs::remove_dir_all(&distribution).unwrap();
    let binary = relocated.join("bin/coven");
    assert!(fs::metadata(&binary).unwrap().permissions().mode() & 0o111 != 0);
    success(cli(&binary, root, &["--help"]));

    for selector in ["blank", "interactive-demo", "native-custom"] {
        let workspace = root.join(format!("workspace-{selector}"));
        let w = workspace.to_str().unwrap();
        let mut args = vec!["init", w, "--agents", "operator,reviewer"];
        if selector == "blank" {
            args.push("--blank");
        } else {
            args.extend(["--example", selector]);
        }
        success(cli(&binary, root, &args));
        for path in ["coven.json", "state/tasks.json", "tmux/coven.toml"] {
            assert!(workspace.join(path).is_file(), "{selector}: {path}");
        }
        if selector == "native-custom" {
            assert_eq!(
                fs::read(workspace.join("goal.md")).unwrap(),
                include_bytes!("fixtures/native_install/examples/native-custom/goal.md")
            );
            assert!(
                fs::read_to_string(workspace.join("prompts/operator.md"))
                    .unwrap()
                    .contains("Installed native-only custom context.")
            );
        } else if selector == "interactive-demo" {
            assert_eq!(
                fs::read(workspace.join("goal.md")).unwrap(),
                include_bytes!("../examples/interactive-demo/goal.md")
            );
        } else {
            let tasks: serde_json::Value =
                serde_json::from_slice(&fs::read(workspace.join("state/tasks.json")).unwrap())
                    .unwrap();
            assert_eq!(tasks, serde_json::json!([]));
        }
        success(cli(&binary, root, &["refresh", w]));
        success(cli(
            &binary,
            root,
            &["task", "add", w, "Native installed task"],
        ));
        let listed = success(cli(&binary, root, &["task", "list", w, "--json"]));
        let tasks: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
        assert!(tasks.to_string().contains("Native installed task"));
        if selector == "native-custom" {
            assert!(tasks.to_string().contains("Installed native seed"));
        }
        let manifest = fs::read_to_string(workspace.join("tmux/coven.toml")).unwrap();
        assert!(manifest.contains("coven events"));
        assert!(!manifest.contains("COVEN_COMMAND"));
        assert!(!manifest.contains(relocated.to_str().unwrap()));
        for path in ["tmux/coven.toml", "prompts/operator.start.txt"] {
            let text = fs::read_to_string(workspace.join(path)).unwrap();
            for forbidden in [
                env!("CARGO_MANIFEST_DIR"),
                env!("CARGO_BIN_EXE_coven"),
                distribution.to_str().unwrap(),
                "runner/coven.py",
            ] {
                assert!(!text.contains(forbidden), "{selector}: {path}: {forbidden}");
            }
        }
        let before = tree(&workspace);
        let preview = success(cli(
            &binary,
            root,
            &[
                "session",
                "prompt",
                w,
                "--native-socket",
                "/missing/socket",
                "--expected-runtime",
                "6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0",
                "--session",
                "ses_install",
                "--request-id",
                "832492cf-947d-4ba4-8fa6-9c43a347a81b",
                "--prompt-file",
                "/missing/prompt",
                "--json",
            ],
        ));
        let report: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
        assert_eq!(report["authorization"], "none");
        assert!(report["prompt_sha256"].is_null());
        assert_eq!(tree(&workspace), before);
    }
    // An explicit COVEN_COMMAND may pin the installed executable, not a wrapper
    // or a build-tree path. Exercise quoting with the relocated prefix's spaces.
    let workspace = root.join("workspace-blank");
    let canonical = binary.canonicalize().unwrap();
    success(
        Command::new(&binary)
            .arg("refresh")
            .arg(&workspace)
            .current_dir(root)
            .env_clear()
            .env("PATH", "")
            .env("HOME", root)
            .env("COVEN_COMMAND", &canonical)
            .output()
            .unwrap(),
    );
    let manifest = fs::read_to_string(workspace.join("tmux/coven.toml")).unwrap();
    assert!(manifest.contains(&format!("'{}' events", canonical.display())));
    assert_eq!(tree(&relocated), installed);
}

#[test]
fn explicit_assets_are_supported_but_unsafe_inputs_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = source.join("scripts/install-native.sh");
    let binary = Path::new(env!("CARGO_BIN_EXE_coven"));
    let assets = source.join("tests/fixtures/native_install/examples");
    let prefix = root.join("new");
    success(install(&script, binary, &prefix, Some(&assets)));
    assert_eq!(tree(&prefix.join("share/coven/examples")), tree(&assets));
    let workspace = root.join("custom");
    success(cli(
        &prefix.join("bin/coven"),
        root,
        &[
            "init",
            workspace.to_str().unwrap(),
            "--example",
            "native-custom",
            "--agents",
            "operator",
        ],
    ));

    let occupied = root.join("legacy");
    fs::create_dir_all(occupied.join("bin")).unwrap();
    fs::create_dir(occupied.join("runner")).unwrap();
    fs::write(
        occupied.join("bin/coven"),
        "existing launcher, do not replace",
    )
    .unwrap();
    fs::write(
        occupied.join("runner/keep"),
        "existing runner, do not delete",
    )
    .unwrap();
    let before = tree(&occupied);
    assert!(
        !install(&script, binary, &occupied, Some(&assets))
            .status
            .success()
    );
    assert_eq!(tree(&occupied), before);
    let hidden = root.join("hidden");
    fs::create_dir(&hidden).unwrap();
    fs::write(hidden.join(".keep"), "unowned").unwrap();
    assert!(
        !install(&script, binary, &hidden, Some(&assets))
            .status
            .success()
    );
    assert_eq!(fs::read(hidden.join(".keep")).unwrap(), b"unowned");

    let linked = root.join("linked-binary");
    symlink(binary, &linked).unwrap();
    let not_executable = root.join("not-executable");
    fs::write(&not_executable, "not executable").unwrap();
    fs::set_permissions(&not_executable, fs::Permissions::from_mode(0o644)).unwrap();
    for invalid in [
        root.join("missing"),
        root.to_path_buf(),
        linked,
        not_executable,
    ] {
        let target = root.join("must-not-exist");
        assert!(
            !install(&script, &invalid, &target, Some(&assets))
                .status
                .success()
        );
        assert!(!target.exists());
    }
    let linked_prefix = root.join("linked-prefix");
    let empty = root.join("untouched");
    fs::create_dir(&empty).unwrap();
    symlink(&empty, &linked_prefix).unwrap();
    assert!(
        !install(&script, binary, &linked_prefix, Some(&assets))
            .status
            .success()
    );
    assert_eq!(fs::read_dir(empty).unwrap().count(), 0);
    for args in [
        vec![],
        vec!["--binary"],
        vec!["--binary", binary.to_str().unwrap()],
        vec!["--prefix", "/unused"],
        vec!["--unknown"],
    ] {
        assert!(
            !Command::new("/bin/sh")
                .arg(&script)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

fn fixture_paths() -> (PathBuf, PathBuf, PathBuf) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    (
        source.join("scripts/install-native.sh"),
        PathBuf::from(env!("CARGO_BIN_EXE_coven")),
        source.join("tests/fixtures/native_install/examples"),
    )
}

fn shim(root: &Path, name: &str, body: &str) -> PathBuf {
    let directory = root.join("host-tool-shims");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(name);
    fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    directory
}

// Include directories and inode/permission/timestamp metadata, not just bytes.
fn metadata_tree(root: &Path) -> BTreeMap<PathBuf, (u64, u32, u64, i64, i64)> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, (u64, u32, u64, i64, i64)>) {
        let meta = fs::symlink_metadata(path).unwrap();
        assert!(!meta.is_symlink());
        entries.insert(
            path.strip_prefix(root).unwrap().into(),
            (
                meta.ino(),
                meta.mode(),
                meta.len(),
                meta.mtime(),
                meta.mtime_nsec(),
            ),
        );
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

#[test]
fn racing_installer_cannot_adopt_or_modify_a_completed_winners_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, _) = fixture_paths();
    let prefix = root.join("contested prefix");
    let ready = root.join("ready");
    let a_assets = root.join("a-assets");
    let b_assets = root.join("b-assets");
    fs::create_dir(&a_assets).unwrap();
    fs::create_dir(&b_assets).unwrap();
    fs::write(a_assets.join("shared"), "loser bytes").unwrap();
    fs::write(a_assets.join("loser-only"), "must not merge").unwrap();
    fs::write(b_assets.join("shared"), "winner bytes").unwrap();
    fs::write(b_assets.join("winner-only"), "keep").unwrap();
    let tools = shim(
        root,
        "mkdir",
        r#"if [ "$#" -eq 1 ] && [ "$1" = "$TEST_PREFIX" ]; then
    printf 'ready\n' > "$TEST_READY"
    IFS= read -r release
fi
exec /bin/mkdir "$@""#,
    );
    // Two distinct, genuine native Rust binaries, not script/source substitutes.
    let a_binary = std::env::current_exe().unwrap();
    assert_ne!(fs::read(&a_binary).unwrap(), fs::read(&binary).unwrap());
    let mut a = installer(&script, &a_binary, &prefix, Some(&a_assets))
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("TEST_PREFIX", &prefix)
        .env("TEST_READY", &ready)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        if Instant::now() >= deadline || a.try_wait().unwrap().is_some() {
            let _ = a.kill();
            panic!(
                "installer did not reach exclusive-reservation barrier: {:?}",
                a.wait_with_output()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // B completes after A's initial nonexistence check but before A's mkdir.
    let b = install(&script, &binary, &prefix, Some(&b_assets));
    let bytes = tree(&prefix);
    let metadata = metadata_tree(&prefix);
    a.stdin.take().unwrap().write_all(b"release\n").unwrap();
    let loser = a.wait_with_output().unwrap();
    success(b);
    assert!(!loser.status.success());
    assert!(String::from_utf8_lossy(&loser.stderr).contains("exclusively reserve prefix"));
    assert!(!String::from_utf8_lossy(&loser.stdout).contains("Installed native"));
    assert_eq!(tree(&prefix), bytes);
    assert_eq!(metadata_tree(&prefix), metadata);
    assert_eq!(
        fs::read(prefix.join("bin/coven")).unwrap(),
        fs::read(binary).unwrap()
    );
    assert_eq!(tree(&prefix.join("share/coven/examples")), tree(&b_assets));
}

#[test]
fn prefix_symlink_suffix_matrix_is_refused_without_touching_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    for dangling in [false, true] {
        let target = root.join(format!("target-{dangling}"));
        if !dangling {
            fs::create_dir(&target).unwrap();
        }
        let link = root.join(format!("link-{dangling}"));
        symlink(&target, &link).unwrap();
        for suffix in ["", "/", "/.", "//", "/./", "/./.", "///.//./"] {
            let selected = PathBuf::from(format!("{}{suffix}", link.display()));
            let output = install(&script, &binary, &selected, Some(&assets));
            assert!(!output.status.success(), "{selected:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("prefix must not be a symlink")
            );
            assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
            if dangling {
                assert!(!target.exists());
            } else {
                assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
            }
        }
    }
}

#[test]
fn examples_symlink_suffix_matrix_is_refused_before_prefix_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    let link = root.join("linked-examples");
    let target = root.join("examples");
    copy_tree(&assets, &target);
    symlink(&target, &link).unwrap();
    let before = tree(&target);
    let metadata = metadata_tree(&target);
    for suffix in ["", "/", "/.", "//", "/./", "/./.", "///.//./"] {
        let selected = PathBuf::from(format!("{}{suffix}", link.display()));
        let prefix = root.join("not-created");
        let output = install(&script, &binary, &prefix, Some(&selected));
        assert!(!output.status.success(), "{selected:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--examples must be a directory, not a symlink")
        );
        assert!(!prefix.exists());
        assert_eq!(tree(&target), before);
        assert_eq!(metadata_tree(&target), metadata);
    }
}

#[test]
fn normalized_new_prefix_succeeds_but_existing_empty_prefix_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    let prefix = root.join("empty");
    fs::create_dir(&prefix).unwrap();
    let before = metadata_tree(&prefix);
    assert!(
        !install(&script, &binary, &prefix, Some(&assets))
            .status
            .success()
    );
    assert_eq!(metadata_tree(&prefix), before);
    let new = root.join("new");
    let selected = PathBuf::from(format!("{}/.//./", new.display()));
    let selected_assets = PathBuf::from(format!("{}/.//./", assets.display()));
    success(install(&script, &binary, &selected, Some(&selected_assets)));
    assert_eq!(tree(&new.join("share/coven/examples")), tree(&assets));
    let output = install(&script, &binary, Path::new("///./"), Some(&assets));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("prefix must not exist"));
}

#[test]
fn missing_or_symlink_parent_is_refused_without_creating_a_parent_chain() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    let missing = root.join("missing");
    assert!(
        !install(&script, &binary, &missing.join("prefix"), Some(&assets))
            .status
            .success()
    );
    assert!(!missing.exists());
    let target = root.join("parent");
    fs::create_dir(&target).unwrap();
    let linked = root.join("linked-parent");
    symlink(&target, &linked).unwrap();
    assert!(
        !install(&script, &binary, &linked.join("prefix"), Some(&assets))
            .status
            .success()
    );
    assert_eq!(fs::read_dir(target).unwrap().count(), 0);
}

#[test]
fn dotdot_components_are_refused_before_any_install_effects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    let prefix = root.join("prefix");
    for (selected_binary, selected_prefix, selected_assets) in [
        (
            binary.clone(),
            root.join("absent/../prefix"),
            assets.clone(),
        ),
        (binary.clone(), root.join("prefix/.."), assets.clone()),
        (binary.clone(), prefix.clone(), assets.join("../examples")),
        (binary.join("../coven"), prefix.clone(), assets.clone()),
    ] {
        let output = install(
            &script,
            &selected_binary,
            &selected_prefix,
            Some(&selected_assets),
        );
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(".. path components are not allowed")
        );
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    }
}

#[test]
fn copy_failure_preserves_partial_prefix_reports_failure_and_refuses_retry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (script, binary, assets) = fixture_paths();
    let prefix = root.join("partial");
    let tools = shim(
        root,
        "cp",
        r#"/bin/cp "$TEST_ASSET" "$3/copied-before-error"
printf 'injected copy failure\n' >&2
exit 73"#,
    );
    let output = installer(&script, &binary, &prefix, Some(&assets))
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .env("TEST_ASSET", assets.join("native-custom/goal.md"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("injected copy failure"));
    assert!(stderr.contains("partial installation preserved"));
    assert!(stderr.contains("no rollback performed"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Installed native"));
    assert_eq!(
        fs::read(prefix.join("bin/coven")).unwrap(),
        fs::read(&binary).unwrap()
    );
    assert_eq!(
        fs::read(prefix.join("share/coven/examples/copied-before-error")).unwrap(),
        fs::read(assets.join("native-custom/goal.md")).unwrap(),
    );
    let before = tree(&prefix);
    let metadata = metadata_tree(&prefix);
    let retry = install(&script, &binary, &prefix, Some(&assets));
    assert!(!retry.status.success());
    assert!(String::from_utf8_lossy(&retry.stderr).contains("prefix must not exist"));
    assert_eq!(tree(&prefix), before);
    assert_eq!(metadata_tree(&prefix), metadata);
}
