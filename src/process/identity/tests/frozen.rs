use super::*;
use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

fn corpus() -> serde_json::Value {
    let corpus = include_bytes!("../../../../tests/retirement/fixtures/process_identity.json");
    let provenance =
        include_bytes!("../../../../tests/retirement/fixtures/process_identity_provenance.json");
    for (raw, expected) in [
        (
            corpus.as_slice(),
            "ee3f56e401ae88caddd2ad9165537e4a244a32baa98dfb1fdd7156285951bf37",
        ),
        (
            provenance.as_slice(),
            "6fb4e35e30b383305ae2b292a7f694253e7b0c14d424f3d4e09cef1f433cf1ff",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(raw)), expected);
    }
    let corpus: serde_json::Value = serde_json::from_slice(corpus).unwrap();
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(
        corpus["which_cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "first-entry",
            "reversed-order",
            "empty-leading",
            "empty-trailing",
            "empty-only-segments",
            "absolute-direct",
            "dot-direct",
            "repeated-separators",
            "nonexec-before-hit",
            "directory-before-hit",
            "symlink",
            "dangling",
        ]
    );
    corpus
}

pub(super) fn identity(path: &Path) -> Value {
    let frozen = corpus();
    // Only these three runtime leaves are bound. Size and hashlib digest stay literal.
    let info = fs::symlink_metadata(path).unwrap();
    assert!(info.is_file());
    let mtime_ns = i128::from(info.mtime()) * 1_000_000_000 + i128::from(info.mtime_nsec());
    Value::object(vec![
        (
            "device".into(),
            Value::integer(&info.dev().to_string()).unwrap(),
        ),
        (
            "inode".into(),
            Value::integer(&info.ino().to_string()).unwrap(),
        ),
        (
            "mtime_ns".into(),
            Value::integer(&mtime_ns.to_string()).unwrap(),
        ),
        (
            "size".into(),
            Value::integer(&frozen["identity"]["size"].to_string()).unwrap(),
        ),
        (
            "sha256".into(),
            Value::String(frozen["identity"]["sha256"].as_str().unwrap().into()),
        ),
    ])
    .unwrap()
}

pub(super) fn default_node(name: &str) -> Option<PathBuf> {
    // Finite host-observation table, not a second PATH parser. Python CS_PATH is
    // pinned in provenance; don't freeze this host's /bin symlink layout.
    let nodes = match name {
        "sh" => ["/bin/sh", "/usr/bin/sh"],
        "identity-missing-tool" => [
            "/bin/identity-missing-tool",
            "/usr/bin/identity-missing-tool",
        ],
        _ => panic!("undeclared default-node observation"),
    };
    nodes.into_iter().find_map(|node| {
        let info = fs::metadata(node).ok()?;
        let encoded = std::ffi::CString::new(node).unwrap();
        (info.is_file() && unsafe { libc::access(encoded.as_ptr(), libc::X_OK) } == 0)
            .then(|| PathBuf::from(node))
    })
}

pub(super) fn which_corpus() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let root = directory.path();
    let cwd = std::env::current_dir().unwrap();
    let relative = root.strip_prefix(&cwd).unwrap();
    assert!(!Path::new("identity-corpus-tool").exists());
    for name in ["first", "second", "nonexec", "directory"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    for name in ["first", "second", "nonexec"] {
        let node = root.join(name).join("identity-corpus-tool");
        executable(&node, b"abc");
        if name == "nonexec" {
            fs::set_permissions(node, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    fs::create_dir(root.join("directory/identity-corpus-tool")).unwrap();
    symlink("first/identity-corpus-tool", root.join("alias")).unwrap();
    symlink("absent", root.join("dangling")).unwrap();
    let expand = |text: &str| {
        text.replace("$ROOT", root.to_str().unwrap())
            .replace("$REL", relative.to_str().unwrap())
    };
    for case in corpus()["which_cases"].as_array().unwrap() {
        let name = expand(case["name"].as_str().unwrap());
        let search = expand(case["path"].as_str().unwrap());
        let expected = case["expected"].as_str().map(expand).map(OsString::from);
        let actual = find_in_path(&name, Some(OsStr::new(&search)));
        // Path equality normalizes dots/repeated separators; lexical bytes must not.
        assert_eq!(
            actual.as_deref().map(Path::as_os_str),
            expected.as_deref(),
            "{}",
            case["id"]
        );
    }
}
