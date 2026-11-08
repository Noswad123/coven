//! Checkout launcher tests use real prebuilt binaries, never a Python oracle.
#![cfg(unix)]

use std::{
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn wrapper(root: &Path) -> PathBuf {
    let path = root.join("bin/coven");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/coven"),
        &path,
    )
    .unwrap();
    path
}

fn binary(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_coven"), path).unwrap();
}

fn cli(wrapper: &Path, root: &Path) -> Command {
    let mut command = Command::new(wrapper);
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", "");
    command
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn failure(output: Output, message: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
    assert!(
        stderr.contains("cargo build --release --bin coven"),
        "{stderr}"
    );
}

fn traps(root: &Path) -> PathBuf {
    let path = root.join("traps");
    fs::create_dir(&path).unwrap();
    for name in [
        "cargo", "python", "python3", "coven", "dirname", "readlink", "bash",
    ] {
        let tool = path.join(name);
        fs::write(
            &tool,
            "#!/bin/sh\nprintf 'unexpected runtime call\\n' >> \"$HOME/runtime-calls\"\nexit 99\n",
        )
        .unwrap();
        fs::set_permissions(tool, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let runner = root.join("runner/coven.py");
    fs::create_dir_all(runner.parent().unwrap()).unwrap();
    fs::copy(path.join("python"), runner).unwrap();
    path
}

#[test]
fn default_is_only_prebuilt_release_and_direct_shebang_needs_no_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    let traps = traps(root);
    binary(&root.join("target/debug/coven"));
    for path in ["", traps.to_str().unwrap()] {
        failure(
            cli(&wrapper, root)
                .env("PATH", path)
                .arg("--help")
                .output()
                .unwrap(),
            "Missing or non-executable native coven",
        );
    }
    let release = root.join("target/release/coven");
    binary(&release);
    success(cli(&wrapper, root).arg("--help").output().unwrap());
    success(
        cli(&wrapper, root)
            .env("PATH", &traps)
            .arg("--help")
            .output()
            .unwrap(),
    );
    fs::set_permissions(&release, fs::Permissions::from_mode(0o644)).unwrap();
    failure(
        cli(&wrapper, root)
            .env("PATH", traps)
            .arg("--help")
            .output()
            .unwrap(),
        "Missing or non-executable native coven",
    );
    assert!(!root.join("runtime-calls").exists());
}

#[test]
fn explicit_absolute_selection_works_relocated_and_through_a_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    let native = root.join("prebuilt with spaces/native coven");
    binary(&native);
    let relocated = root.join("relocated launcher");
    fs::rename(&wrapper, &relocated).unwrap();
    let link = root.join("linked launcher");
    symlink(&relocated, &link).unwrap();
    for launcher in [&relocated, &link] {
        success(
            cli(launcher, root)
                .env("COVEN_NATIVE_BIN", &native)
                .arg("--help")
                .output()
                .unwrap(),
        );
        let direct = cli(&native, root).arg("not-a-command").output().unwrap();
        let wrapped = cli(launcher, root)
            .env("COVEN_NATIVE_BIN", &native)
            .arg("not-a-command")
            .output()
            .unwrap();
        assert_eq!(wrapped.status.code(), direct.status.code());
        assert!(!wrapped.status.success());
        assert_eq!(wrapped.stderr, direct.stderr);
    }
}

#[test]
fn bad_selections_and_same_inode_recursion_fail_without_runtime_calls() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    let traps = traps(root);
    binary(&root.join("target/release/coven"));
    for selected in ["", "coven", "target/release/coven"] {
        failure(
            cli(&wrapper, root)
                .env("PATH", &traps)
                .env("COVEN_NATIVE_BIN", selected)
                .output()
                .unwrap(),
            "must be an absolute path",
        );
    }
    let non_executable = root.join("not executable");
    fs::write(&non_executable, "not a binary").unwrap();
    for selected in [root.join("missing"), non_executable, root.to_path_buf()] {
        failure(
            cli(&wrapper, root)
                .env("PATH", &traps)
                .env("COVEN_NATIVE_BIN", selected)
                .output()
                .unwrap(),
            "Missing or non-executable native coven",
        );
    }
    let link = root.join("self link");
    symlink(&wrapper, &link).unwrap();
    let hard_link = root.join("self hard link");
    fs::hard_link(&wrapper, &hard_link).unwrap();
    for selected in [&wrapper, &link, &hard_link] {
        failure(
            cli(&wrapper, root)
                .env("PATH", &traps)
                .env("COVEN_NATIVE_BIN", selected)
                .output()
                .unwrap(),
            "recursive invocation",
        );
    }
    fs::remove_file(root.join("target/release/coven")).unwrap();
    symlink(&wrapper, root.join("target/release/coven")).unwrap();
    failure(
        cli(&wrapper, root).env("PATH", traps).output().unwrap(),
        "recursive invocation",
    );
    assert!(!root.join("runtime-calls").exists());
}

