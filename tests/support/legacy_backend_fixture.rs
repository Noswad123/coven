//! Std-only stateful transport. Scripts are literal DATA, never executed.
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::fd::AsRawFd,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn flock(fd: i32, operation: i32) -> i32;
    fn isatty(fd: i32) -> i32;
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
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> String {
    String::from_utf8(
        s.as_bytes()
            .chunks_exact(2)
            .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}

fn property(rows: &mut Vec<Vec<String>>, kind: &str, id: &str, value: &str) {
    rows.retain(|r| !(r[0] == kind && r[1] == id));
    rows.push(vec![kind.into(), id.into(), value.into()]);
}

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("isolated HOME"));
    let args: Vec<String> = std::env::args().collect();
    let backend = PathBuf::from(&args[0])
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(matches!(backend.as_str(), "tmux" | "herdr"));
    let a = &args[1..];
    let command = a.join(" ");
    let control: BTreeMap<String, String> =
        fs::read_to_string(home.join("runtime-fixture-control"))
            .unwrap()
            .lines()
            .filter_map(|s| s.split_once('='))
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
    let workspace = PathBuf::from(control.get("workspace").expect("explicit workspace"));
    let null = fs::metadata("/dev/null").unwrap();
    let nulls: Vec<bool> = (0..3)
        .map(|fd| {
            fs::metadata(format!("/dev/fd/{fd}"))
                .is_ok_and(|m| m.rdev() == null.rdev() && m.mode() & 0o170000 == 0o020000)
        })
        .collect();
    let ttys: Vec<bool> = (0..3).map(|fd| unsafe { isatty(fd) } == 1).collect();
    let inherited: Vec<_> = (3..4096)
        .filter(|fd| unsafe { fcntl(*fd, 1) } >= 0)
        .collect();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(workspace.join("state/.workspace.lock"))
        .ok();
    let present = lock.is_some();
    let locked = lock
        .as_ref()
        .is_some_and(|f| unsafe { flock(f.as_raw_fd(), 2 | 4) } != 0);
    drop(lock);
    let env: BTreeMap<_, _> = std::env::vars().collect();
    let record = format!(
        "{{\"backend\":{},\"image\":{},\"argv\":[{}],\"cwd\":{},\"env\":{{{}}},\"stdin_null\":{},\"stdio_null\":{:?},\"ttys\":{:?},\"inherited_fds\":{:?},\"workspace_lock_present\":{},\"workspace_locked\":{}}}\n",
        quote(&backend),
        quote(std::env::current_exe().unwrap().to_str().unwrap()),
        a.iter().map(|v| quote(v)).collect::<Vec<_>>().join(","),
        quote(std::env::current_dir().unwrap().to_str().unwrap()),
        env.iter()
            .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
            .collect::<Vec<_>>()
            .join(","),
        nulls[0],
        nulls,
        ttys,
        inherited,
        present,
        locked
    );
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("runtime-fixture-calls.jsonl"))
        .unwrap()
        .write_all(record.as_bytes())
        .unwrap();
    if control
        .get("barrier")
        .is_some_and(|s| !s.is_empty() && command.starts_with(s))
    {
        fs::write(home.join("runtime-fixture-entered"), &command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        while !home.join("runtime-fixture-release").exists() {
            assert!(Instant::now() < deadline, "fixture barrier deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if control
        .get("fail")
        .is_some_and(|s| !s.is_empty() && command.starts_with(s))
    {
        let counter = home.join("legacy-failure-counter");
        let n = fs::read_to_string(&counter)
            .unwrap_or_default()
            .parse::<usize>()
            .unwrap_or(0)
            + 1;
        fs::write(counter, n.to_string()).unwrap();
        if control
            .get("fail_nth")
            .is_none_or(|s| s.parse::<usize>().unwrap() == n)
        {
            // Quiet ordinary nonzero keeps legacy inherited stderr deterministic.
            std::process::exit(17);
        }
    }
    let path = home.join("legacy-topology");
    let mut rows: Vec<Vec<String>> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.split('\t').map(unhex).collect())
        .collect();
    let flag = |key: &str| {
        a.iter()
            .position(|s| s == key)
            .map(|i| a[i + 1].clone())
            .unwrap()
    };
    let next = |rows: &Vec<Vec<String>>| {
        rows.iter()
            .filter(|r| matches!(r[0].as_str(), "window" | "tab" | "pane"))
            .filter_map(|r| r.last()?.parse::<usize>().ok())
            .max()
            .unwrap_or(0)
            + 1
    };
    let mut output = None;
    let mut code = 0;
    if backend == "tmux" {
        let target_name = |s: String| {
            s.split(':')
                .nth(1)
                .unwrap_or("")
                .split('.')
                .next()
                .unwrap_or("")
                .to_owned()
        };
        match a[0].as_str() {
            "has-session" => {
                code = if rows
                    .iter()
                    .any(|r| r[0] == "session" && r[1] == flag("-t").trim_start_matches('='))
                {
                    0
                } else {
                    1
                }
            }
            "new-session" => {
                rows.clear();
                rows.push(vec!["session".into(), flag("-s")]);
                rows.push(vec!["window".into(), flag("-n"), "%1".into(), "1".into()]);
                property(&mut rows, "script", "%1", a.last().unwrap());
            }
            "kill-session" => rows.clear(),
            "new-window" => {
                let n = next(&rows);
                rows.push(vec![
                    "window".into(),
                    flag("-n"),
                    format!("%{n}"),
                    n.to_string(),
                ]);
                property(&mut rows, "script", &format!("%{n}"), a.last().unwrap());
            }
            "kill-window" => {
                let name = target_name(flag("-t"));
                rows.retain(|r| !(r[0] == "window" && r[1] == name));
            }
            "list-panes" => {
                let name = target_name(flag("-t"));
                output = rows
                    .iter()
                    .find(|r| r[0] == "window" && r[1] == name)
                    .map(|r| r[2].clone());
                if output.is_none() {
                    code = 1;
                }
            }
            "split-window" => {
                let n = next(&rows);
                let id = format!("%{n}");
                rows.push(vec!["pane".into(), id.clone(), n.to_string()]);
                property(&mut rows, "script", &id, a.last().unwrap());
                output = Some(id);
            }
            "load-buffer" => {
                let name = flag("-b");
                rows.retain(|r| !(r[0] == "buffer" && r[1] == name));
                rows.push(vec![
                    "buffer".into(),
                    name,
                    fs::read_to_string(a.last().unwrap()).unwrap(),
                ]);
            }
            "paste-buffer" => {
                let text = rows
                    .iter()
                    .find(|r| r[0] == "buffer" && r[1] == flag("-b"))
                    .map(|r| r[2].clone());
                if let Some(text) = text {
                    property(&mut rows, "pasted", &flag("-t"), &text);
                } else {
                    code = 1;
                }
            }
            "select-pane" => property(&mut rows, "title", &flag("-t"), &flag("-T")),
            "select-layout" => property(&mut rows, "layout", &flag("-t"), a.last().unwrap()),
            "select-window" | "attach-session" | "switch-client" => {
                property(&mut rows, "focus", "session", &flag("-t"));
            }
            "send-keys" => (),
            _ => panic!("unexpected tmux call: {a:?}"),
        }
    } else {
        let list = |kind: &str, rows: &Vec<Vec<String>>| -> String {
            rows.iter()
                .filter(|r| r[0] == kind)
                .map(|r| match kind {
                    "workspace" => format!(
                        "{{\"workspace_id\":{},\"label\":{}}}",
                        quote(&r[1]),
                        quote(&r[2])
                    ),
                    "tab" => format!(
                        "{{\"workspace_id\":\"w1\",\"tab_id\":{},\"label\":{}}}",
                        quote(&r[1]),
                        quote(&r[2])
                    ),
                    "pane" => format!(
                        "{{\"tab_id\":{},\"pane_id\":{}}}",
                        quote(&r[2]),
                        quote(&r[1])
                    ),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        match (a[0].as_str(), a[1].as_str()) {
            ("workspace", "list") => {
                output = Some(format!(
                    "{{\"result\":{{\"workspaces\":[{}]}}}}",
                    list("workspace", &rows)
                ))
            }
            ("workspace", "create") => {
                rows.clear();
                rows.extend([
                    vec!["workspace".into(), "w1".into(), flag("--label")],
                    vec!["tab".into(), "t1".into(), "orchestrator".into(), "1".into()],
                    vec!["pane".into(), "p1".into(), "t1".into(), "1".into()],
                ]);
                output=Some("{\"result\":{\"workspace\":{\"workspace_id\":\"w1\"},\"tab\":{\"tab_id\":\"t1\"},\"root_pane\":{\"pane_id\":\"p1\"}}}".into());
            }
            ("workspace", "close") => rows.clear(),
            ("tab", "list") => {
                output = Some(format!(
                    "{{\"result\":{{\"tabs\":[{}]}}}}",
                    list("tab", &rows)
                ))
            }
            ("pane", "list") => {
                output = Some(format!(
                    "{{\"result\":{{\"panes\":[{}]}}}}",
                    list("pane", &rows)
                ))
            }
            ("tab", "create") => {
                let n = next(&rows);
                let tab = format!("t{n}");
                let pane = format!("p{n}");
                rows.push(vec![
                    "tab".into(),
                    tab.clone(),
                    flag("--label"),
                    n.to_string(),
                ]);
                rows.push(vec!["pane".into(), pane.clone(), tab, n.to_string()]);
                output = Some(format!(
                    "{{\"result\":{{\"root_pane\":{{\"pane_id\":{}}}}}}}",
                    quote(&pane)
                ));
            }
            ("pane", "split") => {
                let tab = rows
                    .iter()
                    .find(|r| r[0] == "pane" && r[1] == flag("--pane"))
                    .unwrap()[2]
                    .clone();
                let n = next(&rows);
                let pane = format!("p{n}");
                rows.push(vec!["pane".into(), pane.clone(), tab, n.to_string()]);
                output = Some(format!(
                    "{{\"result\":{{\"pane\":{{\"pane_id\":{}}}}}}}",
                    quote(&pane)
                ));
            }
            ("tab", "rename") => {
                rows.iter_mut()
                    .find(|r| r[0] == "tab" && r[1] == a[2])
                    .unwrap()[2] = a[3].clone()
            }
            ("tab", "close") => {
                let tab = &a[2];
                rows.retain(|r| {
                    !((r[0] == "tab" && &r[1] == tab) || (r[0] == "pane" && &r[2] == tab))
                });
            }
            ("pane", "rename" | "run" | "send-text") => {
                assert!(rows.iter().any(|r| r[0] == "pane" && r[1] == a[2]));
                let kind = match a[1].as_str() {
                    "rename" => "title",
                    "run" => "script",
                    _ => "text",
                };
                property(&mut rows, kind, &a[2], &a[3]);
            }
            ("pane", "send-keys") => (),
            ("tab" | "workspace", "focus") => property(&mut rows, "focus", &a[0], &a[2]),
            _ => panic!("unexpected Herdr call: {a:?}"),
        }
    }
    fs::write(
        path,
        rows.iter()
            .map(|r| r.iter().map(|s| hex(s)).collect::<Vec<_>>().join("\t") + "\n")
            .collect::<String>(),
    )
    .unwrap();
    if let Some(output) = output {
        println!("{output}");
    }
    std::process::exit(code);
}
