use super::*;
use std::{fs, fs::File, io::Write};

fn executable(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    // Reuse executable permissions without depending on installed native tools.
    let permissions = fs::metadata(std::env::current_exe().unwrap())
        .unwrap()
        .permissions();
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn real_bytes_metadata_header_and_equal_hash() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    executable(&first, b"abc"); // No executable header is required for observation.
    executable(&second, b"abc");
    let identity = fingerprint(&first).unwrap();
    assert_eq!(identity.metadata, metadata(&first).unwrap());
    assert_eq!(identity.metadata.size, 3);
    assert_eq!(
        identity.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(identity.sha256, fingerprint(&second).unwrap().sha256);
    assert_eq!(identity, fingerprint(&first).unwrap());
    executable(&first, b"");
    assert_eq!(
        fingerprint(&first).unwrap().sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn flat_public_values_keep_exact_python_integers() {
    let identity = Identity {
        metadata: platform::FileMetadata {
            device: u64::MAX,
            inode: u64::MAX - 1,
            size: 1 << 30,
            mtime_ns: i128::MIN,
        },
        sha256: "a".repeat(64),
    };
    let value = identity.value().unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.entries().len(), 5);
    for (key, expected) in [
        ("device", u64::MAX.to_string()),
        ("inode", (u64::MAX - 1).to_string()),
        ("size", (1_u64 << 30).to_string()),
        ("mtime_ns", i128::MIN.to_string()),
    ] {
        assert_eq!(
            object.get(key).unwrap().compact().unwrap(),
            expected.as_bytes()
        );
    }
    assert_eq!(
        object.get("sha256").unwrap().as_str(),
        Some(identity.sha256.as_str())
    );
}

#[test]
fn limits_are_closed_and_finite() {
    for timeout in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        30.00001,
    ] {
        assert!(validate_limits(timeout, 1024).is_err());
    }
    for bytes in [i64::MIN, -1, 0, 1023, 1048577, i64::MAX] {
        assert!(validate_limits(1.0, bytes).is_err());
    }
    assert_eq!(
        validate_limits(30.0, 1048576).unwrap(),
        (Duration::from_secs(30), 1048576)
    );
    assert!(validate_limits(f64::MIN_POSITIVE, 1024).is_ok());
}

#[test]
fn environment_is_only_inherited_allowlist_without_defaults() {
    let entries = ENV_KEYS
        .into_iter()
        .chain([
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "OPENAI_API_KEY",
            "COVEN_ROOT",
            "DJINN_CONFIG",
            "KITSUNE_SOCKET_PATH",
            "LC_CTYPE",
            "path",
        ])
        .map(|key| (OsString::from(key), OsString::from("value")));
    let filtered = filtered_environment(entries);
    assert_eq!(filtered.len(), 6);
    assert!(
        ENV_KEYS
            .iter()
            .all(|key| filtered.get(OsStr::new(key)) == Some(&OsString::from("value")))
    );
    assert!(filtered_environment([]).is_empty());
    assert_eq!(
        adapter_environment(),
        filtered_environment(std::env::vars_os())
    );
}

#[test]
fn bounded_hash_and_growth_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("executable");
    executable(&path, b"abcd");
    assert!(fingerprint_checked(&path, 3, |_| Ok(())).is_err());
    assert!(fingerprint_checked(&path, 4, |_| Ok(())).is_ok());
    let mut grew = false;
    let result = fingerprint_checked(&path, 4, |checkpoint| {
        if checkpoint == Checkpoint::AfterRead && !grew {
            fs::OpenOptions::new()
                .append(true)
                .open(&path)?
                .write_all(b"e")?;
            grew = true;
        }
        Ok(())
    });
    assert!(grew && result.is_err());
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_EXECUTABLE_BYTES + 1)
        .unwrap();
    assert!(fingerprint(&path).is_err());
}

