use serde_json::{Value, json};

#[allow(dead_code)]
mod detect {
    include!(concat!(env!("OUT_DIR"), "/detect.rs"));
}
mod platform {
    include!(concat!(env!("OUT_DIR"), "/platform.rs"));
    pub mod windows {
        use super::ForegroundJob;
        use std::collections::{HashMap, HashSet, VecDeque};
        include!(concat!(env!("OUT_DIR"), "/windows.rs"));

        pub fn select(root: u32, rows: &[serde_json::Value]) -> Option<ForegroundJob> {
            let entries: Vec<_> = rows
                .iter()
                .map(|row| {
                    let p = crate::process(row);
                    WindowsProcessEntry {
                        pid: p.pid,
                        parent_pid: row["parent_pid"].as_u64().unwrap() as u32,
                        name: p.name,
                        argv0: p.argv0,
                        argv: p.argv,
                        cmdline: p.cmdline,
                    }
                })
                .collect();
            select_pane_foreground_job(root, &entries)
        }
    }
}

fn process(row: &Value) -> platform::ForegroundProcess {
    platform::ForegroundProcess {
        pid: row["pid"].as_u64().unwrap().try_into().unwrap(),
        name: row["name"].as_str().unwrap().into(),
        argv0: row["argv0"].as_str().map(str::to_owned),
        argv: row["argv"]
            .as_array()
            .map(|a| a.iter().map(|s| s.as_str().unwrap().into()).collect()),
        cmdline: row["cmdline"].as_str().map(str::to_owned),
    }
}

fn row(pid: u32, parent: u32, name: &str, argv: &[&str]) -> Value {
    json!({"pid":pid,"parent_pid":parent,"name":name,"argv0":null,"argv":argv,"cmdline":null})
}

fn identification(job: &platform::ForegroundJob) -> Value {
    detect::identify_agent_in_job(job)
        .map(|(agent, candidate)| json!({"agent":detect::agent_label(agent),"candidate":candidate}))
        .unwrap_or(Value::Null)
}

fn serialized_job(job: Option<platform::ForegroundJob>) -> Value {
    job.map(|j| {
        json!({"process_group_id":j.process_group_id,
        "processes":j.processes.iter().map(|p| json!({"pid":p.pid,"name":p.name,
            "argv0":p.argv0,"argv":p.argv,"cmdline":p.cmdline})).collect::<Vec<_>>(),
        "identification":identification(&j)})
    })
    .unwrap_or(Value::Null)
}

