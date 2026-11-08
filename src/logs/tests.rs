use super::*;
use std::{fs, time::Duration};

const KINDS: [LogKind; 2] = [LogKind::Events, LogKind::Messages];

fn parse(bytes: &[u8]) -> Value {
    values::parse(bytes, Profile::LegacySnapshot).unwrap()
}

// Acquire the real guard, then remove the marker to prove logs never load it.
fn fixture() -> (tempfile::TempDir, WorkspaceGuard) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("coven.json"), b"{}").unwrap();
    let guard = WorkspaceGuard::acquire(dir.path(), Duration::ZERO).unwrap();
    fs::remove_file(dir.path().join("coven.json")).unwrap();
    (dir, guard)
}

struct NeverOpen;
impl AppendIo for NeverOpen {
    fn open(&self, _: &Path) -> Result<File> {
        panic!("refused append opened a file")
    }
}

#[test]
fn append_open_failure_keeps_original_code_and_existing_bytes() {
    struct Refuse;
    impl AppendIo for Refuse {
        fn open(&self, _: &Path) -> Result<File> {
            Err(Error("injected_append_open"))
        }
    }
    let (dir, guard) = fixture();
    for kind in KINDS {
        append_encoded(dir.path(), kind, b"{}\n", &guard).unwrap();
        assert_eq!(
            append_encoded_with(dir.path(), kind, b"{}\n", &guard, &Refuse)
                .unwrap_err()
                .code(),
            "injected_append_open"
        );
        assert_eq!(fs::read(dir.path().join(kind.relative())).unwrap(), b"{}\n");
    }
}

#[test]
fn encoding_is_compact_literal_utf8_last_wins_and_one_object() {
    let record = parse("{\"a\":0,\"text\":\"\u{1f98a}\\n\",\"a\":1}".as_bytes());
    assert_eq!(
        encode_record(&record).unwrap(),
        "{\"a\":1,\"text\":\"\u{1f98a}\\n\"}\n".as_bytes()
    );
    for raw in [b"null".as_slice(), b"[]", b"false", b"0", b"\"text\""] {
        assert_eq!(
            encode_record(&parse(raw)).unwrap_err().code(),
            "invalid_jsonl_record"
        );
    }
    let oversized = Value::object(vec![(
        "s".into(),
        Value::String("x".repeat(values::MAX_STRING + 1)),
    )])
    .unwrap();
    assert_eq!(
        encode_record(&oversized).unwrap_err().code(),
        "unsupported_json_string_profile"
    );
}

