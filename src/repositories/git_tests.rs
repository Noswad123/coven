use super::*;

#[test]
fn byte_status_renames_and_invalid_framing() {
    assert_eq!(
        status(b"R  to\0from\0?? nonutf8-\xff\n\0").unwrap()["untracked"],
        1
    );
    assert_eq!(status(b"R  to\0from\0").unwrap()["tracked_changes"], 1);
    for data in [
        b"?? name".as_slice(),
        b"XX name\0",
        b"?? \0",
        b"R  name\0",
        b"   name\0",
        b"UA\0",
        b"UM name\0",
        b"DM name\0",
    ] {
        assert!(status(data).is_err(), "{data:?}");
    }
}

#[test]
fn independent_git_object_digests_and_strict_tree_order() {
    assert_eq!(
        object_hash("tree", b"", "sha1"),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    );
    assert_eq!(
        object_hash("blob", b"hello\n", "sha1"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
    let oid = [1; 20];
    let mut valid = b"40000 a\0".to_vec();
    valid.extend_from_slice(&oid);
    valid.extend_from_slice(b"100644 b\0");
    valid.extend_from_slice(&oid);
    assert!(validate_tree(&valid, 20).is_ok());
    for raw in [
        b"broken".to_vec(),
        b"100644 missing-oid\0".to_vec(),
        [b"999999 file\0".as_slice(), &oid].concat(),
        [b"100644 file\0".as_slice(), &[0; 20]].concat(),
        [valid.as_slice(), valid.as_slice()].concat(),
    ] {
        assert!(validate_tree(&raw, 20).is_err());
    }
}

#[test]
fn closed_commands_exact_environment_version_and_config() {
    assert!(Command::Locator("--evil").args().is_err());
    assert!(Command::Tree("HEAD").args().is_err());
    assert_eq!(environment().len(), 13);
    assert_eq!(environment()[&OsString::from("GIT_NO_LAZY_FETCH")], "1");
    assert!(!environment().contains_key(&OsString::from("LD_PRELOAD")));
    for version in [
        b"git version 2.48.0\n".as_slice(),
        b"git version 2.56.0\n",
        b"git version 3.0.0.vendor\n",
    ] {
        assert!(valid_version(version));
    }
    for version in [
        b"git version 2.47.9\n".as_slice(),
        b"git version 2.48\n",
        b"git version 2.48.0\nextra\n",
    ] {
        assert!(!valid_version(version));
    }
    for key in [
        b"filter.evil.clean".as_slice(),
        b"include.path",
        b"remote.origin.promisor",
        b"extensions.worktreeconfig",
        b"core.alternaterefscommand",
    ] {
        assert!(forbidden_config(key));
    }
    assert!(!forbidden_config(b"extensions.objectformat"));
}

#[test]
fn fake_probe_boundary_caps_deadlines_status_and_closed_transport() {
    use crate::process::ProbeResult;
    use std::cell::Cell;
    let selected = identity::find_executable("git").unwrap();
    struct Fake {
        status: ProbeStatus,
        code: Option<i32>,
        out: Vec<u8>,
        err: Vec<u8>,
        calls: Cell<usize>,
    }
    impl ProbeRunner for Fake {
        fn run(&self, r: &ProbeRequest) -> ProbeResult {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(r.environment, environment());
            assert_eq!(r.cwd, Path::new("/"));
            assert!(r.timeout > Duration::ZERO && r.timeout <= Duration::from_secs(3));
            assert_eq!(r.max_output_bytes, MAX_OUTPUT);
            assert!(
                r.arguments
                    .starts_with(&GUARDS.iter().map(OsString::from).collect::<Vec<_>>())
            );
            ProbeResult {
                status: self.status,
                returncode: self.code,
                stdout: self.out.clone(),
                stderr: self.err.clone(),
                duration_ms: 0.0,
            }
        }
    }
    for status in [
        ProbeStatus::Nonzero,
        ProbeStatus::Timeout,
        ProbeStatus::OutputLimit,
        ProbeStatus::SpawnError,
    ] {
        let fake = Fake {
            status,
            code: Some(1),
            out: Vec::new(),
            err: Vec::new(),
            calls: Cell::new(0),
        };
        let observation = Observation::new(PathBuf::from("/"), selected.clone(), &fake).unwrap();
        assert!(observation.probe(Command::Version).is_err());
        assert_eq!(
            observation.probe(Command::Head).is_ok(),
            status == ProbeStatus::Nonzero
        );
    }
    let fake = Fake {
        status: ProbeStatus::Ok,
        code: Some(0),
        out: vec![b'x'; MAX_OUTPUT - 1],
        err: vec![b'y'],
        calls: Cell::new(0),
    };
    let mut observation = Observation::new(PathBuf::from("/"), selected.clone(), &fake).unwrap();
    assert_eq!(
        observation.probe(Command::Version).unwrap().len(),
        MAX_OUTPUT - 1
    );
    observation.deadline = Instant::now();
    assert!(observation.probe(Command::Version).is_err());
    assert_eq!(fake.calls.get(), 1);
    let fake = Fake {
        err: vec![b'y'; 2],
        ..fake
    };
    let observation = Observation::new(PathBuf::from("/"), selected, &fake).unwrap();
    assert!(observation.probe(Command::Version).is_err());
}

#[cfg(unix)]
#[test]
fn selected_git_alias_retains_parent_components_in_filesystem_order() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("lexical")).unwrap();
    fs::create_dir_all(temp.path().join("actual/nested")).unwrap();
    symlink(
        temp.path().join("actual/nested"),
        temp.path().join("lexical/alias"),
    )
    .unwrap();
    let executable = temp.path().join("actual/git");
    fs::write(&executable, b"trusted fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let selected = temp.path().join("lexical/alias/../git");
    struct Never;
    impl ProbeRunner for Never {
        fn run(&self, _: &ProbeRequest) -> crate::process::ProbeResult {
            panic!("identity setup must not spawn")
        }
    }
    let observation = Observation::new(temp.path().to_owned(), selected.clone(), &Never).unwrap();
    assert_eq!(observation.selected.as_os_str(), selected.as_os_str());
    assert_eq!(observation.executable, executable.canonicalize().unwrap());
    observation.check_executable().unwrap();
}
