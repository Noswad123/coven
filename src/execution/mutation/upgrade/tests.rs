use super::*;
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

const ID: &str = "dddddddddddddddddddddddddddddddd";
struct Fixed<'a>(&'a str);
impl super::super::lifecycle::Metadata for Fixed<'_> {
    fn uuid(&self) -> Result<String> {
        Ok(self.0.to_owned())
    }
}

fn fixture() -> (tempfile::TempDir, Value, Vec<u8>, WorkspaceGuard) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("state/execution")).unwrap();
    let control = super::super::super::tests::enabled();
    let mut raw = b" \n\t".to_vec();
    raw.extend(control.canonical().unwrap());
    raw.extend(b" \r\n\n");
    fs::write(dir.path().join(CONTROL), &raw).unwrap();
    // Neither planning nor backup publication may admit these current sources.
    for relative in [
        "coven.json",
        "state/tasks.json",
        "state/team.json",
        "state/checkpoint.json",
    ] {
        fs::write(dir.path().join(relative), b"broken current source").unwrap();
    }
    let guard = WorkspaceGuard::execution_maintenance(dir.path(), Duration::ZERO).unwrap();
    (dir, control, raw, guard)
}
fn planned(root: &Path, control: &Value) -> Plan {
    plan(
        root,
        control,
        &validated(control).unwrap(),
        &Fixed(ID),
        &mut Work::default(),
    )
    .unwrap()
}
fn equal(left: &Value, right: &Value) {
    assert_eq!(left.canonical().unwrap(), right.canonical().unwrap());
}