fn capture() -> Value {
    let mut cases = Vec::new();
    let mut add = |name: String, pgid: u32, rows: Vec<Value>| {
        let job = platform::ForegroundJob {
            process_group_id: pgid,
            processes: rows.iter().map(process).collect(),
        };
        cases.push(json!({"name":name,"root_pid":pgid,"entries":rows,
            "identify_agent_in_job":identification(&job),
            "windows_selected_job":serialized_job(platform::windows::select(pgid, &rows))}));
    };
    add("empty".into(), 10, vec![]);
    add(
        "root-absent".into(),
        10,
        vec![row(20, 10, "codex", &["codex"])],
    );
    add("root-plain".into(), 10, vec![row(10, 0, "bash", &["bash"])]);
    for agent in detect::Agent::ALL {
        let label = detect::agent_label(agent);
        add(
            format!("direct/{label}"),
            10,
            vec![row(10, 0, label, &[label])],
        );
        add(
            format!("descendant/{label}"),
            10,
            vec![row(10, 0, "bash", &["bash"]), row(20, 10, label, &[label])],
        );
    }
    for alias in [
        "claude-code",
        "cursor-agent",
        "devin-cli",
        "devin cli",
        "antigravity",
        "antigravity-cli",
        "mastra-code",
        "mastra code",
        "open-code",
        "github-copilot",
        "ghcs",
        "kimi-code",
        "kimi code",
        "kiro-cli",
        "amp-local",
        "grok-build",
        "hermes-agent",
        "kilo-code",
        "kilo code",
        "qoderclicn",
        "qoder",
        "qodercn",
        "djinn-ui",
    ] {
        add(
            format!("alias/{alias}"),
            10,
            vec![row(10, 0, alias, &[alias])],
        );
    }
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
        let label = format!("DJINN{suffix}");
        add(
            format!("suffix/{suffix}"),
            10,
            vec![row(10, 0, &label, &[&label])],
        );
    }
    for (name, runtime, args) in [
        ("node-script", "node", vec!["node", "codex.js"]),
        (
            "node-value-option",
            "node",
            vec!["node", "--require", "unknown", "--", "codex"],
        ),
        ("bun-script", "bun", vec!["bun", "omp"]),
        ("python-script", "python3", vec!["python3", "codex"]),
        (
            "python-script-option",
            "python",
            vec!["python", "-W", "ignore", "codex"],
        ),
        ("shell-script", "sh", vec!["sh", "pi"]),
        ("bash-script", "bash", vec!["bash", "codex"]),
        ("zsh-script", "zsh", vec!["zsh", "codex"]),
        ("fish-script", "fish", vec!["fish", "codex"]),
        (
            "cmd-command",
            "cmd.exe",
            vec!["cmd.exe", "/D", "/S", "/C", "call \"codex.cmd\" --help"],
        ),
        ("cmd-keep", "cmd", vec!["cmd", "/K", "codex"]),
        (
            "powershell-file",
            "powershell.exe",
            vec![
                "powershell.exe",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "claude.ps1",
            ],
        ),
        (
            "pwsh-command",
            "pwsh",
            vec!["pwsh", "-Command", "& 'djinn-ui' --help"],
        ),
        ("pwsh-positional", "pwsh", vec!["pwsh", "djinn.ps1"]),
        ("node-eval", "node", vec!["node", "-e", "codex", "codex"]),
        ("node-inline-eval", "node", vec!["node", "-ecodex", "codex"]),
        (
            "node-long-eval",
            "node",
            vec!["node", "--eval=codex", "codex"],
        ),
        ("node-print", "node", vec!["node", "--print", "codex"]),
        ("bun-eval", "bun", vec!["bun", "-p", "codex"]),
        (
            "python-command",
            "python3",
            vec!["python3", "-c", "codex", "codex"],
        ),
        ("python-module", "python", vec!["python", "-m", "codex"]),
        (
            "shell-command",
            "bash",
            vec!["bash", "-c", "codex", "codex"],
        ),
        ("shell-login-combined", "bash", vec!["bash", "-lc", "codex"]),
        (
            "powershell-encoded",
            "powershell",
            vec!["powershell", "-EncodedCommand", "codex"],
        ),
        (
            "powershell-nonagent-command",
            "pwsh",
            vec!["pwsh", "-Command", "Write-Host codex"],
        ),
        ("tmux-nonclaim", "tmux", vec!["tmux", "codex"]),
        ("unknown-script", "node", vec!["node", "unknown"]),
        (
            "package-cli",
            "node",
            vec![
                "node",
                "sandbox/node_modules/@earendil-works/pi-coding-agent/dist/cli.js",
            ],
        ),
        (
            "package-non-cli",
            "node",
            vec![
                "node",
                "sandbox/node_modules/@earendil-works/pi-coding-agent/scripts/build.js",
            ],
        ),
        (
            "controlled-missing-path",
            "node",
            vec!["node", "sandbox/missing"],
        ),
        (
            "controlled-symlink",
            "node",
            vec!["node", "sandbox/renamed"],
        ),
    ] {
        add(
            format!("wrapper/{name}"),
            10,
            vec![row(10, 0, runtime, &args)],
        );
    }
    for (name, name_value, argv0, argv, cmdline) in [
        ("name-only", "djinn", None, None, None),
        ("argv0-wins", "codex", Some("djinn"), None, None),
        ("argv0-masks-name", "djinn", Some("unknown"), None, None),
        (
            "argv-fallback",
            "MainThread",
            None,
            Some(vec!["djinn-ui"]),
            None,
        ),
        (
            "cmdline-fallback",
            "MainThread",
            None,
            None,
            Some("ghcs --help"),
        ),
        (
            "argv-before-cmdline",
            "MainThread",
            None,
            Some(vec!["codex"]),
            Some("djinn"),
        ),
        (
            "empty-argv-cmdline",
            "MainThread",
            None,
            Some(vec![]),
            Some("djinn"),
        ),
        (
            "unrecognized-argv-cmdline",
            "MainThread",
            None,
            Some(vec!["unknown"]),
            Some("djinn"),
        ),
        (
            "quoted-cmdline-first-token",
            "MainThread",
            None,
            None,
            Some("\"djinn ui\""),
        ),
        ("trim-case", " DJINN ", None, None, None),
    ] {
        add(
            format!("fallback/{name}"),
            10,
            vec![
                json!({"pid":10,"parent_pid":0,"name":name_value,"argv0":argv0,"argv":argv,"cmdline":cmdline}),
            ],
        );
    }
    for mode in ["--help", "--version", "serve", "config", "maintenance"] {
        add(
            format!("mode-not-excluded/{mode}"),
            10,
            vec![row(10, 0, "djinn", &["djinn", mode])],
        );
    }
    let root = row(10, 0, "bash", &["bash"]);
    let codex = row(20, 10, "codex", &["codex"]);
    let child = row(30, 20, "codex", &["codex"]);
    for (name, rows) in [
        (
            "plain-descendant",
            vec![root.clone(), row(20, 10, "unknown", &["unknown"])],
        ),
        (
            "same-chain",
            vec![root.clone(), codex.clone(), child.clone()],
        ),
        (
            "same-chain-intermediary",
            vec![
                root.clone(),
                codex.clone(),
                row(25, 20, "unknown", &["unknown"]),
                row(30, 25, "codex", &["codex"]),
            ],
        ),
        (
            "siblings",
            vec![
                root.clone(),
                codex.clone(),
                row(30, 10, "codex", &["codex"]),
            ],
        ),
        (
            "mixed-nested",
            vec![
                root.clone(),
                codex.clone(),
                row(30, 20, "djinn", &["djinn"]),
            ],
        ),
        (
            "mixed-siblings",
            vec![
                root.clone(),
                codex.clone(),
                row(30, 10, "djinn", &["djinn"]),
            ],
        ),
        (
            "unrelated",
            vec![
                root.clone(),
                codex.clone(),
                row(99, 98, "djinn", &["djinn"]),
            ],
        ),
        (
            "missing-parent",
            vec![root.clone(), row(30, 25, "codex", &["codex"])],
        ),
        (
            "disconnected-cycle",
            vec![
                root.clone(),
                row(20, 30, "codex", &["codex"]),
                row(30, 20, "djinn", &["djinn"]),
            ],
        ),
        (
            "root-cycle",
            vec![row(10, 30, "bash", &["bash"]), codex.clone(), child.clone()],
        ),
        (
            "self-parent",
            vec![root.clone(), row(20, 20, "codex", &["codex"])],
        ),
        (
            "root-agent-descendant-other",
            vec![row(10, 0, "djinn", &["djinn"]), codex.clone()],
        ),
        (
            "pgid-leader",
            vec![
                row(10, 0, "claude", &["claude"]),
                row(20, 10, "node", &["node", "codex"]),
            ],
        ),
        (
            "priority-wrapped",
            vec![
                root.clone(),
                codex.clone(),
                row(30, 10, "node", &["node", "djinn"]),
            ],
        ),
        (
            "priority-tie",
            vec![
                root.clone(),
                codex.clone(),
                row(30, 10, "djinn", &["djinn"]),
            ],
        ),
    ] {
        // Exhaustive input-order permutations, not an independent policy implementation.
        fn permutations(rows: &mut [Value], at: usize, out: &mut Vec<Vec<Value>>) {
            if at == rows.len() {
                out.push(rows.to_vec());
                return;
            }
            for i in at..rows.len() {
                rows.swap(at, i);
                permutations(rows, at + 1, out);
                rows.swap(at, i);
            }
        }
        let mut variants = Vec::new();
        permutations(&mut rows.clone(), 0, &mut variants);
        for (i, entries) in variants.into_iter().enumerate() {
            add(format!("tree/{name}/{i}"), 10, entries);
        }
    }
    json!({"reference":serde_json::from_str::<Value>(include_str!(concat!(env!("OUT_DIR"),"/provenance.json"))).unwrap(),"cases":cases})
}

