use serde_json::{Value, json};
use std::path::PathBuf;

// Dev scaffold: unchanged early source also includes public helpers and cfg(test)
// wrappers not all exercised by this deliberately restricted process-only build.
// Preserve the candidate's nested if verbatim rather than fixing its style here.
#[allow(dead_code, clippy::collapsible_if)]
mod detect {
    include!(concat!(env!("OUT_DIR"), "/detect.rs"));
}

mod platform {
    include!(concat!(env!("OUT_DIR"), "/platform.rs"));
    mod process_selection {
        include!(concat!(env!("OUT_DIR"), "/process_selection.rs"));
    }
    pub mod windows {
        use super::{ForegroundJob, process_selection::ProcessEntry};
        include!(concat!(env!("OUT_DIR"), "/windows.rs"));

        pub fn select(root: u32, rows: &[serde_json::Value]) -> Option<ForegroundJob> {
            let entries: Vec<_> = rows
                .iter()
                .map(|row| {
                    let p = crate::process(row);
                    ProcessEntry {
                        pid: p.pid,
                        parent_pid: crate::pid(&row["parent_pid"]),
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

fn pid(value: &Value) -> u32 {
    value.as_u64().unwrap().try_into().unwrap()
}

fn process(row: &Value) -> platform::ForegroundProcess {
    platform::ForegroundProcess {
        pid: pid(&row["pid"]),
        name: row["name"].as_str().unwrap().into(),
        argv0: row["argv0"].as_str().map(str::to_owned),
        argv: row["argv"]
            .as_array()
            .map(|a| a.iter().map(|s| s.as_str().unwrap().into()).collect()),
        cmdline: row["cmdline"].as_str().map(str::to_owned),
    }
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

struct RestoreCwd(PathBuf);
impl Drop for RestoreCwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore cwd before removing sandbox");
    }
}

fn replay() -> Result<usize, String> {
    let fixture: Value =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/fixture.json"))).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 209);
    let sandbox = tempfile::tempdir().unwrap();
    std::fs::create_dir(sandbox.path().join("sandbox")).unwrap();
    std::fs::write(
        sandbox.path().join("sandbox/cursor-agent"),
        b"inert fixture; never executable",
    )
    .unwrap();
    std::os::unix::fs::symlink("cursor-agent", sandbox.path().join("sandbox/renamed")).unwrap();
    // Only this single-threaded scope changes cwd. The source tests use injected
    // resolvers or lexical matches, not filesystem/cwd mutation.
    let restore = RestoreCwd(std::env::current_dir().unwrap());
    std::env::set_current_dir(sandbox.path()).unwrap();
    let mut mismatches = Vec::new();
    for case in cases {
        let rows = case["entries"].as_array().unwrap();
        let root = pid(&case["root_pid"]);
        let job = platform::ForegroundJob {
            process_group_id: root,
            processes: rows.iter().map(process).collect(),
        };
        for (key, actual) in [
            ("identify_agent_in_job", identification(&job)),
            (
                "windows_selected_job",
                serialized_job(platform::windows::select(root, rows)),
            ),
        ] {
            if actual != case[key] {
                let name: String = case["name"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || "/._-".contains(c) {
                            c
                        } else {
                            '_'
                        }
                    })
                    .take(120)
                    .collect();
                mismatches.push(format!("{name}: {key}\n- {}\n+ {actual}", case[key]));
            }
        }
    }
    drop(restore);
    if mismatches.is_empty() {
        Ok(cases.len())
    } else {
        Err(mismatches.join("\n"))
    }
}

fn main() {
    match replay() {
        Ok(count) => {
            println!("{count} cases passed; current candidate source subset only");
            println!(
                "{}",
                include_str!(concat!(env!("OUT_DIR"), "/provenance.json"))
            );
        }
        Err(diff) => {
            eprintln!("{diff}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn frozen_baseline_replay() {
        assert_eq!(super::replay(), Ok(209));
    }
}
