//! Compiled inert CLI fixture, never a real multiplexer/provider/application.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

unsafe extern "C" {
    fn isatty(fd: i32) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn raise(signal: i32) -> i32;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let root = std::path::Path::new(&args[2]);
    match mode {
        "stdio" => {
            let mut input = [0; 14];
            std::io::stdin().read_exact(&mut input).unwrap();
            assert_eq!(&input, b"literal-input\n");
            assert_eq!(std::env::var("ONLY").unwrap(), "explicit");
            assert_eq!(std::env::var("EXTRA").unwrap(), "also passed");
            assert!(std::env::var_os("FOREGROUND_AMBIENT").is_none());
            assert_eq!(
                std::env::current_dir().unwrap(),
                root.canonicalize().unwrap()
            );
            std::fs::write(
                root.join("mask"),
                format!("{} {}", unsafe { isatty(0) }, unsafe { isatty(1) }),
            )
            .unwrap();
            print!("foreground-stdout");
            eprint!("foreground-stderr");
        }
        "fds" => {
            for fd in &args[3..] {
                assert_eq!(unsafe { fcntl(fd.parse().unwrap(), 1) }, -1);
            }
            std::fs::write(root.join("image"), b"executed").unwrap();
        }
        "exit127" => {
            std::fs::write(root.join("image"), b"executed").unwrap();
            std::process::exit(127);
        }
        "signal" => unsafe {
            raise(15);
        },
        "large" => {
            std::io::stdout().write_all(&vec![b'x'; 1_048_577]).unwrap();
            std::io::stderr().write_all(b"uncaptured-stderr").unwrap();
        }
        "daemon" => {
            Command::new(std::env::current_exe().unwrap())
                .arg("effect")
                .arg(root)
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            std::fs::write(root.join("pid"), std::process::id().to_string()).unwrap();
        }
        "effect" => {
            std::thread::sleep(Duration::from_millis(400));
            std::fs::write(root.join("effect"), b"survived").unwrap();
        }
        "long" => {
            Command::new(std::env::current_exe().unwrap())
                .arg("effect")
                .arg(root)
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(Duration::from_secs(61));
            std::fs::write(root.join("long-effect"), b"past capture deadline").unwrap();
        }
        _ => panic!("unknown fixture mode"),
    }
}
