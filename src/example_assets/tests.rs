use super::*;

fn bundle(root: &Path) -> AssetBundle {
    AssetBundle::resolve(Some(root.as_os_str().into()), None, Path::new("/unused")).unwrap()
}

fn fixture(root: &Path, manifest: &[u8]) -> PathBuf {
    let directory = root.join("custom");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("example.json"), manifest).unwrap();
    directory
}

#[test]
fn optional_defaults_newlines_metadata_coercion_and_raw_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let directory = fixture(
        temp.path(),
        br#"{"id": [], "seed_message": [true, null, {"x": 2}]}"#,
    );
    fs::write(
        directory.join("goal.md"),
        "Unicode \u{1f980}\r\nnext\rfinal",
    )
    .unwrap();
    fs::write(directory.join("prompt-context.md"), "context\r\n").unwrap();
    let raw = br#"[{"title":"Draft","extra":{"keep":true},"owner":null}]"#;
    fs::write(directory.join("tasks.json"), raw).unwrap();
    let assets = bundle(temp.path());
    let example = assets.load("../Custom").unwrap();
    assert_eq!(example.id, "custom");
    assert_eq!(example.seed_message, "[True, None, {'x': 2}]");
    assert_eq!(example.goal, "Unicode \u{1f980}\nnext\nfinal\n");
    assert_eq!(
        example.spec,
        "# Spec\n\nDescribe the desired behavior here.\n"
    );
    assert_eq!(example.work, super::super::default_work());
    assert_eq!(example.output, super::super::default_output());
    assert_eq!(example.tasks.compact().unwrap(), raw);
    assert_eq!(assets.context(Some("custom")).unwrap(), "context\n");
    assert_eq!(fs::read(directory.join("tasks.json")).unwrap(), raw);
    assert_eq!(
        fs::read(directory.join("goal.md")).unwrap(),
        "Unicode \u{1f980}\r\nnext\rfinal".as_bytes()
    );
}

#[test]
fn manifest_shapes_truthiness_and_empty_optional_files() {
    for (manifest, id, message) in [
        (
            r#"{"id": true, "seed_message": false}"#,
            "True",
            "Seeded example: True",
        ),
        (
            r#"{"id": 42, "seed_message": {"x": "y"}}"#,
            "42",
            "{'x': 'y'}",
        ),
        (
            r#"{"id": 0, "seed_message": ""}"#,
            "custom",
            "Seeded example: custom",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let directory = fixture(temp.path(), manifest.as_bytes());
        fs::write(directory.join("goal.md"), "").unwrap();
        fs::write(directory.join("spec.md"), "").unwrap();
        let example = bundle(temp.path()).load("custom").unwrap();
        assert_eq!(example.id, id);
        assert_eq!(example.seed_message, message);
        assert_eq!(example.goal, "\n");
        assert_eq!(example.spec, "");
    }
    for manifest in [b"null".as_slice(), b"[]", b"true", b"", b"{"] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), manifest);
        assert!(bundle(temp.path()).load("custom").is_err());
    }
}

#[test]
fn explicit_roots_are_absolute_authoritative_and_override_demo() {
    let temp = tempfile::tempdir().unwrap();
    let assets = bundle(temp.path());
    assert!(assets.load("interactive-demo").is_err());
    fs::create_dir(temp.path().join("interactive-demo")).unwrap();
    fs::write(temp.path().join("interactive-demo/example.json"), "{}").unwrap();
    fs::write(temp.path().join("interactive-demo/goal.md"), "edited demo").unwrap();
    assert_eq!(
        assets.load("interactive-demo").unwrap().goal,
        "edited demo\n"
    );
    assert!(AssetBundle::resolve(Some("relative".into()), None, Path::new("/unused")).is_err());
    assert!(
        AssetBundle::resolve(
            Some(temp.path().join("missing").into_os_string()),
            None,
            Path::new("/unused")
        )
        .is_err()
    );
    assert!(assets.load("").is_err());
    assert!(assets.context(Some("")).is_err());
    let home = temp.path().join("home");
    fs::create_dir_all(home.join("examples")).unwrap();
    assert_eq!(
        AssetBundle::resolve(None, Some(home.into_os_string()), Path::new("/unused"))
            .unwrap()
            .root
            .unwrap(),
        temp.path().join("home/examples")
    );
    assert_eq!(
        AssetBundle::resolve(
            Some(temp.path().as_os_str().into()),
            Some("/ignored".into()),
            Path::new("/unused")
        )
        .unwrap()
        .root
        .unwrap(),
        temp.path()
    );
}

