//! Inert native peer for the generated-shell characterization, not a real runtime.
use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() {
    let home = PathBuf::from(env::var_os("HOME").unwrap());
    let args: Vec<String> = env::args().skip(1).collect();
    let verb = args[0].as_str();
    let event = |value: &str| {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.join("events"))
            .unwrap();
        writeln!(file, "{value}").unwrap();
    };
    if verb == "foreground" {
        event("foreground-started");
        fs::write(home.join("foreground-started"), []).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !home.join("release-foreground").exists() {
            assert!(
                Instant::now() < deadline,
                "test harness did not release foreground"
            );
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let mut encoded = Vec::new();
    for arg in &args {
        encoded.extend_from_slice(arg.as_bytes());
        encoded.push(0);
    }
    let call = if verb == "pane" {
        args[1].as_str()
    } else {
        verb
    };
    fs::write(home.join(format!("{call}.argv")), encoded).unwrap();
    if verb != "display-message" {
        assert!(
            home.join("exclusive-held").exists(),
            "input preceded exclusive admission"
        );
    }
    event(call);
    match verb {
        "display-message" => println!("{}", fs::read_to_string(home.join("target")).unwrap()),
        "load-buffer" => fs::write(home.join("buffer"), fs::read(&args[3]).unwrap()).unwrap(),
        "paste-buffer" => fs::write(
            home.join("delivered"),
            fs::read(home.join("buffer")).unwrap(),
        )
        .unwrap(),
        "send-text" => unreachable!("Herdr uses pane send-text"),
        "pane" => match args[1].as_str() {
            "send-text" => fs::write(home.join("delivered"), args[3].as_bytes()).unwrap(),
            "send-keys" => {
                fs::write(home.join("done"), []).unwrap();
            }
            _ => panic!("unexpected Herdr command"),
        },
        "send-keys" => {
            fs::write(home.join("done"), []).unwrap();
        }
        _ => panic!("unexpected native peer invocation"),
    }
}