#[test]
fn native_arguments_command_override_and_asset_home_keep_their_meaning() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    binary(&root.join("target/release/coven"));
    let traps = traps(root);
    let home = root.join("asset home");
    let example = home.join("examples/custom");
    fs::create_dir_all(&example).unwrap();
    fs::write(example.join("example.json"), r#"{"id":"custom"}"#).unwrap();
    fs::write(example.join("goal.md"), "custom asset home goal\n").unwrap();
    // A COVEN_HOME binary is deliberately not a candidate for the launcher.
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::copy(traps.join("python"), home.join("bin/coven")).unwrap();
    let workspace = root.join("workspace with spaces");
    let command = "/chosen command/coven";
    success(
        cli(&wrapper, root)
            .env("PATH", &traps)
            .env("COVEN_HOME", &home)
            .env("COVEN_COMMAND", command)
            .args([
                "init",
                workspace.to_str().unwrap(),
                "--example",
                "custom",
                "--agents",
                "operator",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(workspace.join("goal.md")).unwrap(),
        "custom asset home goal\n"
    );
    let manifest = fs::read_to_string(workspace.join("tmux/coven.toml")).unwrap();
    assert!(
        manifest.contains("'/chosen command/coven' events"),
        "{manifest}"
    );
    let literal = "literal ' \" $HOME $(touch injected) ; * \\ title";
    success(
        cli(&wrapper, root)
            .env("PATH", traps)
            .args(["task", "add", workspace.to_str().unwrap(), literal])
            .output()
            .unwrap(),
    );
    let tasks: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("state/tasks.json")).unwrap()).unwrap();
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["title"] == literal)
    );
    assert!(!root.join("injected").exists());
    assert!(!root.join("runtime-calls").exists());
}

#[test]
fn exec_preserves_pid_and_signal_status_with_a_compiled_inert_helper() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    let helper = root.join("inert native helper");
    // Compile at test setup, never through the launcher or its runtime PATH.
    let source = root.join("helper.rs");
    fs::write(
        &source,
        r#"fn main() {
        println!("{}", std::process::id());
        if std::env::args().nth(1).as_deref() == Some("signal") {
            unsafe extern "C" { fn raise(signal: i32) -> i32; }
            unsafe { raise(15); }
        }
        std::process::exit(37);
    }"#,
    )
    .unwrap();
    success(
        Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&helper)
            .output()
            .unwrap(),
    );
    for signal in [false, true] {
        let mut command = cli(&wrapper, root);
        command
            .env("COVEN_NATIVE_BIN", &helper)
            .stdout(std::process::Stdio::piped());
        if signal {
            command.arg("signal");
        }
        let child = command.spawn().unwrap();
        let pid = child.id();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            pid.to_string()
        );
        if signal {
            assert_eq!(output.status.signal(), Some(15));
        } else {
            assert_eq!(output.status.code(), Some(37));
        }
    }
}

#[test]
fn default_symlink_chain_discovers_release_with_empty_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    binary(&root.join("target/release/coven"));
    let relative = root.join("relative");
    symlink("bin/coven", &relative).unwrap();
    let absolute = root.join("absolute");
    symlink(&relative, &absolute).unwrap();
    for launcher in [&wrapper, &relative, &absolute] {
        success(cli(launcher, root).arg("--help").output().unwrap());
    }
}

#[test]
fn arcana_non_utf8_callback_keeps_domain_errors_and_explicit_precedence() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let native = Path::new(env!("CARGO_BIN_EXE_coven"));
    let workspace = root.join("workspace");
    let invalid = std::ffi::OsString::from_vec(vec![0xff]);
    let output = cli(native, root)
        .env("ARCANA_PUBLIC_COVEN", &invalid)
        .arg("init")
        .arg(&workspace)
        .arg("--blank")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported_init_command_environment_profile")
    );
    success(
        cli(native, root)
            .env("ARCANA_PUBLIC_COVEN", &invalid)
            .env("COVEN_COMMAND", "coven")
            .arg("init")
            .arg(&workspace)
            .arg("--blank")
            .output()
            .unwrap(),
    );
    let output = cli(native, root)
        .env("COVEN_COMMAND", "")
        .env("ARCANA_PUBLIC_COVEN", invalid)
        .arg("refresh")
        .arg(&workspace)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported_refresh_command_environment_profile")
    );
}

#[test]
fn explicit_inert_target_receives_unchanged_callback_and_asset_environment() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let wrapper = wrapper(root);
    // Use an existing prebuilt program: a newly written script can remain
    // temporarily write-open in another parallel test's fork (ETXTBSY).
    let helper = Path::new("/bin/sh");
    success(
        cli(&wrapper, root)
            .env("COVEN_NATIVE_BIN", helper)
            .env("COVEN_COMMAND", "")
            .env("ARCANA_PUBLIC_COVEN", "/public/../chosen coven")
            .env("COVEN_HOME", "/asset root")
            .args([
                "-c",
                "[ \"$COVEN_COMMAND\" = '' ] && [ \"$ARCANA_PUBLIC_COVEN\" = '/public/../chosen coven' ] && [ \"$COVEN_HOME\" = '/asset root' ]",
            ])
            .output()
            .unwrap(),
    );
}
