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

#[test]
fn python_initial_marker_is_not_serialized_by_native_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    let ipc = temp.path().join("ipc");
    fs::create_dir(&ipc).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let child = ChildOwner(RefCell::new(
        Command::new("python3")
            .arg(repository.join("src/init/mixed_overlap.py"))
            .arg(repository)
            .arg(&root)
            .arg(&ipc)
            .env("PYTHONDONTWRITEBYTECODE", "1")
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
