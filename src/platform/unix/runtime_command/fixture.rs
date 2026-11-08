// Compiled inert native fixture, never part of the application binary.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn inherited_fds() -> String {
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    // F_GETFD is 1 on both supported fixture platforms (Linux and macOS).
    (3..4096)
        .filter(|fd| unsafe { fcntl(*fd, 1) } >= 0)
        .map(|fd| fd.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "exit127" => {
            std::fs::write(&args[2], b"image executed").unwrap();
            std::process::exit(127);
        }
        "list-panes" | "send-keys" => {
            let inherited = inherited_fds();
            let log = std::env::var_os("COVEN_FD_TEST_LOG").unwrap();
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .unwrap();
            writeln!(file, "{}:{}", args[1], inherited).unwrap();
            if args[1] == "list-panes" {
                println!("%12");
            }
        }
        "fd-backend" => {
            println!("{}", inherited_fds());
            let _daemon = Command::new(&args[0])
                .args(["fd-daemon", &args[2]])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
        }
        "fd-daemon" => {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::write(&args[2], inherited_fds()).unwrap();
        }
        "success" => {
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            assert!(input.is_empty());
            assert_eq!(std::env::var("ONLY").unwrap(), "explicit");
            assert!(std::env::var_os("HOME").is_none());
            print!("success");
            eprint!("stderr");
            std::process::exit(7);
        }
        "bytes" => {
            std::io::stdout().write_all(b"abcd").unwrap();
            std::io::stderr().write_all(b"EFGH").unwrap();
        }
        "sleep" => {
            println!("{}", std::process::id());
            std::io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_secs(10));
        }
        "parent" | "parent-sleep" | "parent-closed" | "parent-flood" => {
            let mut command = Command::new(&args[0]);
            command.args(["daemon", &args[2]]).stdin(Stdio::null());
            if args[1] == "parent-closed" {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
            let daemon = command.spawn().unwrap();
            println!("{} {}", std::process::id(), daemon.id());
            std::io::stdout().flush().unwrap();
            if args[1] == "parent-flood" {
                loop {
                    std::io::stderr().write_all(&[b'x'; 8192]).unwrap();
                }
            }
            if args[1] == "parent-sleep" {
                std::thread::sleep(Duration::from_secs(10));
            }
        }
        "daemon" => {
            std::thread::sleep(Duration::from_millis(400));
            std::fs::write(&args[2], b"survived").unwrap();
        }
        _ => panic!("invalid fixture mode"),
    }
}
