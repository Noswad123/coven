use super::*;
use std::{
    cell::RefCell,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

struct ChildOwner(RefCell<Child>);
impl ChildOwner {
    fn wait(&self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.0.borrow_mut().try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "child did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let child = self.0.get_mut();
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn wait_signal(ipc: &Path, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ipc.join(name).exists() {
        assert!(
            Instant::now() < deadline,
            "waiting for {name}: {}",
            fs::read_to_string(ipc.join("stderr")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn adversarial_marker() -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let proof = include_bytes!("../../tests/retirement/fixtures/source_lead_init_provenance.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(proof)),
        "456db89e702df84b24f0ba98014fff88663b72e6b3acdfd088614be2d55a54d5"
    );
    let raw = include_bytes!("../../tests/retirement/fixtures/source_lead_init.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(raw)),
        "77deb8d7d7b6d40154d4618a45faad21defd621c06cf7400531d8cd30c9f3eab"
    );
    let corpus: serde_json::Value = serde_json::from_slice(raw).unwrap();
    let marker = corpus["marker"]["raw_utf8"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(
        format!("{:x}", Sha256::digest(&marker)),
        "f0ec9ccf310d833846add711eb10bee013b1be63f465e827fb9ba7d9889d2172"
    );
    assert_eq!(corpus["marker"]["mode"], 0o644);
    marker
}

#[test]
#[ignore = "self-exec entry exercised by python_initial_marker_is_not_serialized_by_native_reservation"]
fn adversarial_exclusive_marker_writer_child() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let owned = PathBuf::from(std::env::var_os("COVEN_INIT_MARKER_CHILD_ROOT").unwrap());
    assert!(owned.is_absolute());
    let root = owned.join("workspace");
    let ipc = owned.join("ipc");
    assert!(ipc.is_dir());
    // Independent adversary: no production init, preflight, or reservation API.
    assert!(!root.exists());
    fs::write(ipc.join("preflight-passed"), b"ready").unwrap();
    wait_signal(&ipc, "continue-preflight");
    assert!(root.join("state/.workspace.lock").is_file());
    assert!(!root.join("coven.json").exists());
    // Set only this disposable process's umask, matching the historical capture.
    unsafe {
        libc::umask(0o022);
    }
    let marker = adversarial_marker();
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .open(root.join("coven.json"))
        .unwrap();
    handle.write_all(&marker).unwrap();
    drop(handle);
    fs::write(ipc.join("marker-published"), b"ready").unwrap();
    wait_signal(&ipc, "continue-after-marker");
    assert_eq!(fs::read(root.join("coven.json")).unwrap(), marker);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
    assert_eq!(fs::read_dir(root.join("state")).unwrap().count(), 1);
}

#[test]
fn python_initial_marker_is_not_serialized_by_native_reservation() {
    use std::os::unix::fs::PermissionsExt;
    // Historical test name retained for provenance. Python-specific live
    // compatibility is historical; current coverage is a native OS-open adversary.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    let ipc = temp.path().join("ipc");
    fs::create_dir(&ipc).unwrap();
    let child = ChildOwner(RefCell::new(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "init::tests::overlap::adversarial_exclusive_marker_writer_child",
                "--ignored",
                "--nocapture",
            ])
            .env("COVEN_INIT_MARKER_CHILD_ROOT", temp.path())
            .env_remove("COVEN_MULTIPLEXER")
            .env_remove("COVEN_COMMAND")
            .stdout(Stdio::null())
            .stderr(fs::File::create(ipc.join("stderr")).unwrap())
            .spawn()
            .unwrap(),
    ));
    wait_signal(&ipc, "preflight-passed");
    assert!(!root.exists());
    struct Overlap {
        ipc: PathBuf,
        marker: RefCell<Vec<u8>>,
    }
    impl InitIo for Overlap {
        fn boundary(&self, operation: &str, root: &Path) -> Result<()> {
            if operation == "recheck-reserved" {
                assert!(root.join("state/.workspace.lock").exists());
                assert!(!root.join("coven.json").exists());
                fs::write(self.ipc.join("continue-preflight"), b"go")?;
                wait_signal(&self.ipc, "marker-published");
                let marker = fs::read(root.join("coven.json"))?;
                assert_eq!(marker, adversarial_marker());
                assert_eq!(
                    fs::metadata(root.join("coven.json"))?.permissions().mode() & 0o777,
                    0o644
                );
                let team = values::parse(&marker, Profile::LegacySnapshot)?;
                assert_eq!(
                    team.as_object().unwrap().get("name").unwrap().as_str(),
                    Some("python-owner")
                );
                *self.marker.borrow_mut() = marker;
                assert!(crate::storage::WorkspaceGuard::existing(root, Duration::ZERO).is_err());
            }
            Ok(())
        }
    }
    let io = Overlap {
        ipc: ipc.clone(),
        marker: RefCell::new(vec![]),
    };
    let result = arguments_with(&root, &blank(), &io).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert!(result.warnings[0].contains("init_reserved_name_exists"));
    assert!(result.warnings[0].contains("phase=recheck-reserved"));
    assert!(result.warnings[0].contains("partial_file=false"));
    let marker = io.marker.into_inner();
    assert_eq!(fs::read(root.join("coven.json")).unwrap(), marker);
    assert_eq!(
        fs::metadata(root.join("coven.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    let mut entries = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, ["coven.json", "state"]);
    assert_eq!(fs::read_dir(root.join("state")).unwrap().count(), 1);
    assert_eq!(
        arguments_with(
            &root,
            &Options {
                force: true,
                ..blank()
            },
            &Fixed
        )
        .err()
        .unwrap()
        .code(),
        "init_reserved_name_exists"
    );
    fs::write(ipc.join("continue-after-marker"), b"go").unwrap();
    assert!(
        child.wait().success(),
        "{}",
        fs::read_to_string(ipc.join("stderr")).unwrap()
    );
    assert_eq!(fs::read(root.join("coven.json")).unwrap(), marker);
    assert_eq!(
        workspace::load(&root).unwrap().team.agent_ids().unwrap(),
        ["python"]
    );
    assert!(arguments_with(&root, &blank(), &Fixed).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn existing_root_below_execute_only_parent_succeeds_unprivileged() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "COVEN_INIT_EXECUTE_ONLY_PARENT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("execute-only");
        let root = parent.join("workspace");
        fs::create_dir_all(&root).unwrap();
        // Privilege changes occur only in this disposable test process.
        if unsafe { libc::geteuid() } == 0 {
            use std::os::unix::ffi::OsStrExt;
            for path in [temp.path(), parent.as_path(), root.as_path()] {
                let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::chown(path.as_ptr(), 65534, 65534) }, 0);
            }
            assert_eq!(unsafe { libc::setgid(65534) }, 0);
            assert_eq!(unsafe { libc::setuid(65534) }, 0);
        }
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o111)).unwrap();
        assert!(
            fs::File::open(&parent).is_err(),
            "probe must genuinely deny directory read access"
        );
        let result = arguments_with(&root, &blank(), &Fixed).unwrap();
        // Restore fixture traversal for cleanup even when asserting a failure.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result.exit_code, 0, "{:?}", result.warnings);
        assert!(workspace::load(&root).is_ok());
        return;
    }
    let child = ChildOwner(RefCell::new(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "init::tests::overlap::existing_root_below_execute_only_parent_succeeds_unprivileged", "--nocapture"])
        .env(CHILD, "1").env_remove("COVEN_COMMAND").env_remove("COVEN_MULTIPLEXER")
        .spawn().unwrap()));
    assert!(child.wait().success());
}
