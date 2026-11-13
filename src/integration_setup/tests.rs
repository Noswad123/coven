use super::*;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

#[cfg(target_os = "linux")]
#[path = "tests/acl.rs"]
mod acl;
#[path = "tests/ownership.rs"]
mod ownership;

thread_local! { static FAIL_AT: Cell<Option<&'static str>> = const { Cell::new(None) }; }

pub(super) fn checkpoint(stage: &str) -> Result<()> {
    FAIL_AT.with(|fault| {
        if fault.get() == Some(stage) {
            fault.set(None);
            Err(Error("integration_setup_injected_interruption"))
        } else {
            Ok(())
        }
    })
}

struct Fixture {
    dir: tempfile::TempDir,
    request: Request,
}

impl Fixture {
    fn new() -> Self {
        let dir = crate::hook_files::test_support::tempdir();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let request = Request {
            operation: Operation::Enable,
            kitsune_config: Some(dir.path().join("config.toml")),
            executable: Some(std::env::current_exe().unwrap()),
            // A nonexistent root is only validated, never created by setup.
            state_root: Some(dir.path().join("s")),
            apply: true,
        };
        Self { dir, request }
    }

    fn config(&self) -> &Path {
        self.request.kitsune_config.as_deref().unwrap()
    }
    fn write(&self, bytes: &[u8], mode: u32) {
        fs::write(self.config(), bytes).unwrap();
        fs::set_permissions(self.config(), fs::Permissions::from_mode(mode)).unwrap();
    }
    fn run(&self) -> Result<serde_json::Value> {
        arguments(&self.request, true).map(|o| serde_json::from_str(&o.stdout).unwrap())
    }
    fn root(&self) -> Root {
        Root::open(self.config().parent().unwrap(), Spec::integration_setup()).unwrap()
    }
    fn receipt(&self) -> Receipt {
        serde_json::from_slice(&fs::read(self.config().parent().unwrap().join(RECEIPT)).unwrap())
            .unwrap()
    }
    fn set_stage(&self, stage: Stage) {
        let root = self.root();
        let mut image = root.read(RECEIPT, JOURNAL_LIMIT).unwrap();
        let mut receipt = self.receipt();
        receipt.stage = stage;
        journal(&root, &mut image, &receipt).unwrap();
    }
}

#[test]
fn preserves_comments_sources_unknown_settings_and_original_mode() {
    let mut f = Fixture::new();
    let original = b"# user configuration\nother = 3 # keep\n\n[integrations]\n# order is deliberate\nsources = [\"Build.A\", 'coven', \"aux_2\"] # sources comment\nunknown = \"untouched\"\n\n[unrelated]\nx = true\n";
    f.write(original, 0o640);
    let result = f.run().unwrap();
    assert_eq!(result["applied"], true);
    let bytes = fs::read(f.config()).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("# user configuration\nother = 3 # keep"));
    assert!(text.contains("sources = [\"Build.A\", 'coven', \"aux_2\"] # sources comment"));
    assert!(text.contains("unknown = \"untouched\""));
    assert!(text.contains("[unrelated]\nx = true"));
    let doc = text.parse::<DocumentMut>().unwrap();
    let runtime = &doc["integrations"]["runtime"];
    let expected = result["command"].as_array().unwrap();
    assert_eq!(
        runtime["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        runtime["report_command"]
            .as_array()
            .unwrap()
            .get(3)
            .unwrap()
            .as_str(),
        Some("--from-kitsune")
    );
    assert_eq!(runtime["external_agent_management"].as_bool(), Some(true));
    assert_eq!(runtime["source"].as_str(), Some("coven"));
    assert_eq!(fs::metadata(f.config()).unwrap().mode() & 0o7777, 0o640);
    for leaf in [RECEIPT, LOCK] {
        assert_eq!(
            fs::metadata(f.dir.path().join(leaf)).unwrap().mode() & 0o7777,
            0o600
        );
    }
    assert_eq!(f.receipt().preimage.unwrap().bytes, original);
    f.request.operation = Operation::Disable;
    f.run().unwrap();
    assert_eq!(fs::read(f.config()).unwrap(), original);
    assert_eq!(fs::metadata(f.config()).unwrap().mode() & 0o7777, 0o640);
    assert_eq!(f.receipt().stage, Stage::Inactive);
}

#[test]
fn adds_coven_once_at_end_and_roundtrips_absent_config() {
    let mut f = Fixture::new();
    f.run().unwrap();
    let receipt = f.receipt();
    assert!(receipt.preimage.is_none());
    let doc = std::str::from_utf8(&receipt.postimage)
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(doc["integrations"]["sources"].as_array().unwrap().len(), 1);
    f.request.operation = Operation::Disable;
    f.run().unwrap();
    assert!(!f.config().exists());
    assert_eq!(f.receipt().stage, Stage::Inactive);
    f.request.operation = Operation::Enable;
    f.run().unwrap();
    assert_eq!(fs::read(f.config()).unwrap(), receipt.postimage);
    let output = generated(
        b"[integrations]\nsources = ['first', 'Second.2'] # keep\n",
        f.request.executable.as_deref().unwrap(),
        f.request.state_root.as_deref().unwrap(),
    )
    .unwrap();
    let doc = std::str::from_utf8(&output)
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(
        doc["integrations"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "Second.2", "coven"]
    );
}

#[test]
fn idempotent_enable_and_disable_do_not_rewrite_files_or_modes() {
    let mut f = Fixture::new();
    f.write(b"# before\nx = 1\n", 0o644);
    f.run().unwrap();
    for operation in [Operation::Enable, Operation::Disable] {
        f.request.operation = operation;
        if operation == Operation::Disable {
            f.run().unwrap();
        }
        let before = fs::metadata(f.config()).unwrap();
        let receipt = fs::metadata(f.dir.path().join(RECEIPT)).unwrap();
        assert_eq!(f.run().unwrap()["noop"], true);
        let after = fs::metadata(f.config()).unwrap();
        assert_eq!(
            (before.ino(), before.mode(), before.mtime_nsec()),
            (after.ino(), after.mode(), after.mtime_nsec())
        );
        assert_eq!(
            receipt.ino(),
            fs::metadata(f.dir.path().join(RECEIPT)).unwrap().ino()
        );
    }
}

#[test]
fn plan_and_noapply_have_no_footprint_even_with_missing_parents() {
    let mut f = Fixture::new();
    f.request.kitsune_config = Some(f.dir.path().join("missing/kit/config.toml"));
    for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
        for apply in [false, true] {
            if apply && operation != Operation::Plan {
                continue;
            }
            f.request.operation = operation;
            f.request.apply = apply;
            assert_eq!(f.run().unwrap()["applied"], false);
            assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), 0);
        }
    }
    f.request.operation = Operation::Disable;
    f.request.apply = true;
    assert_eq!(f.run().unwrap()["noop"], true);
    assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), 0);
}