#[test]
fn read_validates_whole_log_preserves_values_and_never_creates_files() {
    let dir = tempfile::tempdir().unwrap();
    for kind in KINDS {
        assert!(read(dir.path(), kind).unwrap().is_empty());
        assert_eq!(read_log(dir.path(), kind.relative()).unwrap(), (0, 0));
    }
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    fs::create_dir(dir.path().join("logs")).unwrap();
    for kind in KINDS {
        let path = dir.path().join(kind.relative());
        let raw = b"\n{\"a\":0,\"a\":1,\"values\":[null,false,0,\"\",[],{}]}\r\n\r\n";
        fs::write(&path, raw).unwrap();
        assert_eq!(
            read_log(dir.path(), kind.relative()).unwrap(),
            (raw.len(), 3)
        );
        let records = read(dir.path(), kind).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].compact().unwrap(),
            br#"{"a":1,"values":[null,false,0,"",[],{}]}"#
        );
        for tail in [b"{\n".as_slice(), b"{}", b"\xff\n", b"[]\n"] {
            let bytes = [raw.as_slice(), tail].concat();
            fs::write(&path, &bytes).unwrap();
            assert!(read(dir.path(), kind).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn encoded_admission_and_wrong_guard_precede_parent_creation() {
    let (dir, guard) = fixture();
    let (other, _) = fixture();
    for kind in KINDS {
        for encoded in [
            b"".as_slice(),
            b"\n",
            b" \n",
            b"{}",
            b"{}\n\n",
            b"{}\n{}\n",
            b"[]\n",
            b"\xff\n",
        ] {
            assert!(append_encoded_with(dir.path(), kind, encoded, &guard, &NeverOpen).is_err());
            assert!(!dir.path().join("logs").exists());
        }
        assert!(append_encoded_with(other.path(), kind, b"{}\n", &guard, &NeverOpen).is_err());
        assert!(!other.path().join("logs").exists());
    }
}

#[test]
fn both_kinds_admit_exact_byte_and_line_limits_before_open() {
    let (dir, guard) = fixture();
    fs::create_dir(dir.path().join("logs")).unwrap();
    for kind in KINDS {
        let path = dir.path().join(kind.relative());
        let mut bytes = vec![b' '; MAX_LOG - 3];
        *bytes.last_mut().unwrap() = b'\n';
        fs::write(&path, &bytes).unwrap();
        append_encoded(dir.path(), kind, b"{}\n", &guard).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_LOG as u64);
        assert_eq!(
            append_encoded_with(dir.path(), kind, b"{}\n", &guard, &NeverOpen)
                .unwrap_err()
                .code(),
            "unsupported_event_log_size_profile"
        );
        fs::write(&path, vec![b'\n'; MAX_LINES - 1]).unwrap();
        append_encoded(dir.path(), kind, b"{}\n", &guard).unwrap();
        assert_eq!(read_log(dir.path(), kind.relative()).unwrap().1, MAX_LINES);
        assert_eq!(
            append_encoded_with(dir.path(), kind, b"{}\n", &guard, &NeverOpen)
                .unwrap_err()
                .code(),
            "unsupported_event_log_line_profile"
        );
        for malformed in [b"{\n".as_slice(), b"{}", b"\xff\n"] {
            fs::write(&path, malformed).unwrap();
            assert!(append_encoded_with(dir.path(), kind, b"{}\n", &guard, &NeverOpen).is_err());
            assert_eq!(fs::read(&path).unwrap(), malformed);
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        cell::Cell,
        os::unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt, symlink},
        },
    };

    #[test]
    fn missing_parent_default_mode_exact_bytes_and_existing_inode() {
        struct Probe<'a>(&'a WorkspaceGuard);
        impl AppendIo for Probe<'_> {
            fn open(&self, path: &Path) -> Result<File> {
                self.0.validate_for(self.0.root())?;
                let lock = platform::open_workspace_lock(
                    &self.0.root().join("state/.workspace.lock"),
                    false,
                )?;
                assert!(!platform::try_workspace_lock(&lock)?);
                platform::open_append(path)
            }
        }
        for kind in KINDS {
            let (dir, guard) = fixture();
            let reference = tempfile::tempdir().unwrap();
            fs::create_dir(reference.path().join("default")).unwrap();
            let encoded = encode_record(&parse("{\"text\":\"\u{1f98a}\"}".as_bytes())).unwrap();
            append_encoded_with(dir.path(), kind, &encoded, &guard, &Probe(&guard)).unwrap();
            assert_eq!(
                fs::metadata(dir.path().join("logs"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                fs::metadata(reference.path().join("default"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            );
            let path = dir.path().join(kind.relative());
            assert_eq!(fs::read(&path).unwrap(), encoded);
            let ino = fs::metadata(&path).unwrap().ino();
            append_encoded(dir.path(), kind, &encoded, &guard).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().ino(), ino);
            assert_eq!(fs::read(&path).unwrap(), encoded.repeat(2));
            assert_eq!(fs::read_dir(dir.path().join("logs")).unwrap().count(), 1);
            assert!(!dir.path().join("coven.json").exists());
        }
    }

    #[test]
    fn unsafe_parent_and_leaf_types_are_preserved_without_following() {
        for kind in KINDS {
            for parent in [false, true] {
                for shape in ["symlink", "dangling", "fifo", "wrong_type"] {
                    let (dir, guard) = fixture();
                    let external = tempfile::tempdir().unwrap();
                    fs::write(external.path().join("target"), b"external").unwrap();
                    let path = if parent {
                        dir.path().join("logs")
                    } else {
                        fs::create_dir(dir.path().join("logs")).unwrap();
                        dir.path().join(kind.relative())
                    };
                    match shape {
                        "symlink" => symlink(
                            if parent {
                                external.path().to_owned()
                            } else {
                                external.path().join("target")
                            },
                            &path,
                        )
                        .unwrap(),
                        "dangling" => symlink(external.path().join("missing"), &path).unwrap(),
                        "fifo" => {
                            let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                            // Test-only namespace fault; no descriptor is opened.
                            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                        }
                        _ if parent => fs::write(&path, b"preserve").unwrap(),
                        _ => fs::create_dir(&path).unwrap(),
                    }
                    let before = fs::symlink_metadata(&path).unwrap();
                    assert!(read(dir.path(), kind).is_err());
                    assert!(
                        append_encoded_with(dir.path(), kind, b"{}\n", &guard, &NeverOpen).is_err()
                    );
                    let after = fs::symlink_metadata(&path).unwrap();
                    assert_eq!(after.ino(), before.ino());
                    assert_eq!(after.file_type(), before.file_type());
                    assert_eq!(
                        fs::read(external.path().join("target")).unwrap(),
                        b"external"
                    );
                    assert_eq!(fs::read_dir(external.path()).unwrap().count(), 1);
                }
            }
        }
    }

    struct Fault {
        partial: bool,
        synced: Cell<bool>,
    }
    impl AppendIo for Fault {
        fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
            if self.partial {
                file.write_all(&bytes[..2])?;
                Err(std::io::Error::other("partial"))
            } else {
                file.write_all(bytes)
            }
        }
        fn sync(&self, _: &File) -> std::io::Result<()> {
            self.synced.set(true);
            Err(std::io::Error::other("sync"))
        }
    }

    #[test]
    fn both_partial_error_codes_preserve_prefix_inode_and_uncertain_records() {
        for kind in KINDS {
            for partial in [false, true] {
                let (dir, guard) = fixture();
                append_encoded(dir.path(), kind, b"{\"old\":1}\n", &guard).unwrap();
                let path = dir.path().join(kind.relative());
                let ino = fs::metadata(&path).unwrap().ino();
                let fault = Fault {
                    partial,
                    synced: Cell::new(false),
                };
                assert_eq!(
                    append_encoded_with(dir.path(), kind, b"{\"new\":2}\n", &guard, &fault)
                        .unwrap_err()
                        .code(),
                    match kind {
                        LogKind::Events => "event_append_may_be_partial",
                        LogKind::Messages => "message_append_may_be_partial",
                    }
                );
                assert_eq!(fault.synced.get(), !partial);
                assert_eq!(fs::metadata(&path).unwrap().ino(), ino);
                let expected = if partial {
                    b"{\"old\":1}\n{\"".as_slice()
                } else {
                    b"{\"old\":1}\n{\"new\":2}\n"
                };
                assert_eq!(fs::read(&path).unwrap(), expected);
                if partial {
                    assert!(
                        append_encoded_with(dir.path(), kind, b"{}\n", &guard, &NeverOpen).is_err()
                    );
                    assert_eq!(fs::read(&path).unwrap(), expected);
                } else {
                    append_encoded(dir.path(), kind, b"{\"new\":2}\n", &guard).unwrap();
                    assert_eq!(
                        fs::read(&path).unwrap(),
                        [expected, b"{\"new\":2}\n"].concat()
                    );
                }
            }
        }
    }
}