#[test]
#[cfg(unix)]
fn selected_root_leaf_symlinks_are_rejected_despite_trailing_separators_and_dots() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    fs::create_dir(&real).unwrap();
    let alias = temp.path().join("alias");
    symlink(&real, &alias).unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    symlink(&real, home.join("examples")).unwrap();
    for suffix in ["", "/", "/.", "//./"] {
        let selected = OsString::from(format!("{}{suffix}", alias.display()));
        assert_eq!(
            AssetBundle::resolve(Some(selected), None, Path::new("/unused"))
                .err()
                .unwrap()
                .code(),
            "unsafe_example_root",
            "examples alias{suffix}"
        );
        let selected_home = OsString::from(format!("{}{suffix}", home.display()));
        assert_eq!(
            AssetBundle::resolve(None, Some(selected_home), Path::new("/unused"))
                .err()
                .unwrap()
                .code(),
            "unsafe_example_root",
            "home{suffix}/examples alias"
        );
        let directory = OsString::from(format!("{}{suffix}", real.display()));
        assert_eq!(
            AssetBundle::resolve(Some(directory), None, Path::new("/unused"))
                .unwrap()
                .root
                .unwrap(),
            real
        );
    }
    // The contract rejects the selected root leaf, not symlinked ancestors.
    fs::create_dir(real.join("examples")).unwrap();
    for suffix in ["", "/", "/.", "//./"] {
        let selected_home = OsString::from(format!("{}{suffix}", alias.display()));
        assert_eq!(
            AssetBundle::resolve(None, Some(selected_home), Path::new("/unused"))
                .unwrap()
                .root
                .unwrap(),
            alias.join("examples")
        );
    }
    assert_eq!(
        AssetBundle::resolve(Some("///./".into()), None, Path::new("/unused"))
            .unwrap()
            .root
            .unwrap(),
        Path::new("/")
    );
    // Do not collapse `..` across a symlink and silently select another root.
    fs::create_dir(real.join("nested")).unwrap();
    let nested_alias = temp.path().join("nested-alias");
    symlink(real.join("nested"), &nested_alias).unwrap();
    let selected = nested_alias.join("../examples/.");
    assert_eq!(
        AssetBundle::resolve(Some(selected.into_os_string()), None, Path::new("/unused"))
            .unwrap()
            .root
            .unwrap(),
        nested_alias.join("../examples")
    );
}

#[test]
fn relocates_from_canonical_executable_and_never_build_checkout() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("bin")).unwrap();
    let exe = temp.path().join("bin/coven");
    fs::write(&exe, "binary").unwrap();
    let portable = AssetBundle::resolve(None, None, &exe).unwrap();
    assert!(portable.root.is_none());
    assert_eq!(
        portable.load("interactive-demo").unwrap().goal,
        embedded_example().unwrap().goal
    );
    fs::create_dir_all(temp.path().join("share/coven/examples")).unwrap();
    let assets = AssetBundle::resolve(None, None, &exe).unwrap();
    assert_eq!(
        assets.root.unwrap(),
        temp.path().join("share/coven/examples")
    );
    #[cfg(unix)]
    {
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&exe, &alias).unwrap();
        assert_eq!(
            AssetBundle::resolve(None, None, &alias)
                .unwrap()
                .root
                .unwrap(),
            temp.path().join("share/coven/examples")
        );
    }
    fs::create_dir_all(temp.path().join("target/debug/deps")).unwrap();
    fs::create_dir(temp.path().join("examples")).unwrap();
    let dev = temp.path().join("target/debug/deps/test");
    fs::write(&dev, "binary").unwrap();
    assert_eq!(
        AssetBundle::resolve(None, None, &dev)
            .unwrap()
            .root
            .unwrap(),
        temp.path().join("examples")
    );
}

#[test]
fn bounded_safe_regular_leaves_and_directories() {
    for relative in [
        "example.json",
        "goal.md",
        "spec.md",
        "prompt-context.md",
        "tasks.json",
        "work/README.md",
        "output/README.md",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let directory = fixture(temp.path(), b"{}");
        let path = directory.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        fs::create_dir(&path).unwrap();
        assert!(bundle(temp.path()).load("custom").is_err(), "{relative}");
    }
    for (relative, limit) in [
        ("goal.md", TEXT_LIMIT),
        ("tasks.json", JSON_LIMIT),
        ("example.json", JSON_LIMIT),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let directory = fixture(temp.path(), b"{}");
        fs::write(directory.join(relative), vec![b' '; limit + 1]).unwrap();
        assert!(bundle(temp.path()).load("custom").is_err(), "{relative}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        for relative in ["goal.md", "prompt-context.md", "tasks.json", "example.json"] {
            let temp = tempfile::tempdir().unwrap();
            let directory = fixture(temp.path(), b"{}");
            let path = directory.join(relative);
            if path.exists() {
                fs::remove_file(&path).unwrap();
            }
            symlink("/dev/zero", &path).unwrap();
            assert!(bundle(temp.path()).load("custom").is_err());
            fs::remove_file(&path).unwrap();
            let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
            assert!(bundle(temp.path()).load("custom").is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let directory = fixture(temp.path(), b"{}");
        symlink("/tmp", directory.join("work")).unwrap();
        assert!(bundle(temp.path()).load("custom").is_err());
    }
}