#[test]
fn apply_creates_only_missing_private_parents_without_chmod_of_ancestor() {
    let mut f = Fixture::new();
    fs::set_permissions(f.dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    f.request.kitsune_config = Some(f.dir.path().join("missing/kit/config.toml"));
    f.run().unwrap();
    assert_eq!(fs::metadata(f.dir.path()).unwrap().mode() & 0o7777, 0o755);
    for dir in ["missing", "missing/kit"] {
        assert_eq!(
            fs::metadata(f.dir.path().join(dir)).unwrap().mode() & 0o7777,
            0o700
        );
    }
    assert_eq!(fs::metadata(f.config()).unwrap().mode() & 0o7777, 0o600);
    f.request.operation = Operation::Disable;
    f.run().unwrap();
    assert!(!f.config().exists());
    assert_eq!(f.receipt().stage, Stage::Inactive);
}

#[test]
fn malformed_or_incompatible_or_unmanaged_config_refuses_without_lock() {
    for input in [
        "[broken",
        "integrations = 3",
        "integrations = { sources = [] }",
        "[integrations]\nsources = 3",
        "[integrations.runtime]\nsource = 'coven'",
        "[integrations]\nruntime = false",
        "[integrations]\nsources = ['dup', 'dup']",
        "[integrations]\nsources = ['bad space']",
        "[integrations]\nsources = ['']",
    ] {
        let f = Fixture::new();
        f.write(input.as_bytes(), 0o600);
        assert!(f.run().is_err(), "{input}");
        assert_eq!(fs::read(f.config()).unwrap(), input.as_bytes());
        assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn eight_source_capacity_and_id_rules_match_kit() {
    let f = Fixture::new();
    let exe = f.request.executable.as_deref().unwrap();
    let state = f.request.state_root.as_deref().unwrap();
    for input in [
        "[integrations]\nsources = ['a','b','c','d','e','f','g','h']",
        "[integrations]\nsources = ['coven','b','c','d','e','f','g','h','i']",
    ] {
        assert_eq!(
            generated(input.as_bytes(), exe, state).unwrap_err().code(),
            "integration_setup_source_capacity"
        );
    }
    assert!(
        generated(
            b"[integrations]\nsources = ['coven','b','c','d','e','f','g','h']",
            exe,
            state
        )
        .is_ok()
    );
    for id in ["has/slash", "non ascii \u{00e9}", &"x".repeat(65)] {
        let input = format!("[integrations]\nsources = ['{id}']");
        assert!(generated(input.as_bytes(), exe, state).is_err());
    }
}

#[test]
fn later_edits_and_mode_changes_are_never_rolled_back_or_adopted() {
    for mode_change in [false, true] {
        let mut f = Fixture::new();
        f.run().unwrap();
        let expected = if mode_change {
            fs::set_permissions(f.config(), fs::Permissions::from_mode(0o640)).unwrap();
            fs::read(f.config()).unwrap()
        } else {
            let mut bytes = fs::read(f.config()).unwrap();
            bytes.extend_from_slice(b"\n# user edit\n");
            fs::write(f.config(), &bytes).unwrap();
            bytes
        };
        for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
            f.request.operation = operation;
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_config_edited"
            );
            assert_eq!(fs::read(f.config()).unwrap(), expected);
        }
    }
}

#[test]
fn refuses_hardlinks_symlinks_and_unsafe_private_files() {
    for leaf in [CONFIG, RECEIPT, LOCK] {
        for kind in ["hardlink", "symlink", "public"] {
            let f = Fixture::new();
            let target = f.dir.path().join("other");
            fs::write(&target, b"{}").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
            let path = f.dir.path().join(leaf);
            match kind {
                "hardlink" => fs::hard_link(&target, &path).unwrap(),
                "symlink" => symlink(&target, &path).unwrap(),
                _ => {
                    fs::write(&path, b"{}").unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
                }
            }
            assert!(f.run().is_err(), "{leaf} {kind}");
            assert_eq!(fs::read(target).unwrap(), b"{}");
        }
    }
    for leaf in [RECEIPT, LOCK] {
        let mut f = Fixture::new();
        f.run().unwrap();
        fs::set_permissions(f.dir.path().join(leaf), fs::Permissions::from_mode(0o644)).unwrap();
        for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
            f.request.operation = operation;
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_unsafe_private_file"
            );
        }
    }
}

