//! Controlled native fixture: no provider, interpreter, or library dependencies.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn getppid() -> i32;
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args[0].as_str() {
        "descriptors" => {
            // F_GETFD is 1 on the supported Linux/macOS targets. Unlike a /proc
            // directory walk, this creates no observation descriptor of its own.
            for fd in 3..4096 {
                assert!(unsafe { fcntl(fd, 1) } < 0, "unexpected inherited fd {fd}");
            }
            let mut input = Vec::new();
            std::io::stdin().read_to_end(&mut input).unwrap();
            assert!(input.is_empty());
            print!("only-stdio");
        }
        "delayed-output" => {
            std::io::stdout().write_all(b"prefix-").unwrap();
            std::io::stderr().write_all(b"error-").unwrap();
            Command::new(std::env::current_exe().unwrap())
                .args(["suffix-after-leader", &std::process::id().to_string()])
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            std::process::exit(7);
        }
        "suffix-after-leader" => {
            let leader: i32 = args[1].parse().unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while unsafe { getppid() } == leader {
                assert!(Instant::now() < deadline, "leader did not exit");
                std::thread::sleep(Duration::from_millis(1));
            }
            std::thread::sleep(Duration::from_millis(120));
            std::io::stdout().write_all(b"suffix").unwrap();
            std::io::stderr().write_all(b"suffix").unwrap();
        }
        _ => panic!("unknown fixture mode"),
    }
}