#[derive(Debug, PartialEq, Eq)]
struct Entry(PathBuf, Option<Vec<u8>>, u32);
fn inventory(root: &Path) -> Vec<Entry> {
    fn walk(root: &Path, relative: &Path, out: &mut Vec<Entry>) {
        let path = root.join(relative);
        let meta = fs::symlink_metadata(&path).unwrap();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = 0;
        out.push(Entry(
            relative.to_owned(),
            meta.is_file().then(|| fs::read(&path).unwrap()),
            mode,
        ));
        if meta.is_dir() {
            let mut children = fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                walk(root, &relative.join(child), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out);
    out
}

#[test]
fn read_only_plan_preserves_complete_legacy_and_exact_metadata() {
    let (dir, control, raw, _guard) = fixture();
    let before = inventory(dir.path());
    let plan = planned(dir.path(), &control);
    assert_eq!(inventory(dir.path()), before);
    equal(get(&plan.control, "legacy_v1").unwrap(), &control);
    for key in [
        "generation",
        "execution_authority_id",
        "snapshot_source_id",
        "extensions",
    ] {
        equal(
            get(&plan.control, key).unwrap(),
            get(&control, key).unwrap(),
        );
    }
    assert!(
        get(&plan.control, "history")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(exact_integer(
        get(&plan.control, "schema_version").unwrap(),
        2
    ));
    fields(
        &plan.effect,
        "upgrade_id source_revision source_sha256 source_mode backup_path",
    )
    .unwrap();
    equal(get(&plan.control, "upgrade").unwrap(), &plan.effect);
    assert_eq!(
        text(get(&plan.effect, "source_sha256").unwrap()).unwrap(),
        canonical::digest(&raw)
    );
    assert_eq!(plan.raw, raw);
    assert_ne!(
        canonical::digest(&raw),
        canonical::digest(&control.canonical().unwrap())
    );
}

#[test]
fn refuses_changed_extensions_invalid_uuid_oversize_and_wrong_selected_view_without_artifacts() {
    let (dir, mut control, raw, _guard) = fixture();
    let before = inventory(dir.path());
    let selected = validated(&control).unwrap();
    for id in ["../escape", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "", "d"] {
        assert!(
            plan(
                dir.path(),
                &control,
                &selected,
                &Fixed(id),
                &mut Work::default()
            )
            .is_err()
        );
    }
    assert_eq!(inventory(dir.path()), before);
    control
        .set_field(
            "extensions",
            object(vec![("unknown", string("changed"))]).unwrap(),
        )
        .unwrap();
    assert!(
        plan(
            dir.path(),
            &control,
            &selected,
            &Fixed(ID),
            &mut Work::default()
        )
        .is_err()
    );
    fs::write(dir.path().join(CONTROL), control.canonical().unwrap()).unwrap();
    assert!(
        plan(
            dir.path(),
            &control,
            &selected,
            &Fixed(ID),
            &mut Work::default()
        )
        .is_err()
    );
    let mut oversized = raw.clone();
    oversized.resize(values::MAX_INPUT + 1, b' ');
    fs::write(dir.path().join(CONTROL), oversized).unwrap();
    assert!(
        plan(
            dir.path(),
            &control,
            &selected,
            &Fixed(ID),
            &mut Work::default()
        )
        .is_err()
    );
    assert!(!dir.path().join(UPGRADES).exists());
}

#[test]
fn publish_requires_matching_guard_and_unchanged_raw_authority_before_any_backup_io() {
    let (dir, control, raw, guard) = fixture();
    let (other, _, _, other_guard) = fixture();
    let plan = planned(dir.path(), &control);
    let before = inventory(dir.path());
    assert!(plan.publish(dir.path(), &other_guard).is_err());
    assert!(plan.publish(other.path(), &guard).is_err());
    assert!(plan.publish(other.path(), &other_guard).is_err());
    assert_eq!(inventory(dir.path()), before);
    fs::write(dir.path().join(CONTROL), control.canonical().unwrap()).unwrap();
    assert!(plan.publish(dir.path(), &guard).is_err());
    assert!(!dir.path().join(UPGRADES).exists());
    fs::write(dir.path().join(CONTROL), raw).unwrap();
    let file = platform::open_readonly(&dir.path().join(CONTROL)).unwrap();
    platform::apply_replacement_mode(&file, plan.mode ^ 0o100).unwrap();
    assert!(plan.publish(dir.path(), &guard).is_err());
    assert!(!dir.path().join(UPGRADES).exists());
}

#[test]
fn verified_backup_preserves_raw_special_modes_and_never_writes_authority_or_adopts_collision() {
    for mode in [0o640, 0o1740, 0o2740, 0o4740, 0o7740] {
        let (dir, control, raw, guard) = fixture();
        let source = platform::open_readonly(&dir.path().join(CONTROL)).unwrap();
        platform::apply_replacement_mode(&source, mode).unwrap();
        let plan = planned(dir.path(), &control);
        plan.publish(dir.path(), &guard).unwrap();
        let backup = dir.path().join(&plan.backup);
        assert_eq!(
            read_fs::read_regular(&backup, values::MAX_INPUT).unwrap(),
            raw
        );
        assert_eq!(platform::replacement_mode(&backup).unwrap(), Some(mode));
        assert_eq!(fs::read(dir.path().join(CONTROL)).unwrap(), raw);
        assert_eq!(
            platform::replacement_mode(&dir.path().join(CONTROL)).unwrap(),
            Some(mode)
        );
        let before = inventory(dir.path());
        assert!(plan.publish(dir.path(), &guard).is_err());
        assert_eq!(inventory(dir.path()), before);
    }
}

#[derive(Default)]
struct Fault {
    fail: usize,
    calls: RefCell<Vec<&'static str>>,
    verifies: Cell<usize>,
    partial: bool,
    corrupt_bytes: bool,
    corrupt_mode: bool,
    directories: RefCell<Vec<PathBuf>>,
}
impl Fault {
    fn step(&self, name: &'static str) -> Result<()> {
        let mut calls = self.calls.borrow_mut();
        calls.push(name);
        require(calls.len() != self.fail, "injected_upgrade_failure")
    }
}
impl BackupIo for Fault {
    fn mkdir(&self, path: &Path) -> Result<()> {
        self.step("mkdir")?;
        SystemBackupIo.mkdir(path)
    }
    fn create(&self, path: &Path) -> Result<File> {
        self.step("create")?;
        SystemBackupIo.create(path)
    }
    fn write(&self, file: &mut File, raw: &[u8]) -> Result<()> {
        if self.partial {
            file.write_all(&raw[..raw.len() / 2])?;
            return Err(Error("injected_partial_write"));
        }
        self.step("write")?;
        SystemBackupIo.write(file, raw)
    }
    fn apply_mode(&self, file: &File, mode: u32) -> Result<()> {
        self.step("apply")?;
        SystemBackupIo.apply_mode(file, mode)
    }
    fn verify_mode(&self, file: &File, mode: u32) -> Result<()> {
        self.verifies.set(self.verifies.get() + 1);
        self.step("verify")?;
        if self.corrupt_mode && self.verifies.get() == 2 {
            platform::apply_replacement_mode(file, mode ^ 0o100)?;
        }
        SystemBackupIo.verify_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> Result<()> {
        self.step("fsync")?;
        SystemBackupIo.sync_file(file)
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.step("read")?;
        let mut bytes = SystemBackupIo.read(path)?;
        if self.corrupt_bytes {
            bytes.push(0);
        }
        Ok(bytes)
    }
    fn reopen(&self, path: &Path) -> Result<File> {
        self.step("reopen")?;
        SystemBackupIo.reopen(path)
    }
    fn sync_dir(&self, path: &Path) -> Result<()> {
        self.step("dirsync")?;
        self.directories.borrow_mut().push(path.to_owned());
        SystemBackupIo.sync_dir(path)
    }
}

#[test]
fn fault_matrix_retains_only_inert_artifacts_and_preserves_every_original_byte_and_mode() {
    let expected = [
        "mkdir", "mkdir", "create", "write", "apply", "verify", "fsync", "read", "reopen",
        "verify", "dirsync", "dirsync", "dirsync", "dirsync", "dirsync",
    ];
    for failure in 1..=expected.len() + 4 {
        let (dir, control, raw, guard) = fixture();
        let before = inventory(dir.path());
        let plan = planned(dir.path(), &control);
        let fault = Fault {
            fail: if failure <= expected.len() {
                failure
            } else {
                0
            },
            partial: failure == expected.len() + 1,
            corrupt_bytes: failure == expected.len() + 2,
            corrupt_mode: failure == expected.len() + 3,
            ..Fault::default()
        };
        let result = plan.publish_with(dir.path(), &guard, &fault);
        if failure == expected.len() + 4 {
            result.unwrap();
            assert_eq!(*fault.calls.borrow(), expected);
            assert_eq!(
                *fault.directories.borrow(),
                [
                    dir.path().join(plan.backup.parent().unwrap()),
                    dir.path().join(UPGRADES),
                    dir.path().join("state/execution"),
                    dir.path().join("state"),
                    dir.path().to_owned(),
                ]
            );
        } else {
            assert_eq!(
                result.unwrap_err().code(),
                "execution_upgrade_not_published_inert_backup_retained"
            );
        }
        let after = inventory(dir.path());
        for entry in &before {
            assert!(
                after.contains(entry),
                "changed original {entry:?}, fault {failure}"
            );
        }
        for entry in after
            .iter()
            .filter(|e| !before.iter().any(|old| old.0 == e.0))
        {
            assert!(entry.0.starts_with(UPGRADES));
        }
        assert_eq!(fs::read(dir.path().join(CONTROL)).unwrap(), raw);
        validated(&control).unwrap();
        if fault.partial {
            assert_eq!(
                fs::read(dir.path().join(&plan.backup)).unwrap(),
                raw[..raw.len() / 2]
            );
        }
        if dir.path().join(plan.backup.parent().unwrap()).exists() {
            let inert = inventory(dir.path());
            assert!(plan.publish(dir.path(), &guard).is_err());
            assert_eq!(inventory(dir.path()), inert);
        }
    }
}

#[test]
fn existing_safe_upgrades_directory_is_accepted_but_uuid_collision_is_never_adopted() {
    let (dir, control, _, guard) = fixture();
    fs::create_dir(dir.path().join(UPGRADES)).unwrap();
    let plan = planned(dir.path(), &control);
    plan.publish(dir.path(), &guard).unwrap();
    fs::remove_file(dir.path().join(&plan.backup)).unwrap();
    let before = inventory(dir.path());
    assert!(plan.publish(dir.path(), &guard).is_err());
    assert_eq!(inventory(dir.path()), before);
}

#[test]
fn exclusive_backup_open_never_overwrites_a_colliding_file_or_directory() {
    struct Collision(bool);
    impl BackupIo for Collision {
        fn create(&self, path: &Path) -> Result<File> {
            if self.0 {
                fs::create_dir(path)?;
            } else {
                fs::write(path, b"collision evidence")?;
            }
            SystemBackupIo.create(path)
        }
    }
    for directory in [false, true] {
        let (dir, control, raw, guard) = fixture();
        let plan = planned(dir.path(), &control);
        assert!(
            plan.publish_with(dir.path(), &guard, &Collision(directory))
                .is_err()
        );
        let backup = dir.path().join(&plan.backup);
        if directory {
            assert!(backup.is_dir());
        } else {
            assert_eq!(fs::read(backup).unwrap(), b"collision evidence");
        }
        assert_eq!(fs::read(dir.path().join(CONTROL)).unwrap(), raw);
    }
}

#[test]
fn planning_charges_comparisons_and_refuses_exhausted_work_without_publication() {
    let (dir, control, _, _guard) = fixture();
    let before = inventory(dir.path());
    assert!(
        plan(
            dir.path(),
            &control,
            &validated(&control).unwrap(),
            &Fixed(ID),
            &mut Work(MAX_WORK),
        )
        .is_err()
    );
    assert_eq!(inventory(dir.path()), before);
}

#[test]
fn capture_accepts_exactly_eight_mib_of_original_bytes_without_reencoding() {
    let (dir, control, mut raw, guard) = fixture();
    raw.resize(values::MAX_INPUT, b' ');
    fs::write(dir.path().join(CONTROL), &raw).unwrap();
    let plan = planned(dir.path(), &control);
    assert!(!dir.path().join(UPGRADES).exists());
    plan.publish(dir.path(), &guard).unwrap();
    assert_eq!(fs::read(dir.path().join(&plan.backup)).unwrap(), raw);
}

#[cfg(unix)]
#[test]
fn planning_refuses_control_and_parent_symlinks_and_nonregular_sources() {
    use std::os::unix::fs::symlink;
    for parent in [false, true] {
        let (dir, control, raw, _guard) = fixture();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("control.json"), &raw).unwrap();
        fs::remove_file(dir.path().join(CONTROL)).unwrap();
        if parent {
            fs::remove_dir(dir.path().join("state/execution")).unwrap();
            symlink(external.path(), dir.path().join("state/execution")).unwrap();
        } else {
            symlink(
                external.path().join("control.json"),
                dir.path().join(CONTROL),
            )
            .unwrap();
        }
        assert!(
            plan(
                dir.path(),
                &control,
                &validated(&control).unwrap(),
                &Fixed(ID),
                &mut Work::default()
            )
            .is_err()
        );
        assert_eq!(fs::read(external.path().join("control.json")).unwrap(), raw);
    }
    let (dir, control, _, _guard) = fixture();
    fs::remove_file(dir.path().join(CONTROL)).unwrap();
    fs::create_dir(dir.path().join(CONTROL)).unwrap();
    assert!(
        plan(
            dir.path(),
            &control,
            &validated(&control).unwrap(),
            &Fixed(ID),
            &mut Work::default()
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlink_and_type_collisions_are_refused_without_touching_targets() {
    use std::os::unix::fs::symlink;
    for relative in [UPGRADES.to_owned(), format!("{UPGRADES}/{ID}")] {
        for link in [false, true] {
            let (dir, control, _, guard) = fixture();
            let external = tempfile::tempdir().unwrap();
            fs::write(external.path().join("sentinel"), b"unchanged").unwrap();
            let plan = planned(dir.path(), &control);
            if relative != UPGRADES {
                fs::create_dir(dir.path().join(UPGRADES)).unwrap();
            }
            if link {
                symlink(external.path(), dir.path().join(&relative)).unwrap();
            } else {
                fs::write(dir.path().join(&relative), b"collision").unwrap();
            }
            let before = inventory(dir.path());
            assert!(plan.publish(dir.path(), &guard).is_err());
            assert_eq!(inventory(dir.path()), before);
            assert_eq!(
                fs::read(external.path().join("sentinel")).unwrap(),
                b"unchanged"
            );
        }
    }
}

#[test]
fn python_upgrade_oracle_matches_base_effect_raw_backup_and_mode() {
    use std::process::{Command, Stdio};
    let (dir, control, raw, guard) = fixture();
    let plan = planned(dir.path(), &control);
    let input =
        serde_json::json!({"raw": String::from_utf8(raw.clone()).unwrap(), "mode": plan.mode});
    let mut child = Command::new("python3")
        .args([
            "-I",
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/execution/mutation/upgrade/oracle.py"
            ),
        ])
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle = values::parse(&output.stdout, Profile::Authority).unwrap();
    equal(&plan.control, get(&oracle, "control").unwrap());
    equal(&plan.effect, get(&oracle, "effect").unwrap());
    plan.publish(dir.path(), &guard).unwrap();
    assert_eq!(
        text(get(&oracle, "backup_hex").unwrap()).unwrap(),
        raw.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    equal(
        get(&plan.effect, "source_mode").unwrap(),
        get(&oracle, "backup_mode").unwrap(),
    );
}