#[test]
fn pending_stages_resume_only_exact_known_pre_or_post_images() {
    for stage in [Stage::PendingEnable, Stage::PendingDisable] {
        for at_post in [false, true] {
            let mut f = Fixture::new();
            f.write(b"# exact original\n", 0o640);
            f.run().unwrap();
            let receipt = f.receipt();
            f.set_stage(stage);
            if !at_post {
                let root = f.root();
                let image = root.read(CONFIG, CONFIG_LIMIT).unwrap();
                let pre = receipt.preimage.as_ref().unwrap();
                root.replace(CONFIG, &image, Some(&pre.bytes), pre.mode)
                    .unwrap();
            }
            f.request.operation = Operation::Plan;
            let journal_before = fs::read(f.dir.path().join(RECEIPT)).unwrap();
            f.run().unwrap();
            assert_eq!(
                fs::read(f.dir.path().join(RECEIPT)).unwrap(),
                journal_before
            );
            f.request.operation = if stage == Stage::PendingEnable {
                Operation::Disable
            } else {
                Operation::Enable
            };
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_pending_operation_required"
            );
            f.request.operation = if stage == Stage::PendingEnable {
                Operation::Enable
            } else {
                Operation::Disable
            };
            f.run().unwrap();
            assert_eq!(
                f.receipt().stage,
                if stage == Stage::PendingEnable {
                    Stage::Active
                } else {
                    Stage::Inactive
                }
            );
            assert_eq!(
                fs::read(f.config()).unwrap(),
                if stage == Stage::PendingEnable {
                    receipt.postimage
                } else {
                    receipt.preimage.unwrap().bytes
                }
            );
        }
    }
}

#[test]
fn pending_absent_preimage_is_recoverable_and_pending_user_edit_is_not() {
    let mut f = Fixture::new();
    f.run().unwrap();
    f.set_stage(Stage::PendingEnable);
    let root = f.root();
    let image = root.read(CONFIG, CONFIG_LIMIT).unwrap();
    root.replace(CONFIG, &image, None, 0o600).unwrap();
    f.run().unwrap();
    assert_eq!(f.receipt().stage, Stage::Active);
    f.set_stage(Stage::PendingDisable);
    f.write(b"# later user edit\n", 0o600);
    f.request.operation = Operation::Disable;
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_config_edited"
    );
    assert_eq!(fs::read(f.config()).unwrap(), b"# later user edit\n");
}