#[test]
fn path_descriptor_replacement_and_truncation_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected");
    let replacement = directory.path().join("replacement");
    for stage in [
        Checkpoint::BeforeOpen,
        Checkpoint::AfterOpen,
        Checkpoint::AfterRead,
    ] {
        executable(&path, b"original");
        executable(&replacement, b"replaced");
        let mut changed = false;
        assert!(
            fingerprint_checked(&path, 64, |checkpoint| {
                if checkpoint == stage && !changed {
                    fs::rename(&replacement, &path)?;
                    changed = true;
                }
                Ok(())
            })
            .is_err()
        );
        assert!(changed);
    }
    executable(&path, b"original");
    assert!(
        fingerprint_checked(&path, 64, |stage| {
            if stage == Checkpoint::AfterOpen {
                fs::write(&path, b"short")?;
            }
            Ok(())
        })
        .is_err()
    );
}

#[test]
fn same_metadata_rewrite_changes_identity_hash() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected");
    executable(&path, b"original");
    let original = fingerprint(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    // Use the same length so metadata alone cannot distinguish the rewrite.
    fs::write(&path, b"changed!").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let rewritten = fingerprint(&path).unwrap();
    assert_eq!(original.metadata, rewritten.metadata);
    assert_ne!(original.sha256, rewritten.sha256);
}

#[test]
fn path_lookup_does_not_hash_or_default_empty_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool");
    executable(&path, b"not a native executable header");
    assert_eq!(
        find_in_path("tool", Some(directory.path().as_os_str())),
        Some(path.clone())
    );
    assert_eq!(find_in_path("tool", Some(OsStr::new(""))), None);
    assert_eq!(find_in_path("", None), None);
    assert_eq!(
        find_in_path(path.to_str().unwrap(), Some(OsStr::new(""))),
        Some(path.clone())
    );
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_EXECUTABLE_BYTES + 1)
        .unwrap();
    assert_eq!(
        find_in_path("tool", Some(directory.path().as_os_str())),
        Some(path)
    );
    assert_eq!(find_executable(""), None);
}

#[test]
fn relative_path_directories_and_unset_default() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let cwd = std::env::current_dir().unwrap();
    let relative_directory = directory.path().strip_prefix(&cwd).unwrap();
    let path = relative_directory.join("identity-test-tool");
    executable(&path, b"abc");
    assert!(!path.is_absolute());
    assert_eq!(
        find_in_path("identity-test-tool", Some(relative_directory.as_os_str())),
        Some(path.clone())
    );
    assert_eq!(
        find_in_path(path.to_str().unwrap(), None),
        Some(path.clone())
    );
    assert_eq!(
        find_in_path("identity-missing-tool", None),
        find_in_path("identity-missing-tool", Some(OsStr::new("/bin:/usr/bin")))
    );

    // Resolve Python before changing only the child's PATH, including to empty/unset.
    let python = find_executable("python3").unwrap().canonicalize().unwrap();
    let search_path =
        std::env::join_paths([Path::new("missing-identity-bin"), relative_directory]).unwrap();
    for (name, search) in [
        ("identity-test-tool", Some(relative_directory.as_os_str())),
        ("identity-test-tool", Some(search_path.as_os_str())),
        ("identity-test-tool", Some(OsStr::new(""))),
        (path.to_str().unwrap(), Some(OsStr::new(""))),
        ("identity-missing-tool", None),
        ("sh", None),
    ] {
        let mut command = std::process::Command::new(&python);
        command
            .args(["-B", "-c", include_str!("tests/which_oracle.py")])
            .arg(name)
            .current_dir(&cwd)
            .env("PYTHONDONTWRITEBYTECODE", "1");
        match search {
            Some(value) => {
                command.env("PATH", value);
            }
            None => {
                command.env_remove("PATH");
            }
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Option<PathBuf> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            find_in_path(name, search),
            expected,
            "name={name:?}, PATH={search:?}"
        );
    }
}

#[test]
fn legacy_python_oracle_matches_real_file_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool");
    let bytes: Vec<u8> = (0..200_000).map(|index| (index % 256) as u8).collect();
    executable(&path, &bytes);
    let output = std::process::Command::new("python3")
        .args(["-B", "-c", include_str!("tests/oracle.py")])
        .arg(&path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle = crate::values::parse(&output.stdout, crate::values::Profile::Authority).unwrap();
    assert_eq!(
        fingerprint(&path)
            .unwrap()
            .value()
            .unwrap()
            .canonical()
            .unwrap(),
        oracle.canonical().unwrap()
    );
}

#[cfg(unix)]
mod unix;
