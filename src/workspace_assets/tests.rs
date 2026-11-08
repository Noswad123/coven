use super::*;
use std::process::Command;

#[cfg(unix)]
#[path = "tests/bootstrap.rs"]
mod bootstrap;

// The oracle is test-only, with a deadline and file-backed output so neither
// full pipes nor a failing Python import can hang the test harness.
pub(super) fn run(command: &mut Command) -> Vec<u8> {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut child = command
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .spawn()
        .expect("asset test subprocess must be available");
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::io::{Read, Seek, SeekFrom};
    let mut stderr = stderr;
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut diagnostic = String::new();
    stderr.take(16384).read_to_string(&mut diagnostic).unwrap();
    assert!(
        status.is_some(),
        "asset subprocess exceeded 15-second watchdog: {diagnostic}"
    );
    assert!(
        status.unwrap().success(),
        "asset subprocess failed: {diagnostic}"
    );
    let mut stdout = stdout;
    stdout.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    stdout.take(8 * 1_048_576).read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn python_exact_asset_bytes_all_roles_examples_unicode_and_shell_commands() {
    let expected: serde_json::Value = serde_json::from_slice(&run(Command::new("python3")
        .args(["-B", "-c", include_str!("oracle.py")])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PYTHONDONTWRITEBYTECODE", "1")))
    .unwrap();
    for case in expected.as_array().unwrap() {
        let root = case["root"].as_str().unwrap();
        let agent = case["agent"].as_str().unwrap();
        let command = case["command"].as_str().unwrap();
        let cli = case["cli"].as_str().unwrap();
        let example = case["example"].as_bool().unwrap();
        let context = PureAssetContext {
            cli_command: shell_quote(cli),
            example_text: example_text(if example {
                Some(" Interactive Demo ")
            } else {
                None
            })
            .unwrap(),
        };
        assert_eq!(agent_role(agent), case["role"]);
        assert_eq!(roleprompt_text(agent, root, &context), case["prompt"]);
        assert_eq!(starter_text(agent, root), case["starter"]);
        assert_eq!(normalize_opencode_command(command), case["normalized"]);
        assert_eq!(shell_quote(cli), case["shell_quote"]);
        let manifest =
            tmux_manifest("A 'name' \"  café 😀", root, &[agent], command, &context).unwrap();
        assert_eq!(manifest, case["manifest"]);
        manifest.parse::<toml::Value>().unwrap();
        for backend in ["tmux", "herdr"] {
            let launch =
                agent_launch_command("A 'name' \" \u{7f} café 😀", root, agent, command, backend)
                    .unwrap();
            assert_eq!(launch, case["launches"][backend]);
            assert_eq!(
                launch.matches("zsh -ic").count(),
                usize::from(normalize_opencode_command(command).starts_with("zsh -ic"))
            );
            // Syntax checks only: no backend, provider, delayed shell, or runtime.
            run(Command::new("/bin/sh").args(["-n", "-c", &launch]));
        }
    }
}

#[test]
fn profile_and_quote_boundaries() {
    assert!(example_text(None).unwrap().is_empty());
    for example in ["../custom", "___", "other-example"] {
        assert_eq!(
            example_text(Some(example)).unwrap_err().code(),
            "unsupported_refresh_example_profile"
        );
    }
    assert_eq!(q("😀\u{7f}"), "\"\\ud83d\\ude00\\u007f\"");
    assert_eq!(toml_quote("😀\u{7f}"), "\"😀\\u007f\"");
    assert_eq!(shell_quote(""), "''");
}

#[test]
fn unsafe_root_labels_are_literal_shell_data_with_native_command_goldens() {
    let context = PureAssetContext {
        cli_command: "coven".into(),
        example_text: String::new(),
    };
    for (root, label) in [
        (
            "/tmp/a'b",
            r#"printf '%s\n' 'Agent: CUSTOM' 'Prompt: /tmp/a'"'"'b/prompts/CUSTOM.md'"#,
        ),
        (
            "/tmp/100%ready",
            r"printf '%s\n' 'Agent: CUSTOM' 'Prompt: /tmp/100%ready/prompts/CUSTOM.md'",
        ),
        (
            r"/tmp/back\slash\nraw",
            r"printf '%s\n' 'Agent: CUSTOM' 'Prompt: /tmp/back\slash\nraw/prompts/CUSTOM.md'",
        ),
        (
            "/tmp/a'b%\\; $(touch NEVER)\nnext",
            "printf '%s\\n' 'Agent: CUSTOM' 'Prompt: /tmp/a'\"'\"'b%\\; $(touch NEVER)\nnext/prompts/CUSTOM.md'",
        ),
    ] {
        // Intentional native safety policy, not the unsafe Python launch prefix.
        let manifest =
            tmux_manifest("test", root, &["CUSTOM"], "opencode --flag 'x y'", &context).unwrap();
        let parsed = manifest.parse::<toml::Value>().unwrap();
        let launch = parsed["sessions"][0]["windows"][1]["panes"][0]["command"]
            .as_str()
            .unwrap();
        let expected = format!(
            r#"{label}; printf 'Auto-bootstrapping OpenCode with prompts/CUSTOM.start.txt...\n\n'; pane="$(tmux display-message -p '#{{pane_id}}')"; (sleep 5; tmux load-buffer -b "coven-test-CUSTOM-starter" "prompts/CUSTOM.start.txt"; tmux paste-buffer -b "coven-test-CUSTOM-starter" -t "$pane"; tmux send-keys -t "$pane" Enter) & opencode --flag 'x y'"#
        );
        assert_eq!(launch, expected);
        let native = agent_launch_command("test", root, "CUSTOM", "oc", "herdr").unwrap();
        assert!(native.starts_with(&format!("{label};")));
        assert!(
            native.contains("pane=\"${HERDR_PANE_ID:-}\"; herdr_bin=\"${HERDR_BIN_PATH:-herdr}\"")
        );
        assert!(native.contains(
            "(sleep 5; if [ -n \"$pane\" ]; then prompt=\"$(cat prompts/CUSTOM.start.txt)\";"
        ));
        assert!(!native.contains("admission"));
        run(Command::new("/bin/sh").args(["-n", "-c", &native]));
        run(Command::new("/bin/sh").args(["-n", "-c", launch]));
        let tokens: Vec<String> = serde_json::from_slice(&run(Command::new("python3").args([
            "-B",
            "-c",
            r#"
import json, shlex, sys
lexer = shlex.shlex(sys.argv[1], posix=True, punctuation_chars=';')
lexer.whitespace_split = True
first = []
for token in lexer:
    if token == ';':
        break
    first.append(token)
print(json.dumps(first))
"#,
            launch,
        ])))
        .unwrap();
        assert_eq!(
            tokens,
            [
                "printf".to_owned(),
                "%s\\n".to_owned(),
                "Agent: CUSTOM".to_owned(),
                format!("Prompt: {root}/prompts/CUSTOM.md")
            ]
        );
    }
}

#[test]
fn shared_launch_generator_rejects_identifiers_and_path_bounds_without_io() {
    for agent in ["", "../escape", "a/b", "a'b", "a\n", "a;echo", "😀"] {
        for backend in ["tmux", "herdr"] {
            assert_eq!(
                agent_launch_command("test", "/nonexistent", agent, "oc", backend)
                    .unwrap_err()
                    .code(),
                "invalid_record_identifier"
            );
        }
    }
    let max = "a".repeat(255 - ".start.txt".len());
    assert!(agent_launch_command("test", "/nonexistent", &max, "oc", "herdr").is_ok());
    for (root, agent) in [
        ("/tmp/\0bad".to_owned(), "agent".to_owned()),
        ("/tmp".to_owned(), format!("{max}a")),
        ("x".repeat(1_048_577), "agent".to_owned()),
    ] {
        assert_eq!(
            agent_launch_command("test", &root, &agent, "oc", "tmux")
                .unwrap_err()
                .code(),
            "unsupported_refresh_template_profile"
        );
    }
}

#[test]
fn launch_generators_reject_original_command_nul_before_normalization_without_io() {
    let context = PureAssetContext {
        cli_command: "coven".into(),
        example_text: String::new(),
    };
    for command in ["opencode\0", "oc --flag\0", "\0oc", "oc\0 --flag"] {
        for backend in ["tmux", "herdr"] {
            assert_eq!(
                agent_launch_command("test", "/nonexistent", "agent", command, backend)
                    .unwrap_err()
                    .code(),
                "invalid_runtime_command"
            );
        }
        // Manifest generation also normalizes once, so it must check raw input.
        for agents in [&["agent"][..], &[][..]] {
            assert_eq!(
                tmux_manifest("test", "/nonexistent", agents, command, &context)
                    .unwrap_err()
                    .code(),
                "invalid_runtime_command"
            );
        }
    }
}

#[test]
fn generated_launch_script_must_be_nul_free_without_io() {
    for backend in ["tmux", "herdr"] {
        // Exercise the final-script guard independently of the public raw-input
        // check, including the internal normalized-command path used by manifests.
        assert_eq!(
            normalized_agent_launch_command(
                "test", "/nonexistent", "agent", "opencode\0", backend,
            )
            .unwrap_err()
            .code(),
            "invalid_runtime_command"
        );
        let launch =
            agent_launch_command("test", "/nonexistent", "agent", "oc --flag", backend).unwrap();
        assert!(!launch.contains('\0'));
    }
}
