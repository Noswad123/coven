//! Controlled help/persisted-evidence producer. No interpreter or dependencies.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c <= '\u{1f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn text(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join(name)).unwrap_or_default()
}

fn main() {
    let exe = std::env::current_exe().expect("fixture executable");
    let root = exe.parent().expect("fixture directory");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("null stdin");
    let mut env: Vec<_> = std::env::vars().collect();
    env.sort();
    let environment = env
        .iter()
        .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
        .collect::<Vec<_>>()
        .join(",");
    let mut descriptors = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc/self/fd") {
        for entry in entries.flatten() {
            if let Ok(target) = fs::read_link(entry.path()) {
                let label = target.to_string_lossy().into_owned();
                if !label.starts_with("/proc/") {
                    descriptors.push(label);
                }
            }
        }
    }
    descriptors.sort();
    let log = format!(
        "{{\"argv\":[{}],\"cwd\":{},\"environment\":{{{}}},\"stdin_bytes\":{},\"fds\":[{}]}}\n",
        args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","),
        quote(&std::env::current_dir().expect("cwd").to_string_lossy()),
        environment,
        input.len(),
        descriptors
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(",")
    );
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("calls.jsonl"))
        .expect("external call log")
        .write_all(log.as_bytes())
        .expect("call log");
    if let Ok(milliseconds) = text(root, "delay_ms").parse::<u64>() {
        std::thread::sleep(std::time::Duration::from_millis(milliseconds));
    }
    let variant = text(root, "variant");
    let mode = text(root, "mode");
    let mut output = Vec::new();
    if mode == "help" {
        assert!(matches!(
            args.last().map(String::as_str),
            Some("--help" | "--version")
        ));
        if args == ["--version"] {
            output.extend_from_slice(b"djinn 9.8.7\n");
        } else if variant == "missing-failure" && args == ["ask", "--help"] {
            output.extend_from_slice(b"PRIVATE_NATIVE_TRANSCRIPT\n");
        } else {
            let route = &args[..args.len() - 1];
            let commands: &[&str] = if route.is_empty() {
                if variant.starts_with("missing") {
                    &["ask", "agents"]
                } else {
                    &["session", "ask", "agents"]
                }
            } else if route == ["session"] {
                &["run", "status", "watch"]
            } else {
                &[]
            };
            let mut help = format!(
                "PRIVATE_NATIVE_TRANSCRIPT\nUsage: djinn {}[OPTIONS]\n",
                if route.is_empty() {
                    String::new()
                } else {
                    format!("{} ", route.join(" "))
                }
            );
            if !commands.is_empty() {
                help.push_str("\nCommands:\n");
                for command in commands {
                    help.push_str(&format!("  {command}  Description\n"));
                }
            }
            help.push_str("\nOptions:\n  -h, --help  Help\n");
            if variant != "wrapped" && route.is_empty() {
                help.push_str("      --version  Print version\n");
            }
            if variant == "wrapped" {
                help.push_str("          --version is prose, not a flag\n");
            }
            if variant == "ansi" {
                help = help.replace("Usage:", "\x1b[1mUsage:\x1b[0m");
            }
            output.extend_from_slice(help.as_bytes());
        }
    } else {
        assert_eq!(args.len(), 4, "only fixed session observation");
        assert_eq!(&args[..2], ["session", "observe"]);
        assert_eq!(args[3], "--json");
        output = fs::read(root.join("payload")).expect("controlled payload");
    }
    if variant == "retarget" {
        let alias = text(root, "alias");
        let replacement = text(root, "replacement");
        fs::remove_file(&alias).expect("remove alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(replacement, alias).expect("retarget alias");
    }
    let stderr = fs::read(root.join("stderr")).unwrap_or_default();
    std::io::stdout().write_all(&output).expect("stdout");
    std::io::stderr().write_all(&stderr).expect("stderr");
    let code = text(root, "exit").parse::<i32>().unwrap_or(0);
    std::process::exit(
        if variant == "missing-failure" && args == ["ask", "--help"] {
            9
        } else {
            code
        },
    );
}
