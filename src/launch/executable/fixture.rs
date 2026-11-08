//! Inert compiled backend for PATH fallback tests. Never invokes another process.
fn main() {
    let executable = std::env::current_exe().expect("fixture executable");
    if executable.parent().and_then(std::path::Path::file_name)
        == Some(std::ffi::OsStr::new("exit127"))
    {
        std::process::exit(127);
    }
    let stopping = std::env::current_dir().expect("fixture cwd").file_name()
        == Some(std::ffi::OsStr::new("workspace-stop"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if args.starts_with(&["has-session"]) {
        if !stopping {
            eprintln!("can't find session: coven-a-b");
            std::process::exit(1);
        }
    } else if args.starts_with(&["workspace", "list"]) {
        println!(
            "{}",
            if stopping {
                r#"{"result":{"workspaces":[{"label":"coven-a-b","workspace_id":"w"}]}}"#
            } else {
                r#"{"result":{"workspaces":[]}}"#
            }
        );
    } else if args.starts_with(&["workspace", "create"]) {
        println!(
            r#"{{"result":{{"workspace":{{"workspace_id":"w"}},"root_pane":{{"pane_id":"root"}}}}}}"#
        );
    } else if args.starts_with(&["tab", "create"]) {
        println!(r#"{{"result":{{"root_pane":{{"pane_id":"p"}}}}}}"#);
    }
}
