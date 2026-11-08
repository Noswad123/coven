//! Standalone std-only inert tmux/Herdr. Controls and observations use HOME,
//! because reviewed plans intentionally discard arbitrary environment variables.
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn flock(fd: i32, operation: i32) -> i32;
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() {
    use std::os::fd::AsRawFd;
    let home = PathBuf::from(std::env::var_os("HOME").expect("isolated HOME"));
    let args: Vec<String> = std::env::args().collect();
    let backend = PathBuf::from(&args[0])
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(matches!(backend.as_str(), "tmux" | "herdr"));
    let argv = &args[1..];
    let command = argv.join(" ");
    let controls: BTreeMap<String, String> =
        fs::read_to_string(home.join("runtime-fixture-control"))
            .unwrap_or_default()
            .lines()
            .filter_map(|s| s.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    let cwd = std::env::current_dir().unwrap();
    let workspace = PathBuf::from(
        controls
            .get("workspace")
            .expect("explicit fixture workspace"),
    );
    let image = std::env::current_exe().unwrap();
    let null = fs::metadata("/dev/null").unwrap();
    let stdin_null = fs::metadata("/dev/stdin")
        .is_ok_and(|m| m.rdev() == null.rdev() && m.mode() & 0o170000 == 0o020000);
    let inherited: Vec<_> = (3..4096)
        .filter(|fd| unsafe { fcntl(*fd, 1) } >= 0)
        .collect();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(workspace.join("state/.workspace.lock"))
        .ok();
    let workspace_lock_present = lock.is_some();
    let workspace_locked = lock
        .as_ref()
        .is_some_and(|f| unsafe { flock(f.as_raw_fd(), 2 | 4) } != 0);
    drop(lock);
    let env: BTreeMap<_, _> = std::env::vars().collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let record = format!(
        "{{\"backend\":{},\"image\":{},\"argv\":[{}],\"cwd\":{},\"env\":{{{}}},\"stdin_null\":{},\"inherited_fds\":{:?},\"workspace_lock_present\":{},\"workspace_locked\":{},\"at_ns\":{}}}\n",
        quote(&backend),
        quote(image.to_str().unwrap()),
        argv.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","),
        quote(cwd.to_str().unwrap()),
        env.iter()
            .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
            .collect::<Vec<_>>()
            .join(","),
        stdin_null,
        inherited,
        workspace_lock_present,
        workspace_locked,
        now
    );
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("runtime-fixture-calls.jsonl"))
        .unwrap()
        .write_all(record.as_bytes())
        .unwrap();
    if controls
        .get("barrier")
        .is_some_and(|v| !v.is_empty() && command.contains(v))
    {
        fs::write(home.join("runtime-fixture-entered"), command.as_bytes()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !home.join("runtime-fixture-release").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture barrier deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if let Some(ms) = controls.get("delay_ms") {
        std::thread::sleep(Duration::from_millis(ms.parse().unwrap()));
    }
    if controls
        .get("fail")
        .is_some_and(|v| !v.is_empty() && command.contains(v))
    {
        eprintln!("injected required command failure");
        let code = controls
            .get("fail_exit")
            .map_or(17, |value| value.parse().unwrap());
        std::process::exit(code);
    }
    let state = home.join(format!("runtime-fixture-{backend}-state"));
    let label = fs::read_to_string(&state).unwrap_or_default();
    let value_after = |flag: &str| -> String {
        argv.iter()
            .position(|v| v == flag)
            .map(|i| argv[i + 1].clone())
            .unwrap()
    };
    if backend == "tmux" {
        match argv.first().map(String::as_str) {
            Some("has-session") => {
                if controls.get("query_error").is_some_and(|v| v == "true") {
                    eprintln!("permission denied by fixture");
                    std::process::exit(1);
                }
                if label.is_empty() {
                    eprintln!("can't find session: {}", value_after("-t"));
                    std::process::exit(1);
                }
            }
            Some("new-session") => fs::write(&state, value_after("-s")).unwrap(),
            Some("kill-session") => {
                fs::remove_file(&state).unwrap();
            }
            Some("list-panes") => println!("%9001"),
            Some("new-window" | "send-keys") => (),
            _ => panic!("unexpected inert tmux call: {argv:?}"),
        }
    } else {
        match argv.get(..2).map(|s| (s[0].as_str(), s[1].as_str())) {
            Some(("workspace", "list")) => {
                let entries = if label.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{{\"workspace_id\":\"fixture_workspace\",\"label\":{}}}",
                        quote(&label)
                    )
                };
                println!("{{\"result\":{{\"workspaces\":[{entries}]}}}}");
            }
            Some(("workspace", "create")) => {
                fs::write(&state, value_after("--label")).unwrap();
                println!(
                    "{{\"result\":{{\"workspace\":{{\"workspace_id\":\"fixture_workspace\"}},\"root_pane\":{{\"pane_id\":\"fixture_orchestrator\"}}}}}}"
                );
            }
            Some(("tab", "create")) => {
                println!("{{\"result\":{{\"root_pane\":{{\"pane_id\":\"fixture_agent\"}}}}}}")
            }
            Some(("tab", "list")) => println!(
                "{{\"result\":{{\"tabs\":[{{\"workspace_id\":\"fixture_workspace\",\"tab_id\":\"fixture_tab\",\"label\":\"agent-clippy\"}}]}}}}"
            ),
            Some(("pane", "list")) => println!(
                "{{\"result\":{{\"panes\":[{{\"tab_id\":\"fixture_tab\",\"pane_id\":\"fixture_agent\"}}]}}}}"
            ),
            Some(("workspace", "close")) => {
                fs::remove_file(&state).unwrap();
            }
            Some(("pane", "run" | "send-text" | "send-keys")) => (),
            _ => panic!("unexpected inert Herdr call: {argv:?}"),
        }
    }
}