fn sandboxed_capture() -> Value {
    let previous = std::env::current_dir().unwrap();
    let sandbox = tempfile::tempdir().unwrap();
    std::env::set_current_dir(sandbox.path()).unwrap();
    std::fs::create_dir("sandbox").unwrap();
    std::fs::write("sandbox/cursor-agent", b"inert fixture; never executable").unwrap();
    std::os::unix::fs::symlink("cursor-agent", "sandbox/renamed").unwrap();
    let result = capture();
    std::env::set_current_dir(previous).unwrap();
    result
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(
        args.len() == 1 || (args.len() == 2 && args[0] == "--check"),
        "usage: [--check] absolute-output.json"
    );
    let path = std::path::PathBuf::from(args.last().unwrap());
    assert!(path.is_absolute(), "output/check path must be absolute");
    let mut bytes = serde_json::to_vec_pretty(&sandboxed_capture()).unwrap();
    bytes.push(b'\n');
    if args.len() == 2 {
        assert_eq!(std::fs::read(path).unwrap(), bytes, "capture drift");
    } else {
        std::fs::write(path, bytes).unwrap();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_execution_and_private_filesystem_are_repeatable() {
        let first = super::sandboxed_capture();
        assert_eq!(first, super::sandboxed_capture());
        let cases = first["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 209);
        let case = |name| cases.iter().find(|c| c["name"] == name).unwrap();
        assert_eq!(
            case("wrapper/controlled-symlink")["identify_agent_in_job"]["agent"],
            "cursor"
        );
        assert!(case("wrapper/controlled-missing-path")["identify_agent_in_job"].is_null());
        assert!(case("root-absent")["windows_selected_job"].is_null());
        assert_eq!(
            case("tree/same-chain-intermediary/0")["windows_selected_job"]["process_group_id"],
            20
        );
        assert_eq!(
            case("tree/siblings/0")["windows_selected_job"]["process_group_id"],
            10
        );
    }
}
