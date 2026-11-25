//! Package inventory only: never recursively build a package under Cargo's lock.
//! The full legacy regression/oracle gate deliberately remains checkout-only.
use std::{collections::BTreeSet, path::Path, process::Command};

#[test]
fn native_source_package_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: toml::Value = std::fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(manifest["package"]["include"].as_array().is_some());
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--offline",
            "--locked",
            "--allow-dirty",
            "--list",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).unwrap();
    let files: BTreeSet<_> = listing.lines().collect();
    // Frozen evidence replaces every active consumer of these retired helpers.
    for retired in [
        "src/launch/oracle.py",
        "src/launch/executable/native_oracle.py",
        "src/process/identity/tests/oracle.py",
        "src/process/identity/tests/which_oracle.py",
        "src/workspace_assets/oracle.py",
        "src/workspace_assets/init_oracle.py",
        "tests/_checkpoint_parity.py",
    ] {
        assert!(
            !root.join(retired).exists(),
            "retired helper restored: {retired}"
        );
        assert!(
            !files.contains(retired),
            "retired helper packaged: {retired}"
        );
    }
    for file in &files {
        assert!(
            !file.starts_with("tools/"),
            "development harness shipped: {file}"
        );
        assert!(
            !file.split('/').any(|part| matches!(
                part,
                "runner" | "target" | ".git" | "__pycache__" | ".env" | ".ssh"
            )) && !file.ends_with(".pyc")
                && !file.ends_with(".pem")
                && !file.ends_with(".key"),
            "unexpected package payload: {file}"
        );
        assert!(
            !file.starts_with("tests/")
                || file.starts_with("tests/retirement/fixtures/")
                || file.starts_with("tests/retirement/reviews/")
                || *file == "tests/support/interactive_session_server.rs"
                || *file == "tests/support/session_prompt_fixture.rs"
                || *file == "tests/fixtures/copilot_hooks/kitsune-agent-state.sh"
                || *file == "tests/fixtures/copilot_hooks/KITSUNE_LICENSE"
                || *file == "tests/fixtures/pi_extension/kitsune-agent-state.ts"
                || *file == "tests/fixtures/pi_extension/KITSUNE_LICENSE"
                || *file == "tests/fixtures/opencode_extension/kitsune-agent-state.js"
                || *file == "tests/fixtures/opencode_extension/KITSUNE_LICENSE"
                || *file == "tests/fixtures/kitsune_process_policy/reference.json"
                || *file == "tests/native_package.rs",
            "checkout-only regression harness shipped: {file}"
        );
    }
    for required in [
        "src/main.rs",
        "clients/web/index.html",
        "clients/web/app.js",
        "clients/web/styles.css",
        "clients/web/service-worker.js",
        "build.rs",
        "src/hook_files/platform.rs",
        "src/private_access.rs",
        "src/agent_management/files.rs",
        "src/storage/delivery.rs",
        "src/hook_files/platform/linux.rs",
        "src/hook_files/platform/macos.rs",
        "src/hook_files/platform/macos_acl.c",
        "src/hook_files/platform/macos_acl_mock.c",
        "src/hook_files/platform/macos_acl_mock.h",
        "src/lib.rs",
        "src/session_resume_plans.rs",
        // Cargo omits whole nested packages even with explicit include patterns.
        "src/group_publisher/engine.rs",
        "src/group_publisher/live.rs",
        "src/group_publisher/model.rs",
        "src/group_publisher/native.rs",
        "src/group_publisher/tests.rs",
        "src/group_publisher/live_tests.rs",
        "src/group_publisher/native_tests.rs",
        "src/group_publisher/oracle.py",
        "src/group_publisher/counter_oracle.py",
        "src/workspace_assets/role.txt",
        "src/workspace_assets/starter.txt",
        "src/workspace_assets/fixture.rs",
        "src/execution/inspection/projection/vectors.json",
        "examples/interactive-demo/example.json",
        "examples/interactive-demo/tasks.json",
        "examples/interactive-demo/goal.md",
        "tests/retirement/fixtures/records.json",
        "tests/retirement/fixtures/checkpoints.json",
        "tests/retirement/fixtures/task_graphs.json",
        "tests/retirement/fixtures/launch.json",
        "tests/retirement/fixtures/launch_provenance.json",
        "tests/retirement/fixtures/launch_fallback.json",
        "tests/retirement/fixtures/process_identity.json",
        "tests/retirement/fixtures/process_identity_provenance.json",
        "tests/retirement/reviews/process_identity_oracles.json",
        "tests/retirement/reviews/launch_fallback_oracle.json",
        "tests/retirement/fixtures/presentation.json",
        "tests/retirement/fixtures/presentation_provenance.json",
        "tests/retirement/fixtures/workspace_assets.json",
        "tests/retirement/fixtures/workspace_init.json",
        "tests/retirement/fixtures/workspace_assets_provenance.json",
        "tests/retirement/fixtures/checkpoint_inspection.json",
        "tests/retirement/fixtures/checkpoint_inspection_provenance.json",
        "src/presentation_test_fixture.rs",
        "src/launch/retirement.rs",
        "src/launch/executable/fixture.rs",
        "src/launch/executable/fallback.rs",
        "src/process/identity/tests/frozen.rs",
        "src/groups/retirement.rs",
        "src/handoff/retirement.rs",
        "tests/retirement/reviews/task_graphs.json",
        "tests/retirement/reviews/workspace_assets_oracles.json",
        "tests/retirement/reviews/checkpoint_inspection_oracle.json",
        "tests/support/interactive_session_server.rs",
        "tests/support/session_prompt_fixture.rs",
        "src/copilot_hooks.rs",
        "src/pi_extension.rs",
        "src/pi_extension/template.js",
        "src/extension_assets.rs",
        "src/agent_discovery.rs",
        "src/agent_discovery_watch.rs",
        "src/agent_processes.rs",
        "tests/fixtures/kitsune_process_policy/reference.json",
        "src/opencode_extension.rs",
        "src/opencode_extension/template.js",
        "tests/fixtures/opencode_extension/kitsune-agent-state.js",
        "tests/fixtures/opencode_extension/KITSUNE_LICENSE",
        "tests/fixtures/pi_extension/kitsune-agent-state.ts",
        "tests/fixtures/pi_extension/KITSUNE_LICENSE",
        "tests/fixtures/copilot_hooks/kitsune-agent-state.sh",
        "tests/fixtures/copilot_hooks/KITSUNE_LICENSE",
    ] {
        assert!(
            files.contains(required),
            "missing package input: {required}"
        );
    }
    assert_eq!(
        files
            .iter()
            .filter(|file| file.starts_with("clients/"))
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "clients/web/index.html",
            "clients/web/app.js",
            "clients/web/styles.css",
            "clients/web/service-worker.js"
        ]),
        "only the Coven Web runtime assets belong in the source package"
    );
}

#[test]
fn native_help_matches_candidate_formula() {
    let output = Command::new(env!("CARGO_BIN_EXE_coven"))
        .arg("--help")
        .env_clear()
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Manage multi-agent coven workspaces"));
    assert!(!help.contains("Python coven remains the default"));
}