#[test]
fn cross_file_interruptions_leave_durable_intent_and_resume_without_blind_rollback() {
    for point in ["intent", "config"] {
        let mut f = Fixture::new();
        let original = b"# original exact bytes\nx = 1\n";
        f.write(original, 0o640);
        for operation in [Operation::Enable, Operation::Disable] {
            f.request.operation = operation;
            FAIL_AT.with(|fault| fault.set(Some(point)));
            assert_eq!(
                f.run().unwrap_err().code(),
                "integration_setup_injected_interruption"
            );
            let receipt = f.receipt();
            assert_eq!(
                receipt.stage,
                if operation == Operation::Enable {
                    Stage::PendingEnable
                } else {
                    Stage::PendingDisable
                }
            );
            assert_eq!(receipt.preimage.as_ref().unwrap().bytes, original);
            let image = f.root().read(CONFIG, CONFIG_LIMIT).unwrap();
            let expect_post = (operation == Operation::Enable) == (point == "config");
            assert!(if expect_post {
                receipt.post_matches(&image)
            } else {
                receipt.pre_matches(&image)
            });
            f.run().unwrap();
            assert_eq!(
                f.receipt().stage,
                if operation == Operation::Enable {
                    Stage::Active
                } else {
                    Stage::Inactive
                }
            );
        }
        assert_eq!(fs::read(f.config()).unwrap(), original);
    }
}

#[test]
fn malformed_receipts_and_engine_recovery_are_fail_closed() {
    for damage in ["version", "digest", "post", "stage", "extra"] {
        let f = Fixture::new();
        f.run().unwrap();
        let bytes = fs::read(f.config()).unwrap();
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(f.dir.path().join(RECEIPT)).unwrap()).unwrap();
        match damage {
            "version" => receipt["version"] = json!(2),
            "digest" => receipt["post_digest"] = json!("bad"),
            "post" => receipt["postimage"] = json!([]),
            "stage" => receipt["stage"] = json!("unknown"),
            _ => receipt["extra"] = json!(true),
        }
        fs::write(
            f.dir.path().join(RECEIPT),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        assert!(f.run().is_err());
        assert_eq!(fs::read(f.config()).unwrap(), bytes);
    }
    let f = Fixture::new();
    fs::create_dir(f.dir.path().join(".coven-kit-integration-recovery")).unwrap();
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_recovery_required"
    );
    assert!(!f.dir.path().join(LOCK).exists());
}

#[test]
fn spec_is_a_fixed_allowlist_not_an_arbitrary_writer() {
    let f = Fixture::new();
    let root = f.root();
    for name in [
        "other.toml",
        "../config.toml",
        "/config.toml",
        "sub/config.toml",
    ] {
        assert!(root.read(name, CONFIG_LIMIT).is_err());
        assert!(root.replace(name, &None, Some(b"bad"), 0o600).is_err());
    }
    assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), 0);
}

#[test]
fn validates_paths_executable_socket_budget_and_development_warning() {
    let mut f = Fixture::new();
    f.request.apply = false;
    for path in [
        "relative/config.toml",
        "/home/../config.toml",
        "/home/./config.toml",
        "/home//config.toml",
        "/home/wrong.toml",
    ] {
        f.request.kitsune_config = Some(PathBuf::from(path));
        assert!(f.run().is_err());
    }
    f.request.kitsune_config = Some(f.dir.path().join("config.toml"));
    let alias = f.dir.path().join("alias");
    symlink(f.dir.path(), &alias).unwrap();
    f.request.kitsune_config = Some(alias.join("config.toml"));
    assert!(f.run().is_err());
    f.request.kitsune_config = Some(f.dir.path().join("config.toml"));
    f.request.executable = Some(PathBuf::from("coven"));
    assert!(f.run().is_err());
    f.request.executable = Some(f.dir.path().join("not-executable"));
    fs::write(f.request.executable.as_ref().unwrap(), b"not a program").unwrap();
    fs::set_permissions(
        f.request.executable.as_ref().unwrap(),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(f.run().is_err());
    f.request.executable = Some(std::env::current_exe().unwrap());
    f.request.state_root = Some(PathBuf::from(format!("/{}", "x".repeat(120))));
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_socket_path_too_long"
    );
    f.request.state_root = Some(f.dir.path().join("s"));
    assert_eq!(f.run().unwrap()["provider_setup_required"], true);
    assert_eq!(f.run().unwrap()["live_api_used"], false);
}

#[test]
fn previews_existing_config_preserve_bytes_modes_and_directory_entries() {
    let mut f = Fixture::new();
    f.write(b"# user settings\nx = 1\n", 0o644);
    f.request.apply = false;
    for managed in [false, true] {
        if managed {
            f.request.operation = Operation::Enable;
            f.request.apply = true;
            f.run().unwrap();
            f.request.apply = false;
        }
        let before = fs::read(f.config()).unwrap();
        let metadata = fs::metadata(f.config()).unwrap();
        let entries = fs::read_dir(f.dir.path()).unwrap().count();
        let receipt = fs::read(f.dir.path().join(RECEIPT)).ok();
        for operation in [Operation::Plan, Operation::Enable, Operation::Disable] {
            f.request.operation = operation;
            f.run().unwrap();
            assert_eq!(fs::read(f.config()).unwrap(), before);
            assert_eq!(fs::metadata(f.config()).unwrap().ino(), metadata.ino());
            assert_eq!(fs::metadata(f.config()).unwrap().mode(), metadata.mode());
            assert_eq!(fs::read_dir(f.dir.path()).unwrap().count(), entries);
            assert_eq!(fs::read(f.dir.path().join(RECEIPT)).ok(), receipt);
        }
    }
}

#[test]
fn lock_contention_and_changed_spec_refuse_without_config_mutation() {
    let mut f = Fixture::new();
    f.write(b"# before\n", 0o600);
    let root = f.root();
    let lock = root.lock(LOCK).unwrap();
    assert_eq!(f.run().unwrap_err().code(), "integration_setup_locked");
    assert_eq!(fs::read(f.config()).unwrap(), b"# before\n");
    assert!(!f.dir.path().join(RECEIPT).exists());
    drop(lock);
    f.run().unwrap();
    let post = fs::read(f.config()).unwrap();
    f.request.state_root = Some(f.dir.path().join("other"));
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_spec_conflict"
    );
    assert_eq!(fs::read(f.config()).unwrap(), post);
    // Explicit pins cannot silently retarget even an undo operation.
    f.request.operation = Operation::Disable;
    assert_eq!(
        f.run().unwrap_err().code(),
        "integration_setup_spec_conflict"
    );
    f.request.state_root = None;
    f.run().unwrap();
    assert_eq!(fs::read(f.config()).unwrap(), b"# before\n");
}

#[test]
fn socket_preflight_matches_runtime_layout_at_exact_boundary() {
    let f = Fixture::new();
    let capacity = unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
        .sun_path
        .len();
    // /i/ + 24 hex + /r.sock consumes 34 bytes after ROOT.
    let base_len = f.dir.path().as_os_str().len();
    let accepted = f.dir.path().join("x".repeat(capacity - base_len - 36));
    let refused = f.dir.path().join("x".repeat(capacity - base_len - 35));
    paths::state_root(&accepted).unwrap();
    assert_eq!(
        paths::state_root(&refused).unwrap_err().code(),
        "integration_setup_socket_path_too_long"
    );
}

#[test]
fn config_defaults_child() {
    let Some(expected) = std::env::var_os("COVEN_SETUP_EXPECT_CONFIG") else {
        return;
    };
    if expected == "error" {
        assert!(paths::config(None).is_err());
        assert!(crate::integration_runtime::default_state_root().is_err());
    } else {
        assert_eq!(paths::config(None).unwrap(), PathBuf::from(expected));
    }
    assert_eq!(
        paths::config(Some(Path::new("/explicit/config.toml"))).unwrap(),
        PathBuf::from("/explicit/config.toml")
    );
}

#[test]
fn defaults_follow_kit_precedence_without_temporary_fallback() {
    for (kit, xdg, home, expected) in [
        (
            Some("/selected/config.toml"),
            Some("/xdg"),
            Some("/home/test"),
            "/selected/config.toml",
        ),
        (
            None,
            Some("/xdg"),
            Some("/home/test"),
            "/xdg/kitsune/config.toml",
        ),
        (
            None,
            None,
            Some("/home/test"),
            "/home/test/.config/kitsune/config.toml",
        ),
        (None, None, None, "error"),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("COVEN_SETUP_EXPECT_CONFIG", expected)
            .args([
                "--exact",
                "integration_setup::tests::config_defaults_child",
                "--nocapture",
            ]);
        for (key, value) in [
            ("KITSUNE_CONFIG_PATH", kit),
            ("XDG_CONFIG_HOME", xdg),
            ("HOME", home),
        ] {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
