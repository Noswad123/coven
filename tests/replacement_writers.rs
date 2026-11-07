#[cfg(unix)]
mod unix {
    use coven::storage::{Delivery, WorkspaceGuard, atomic_replace_bytes};
    use std::{
        io::{self, BufRead, Write},
        path::Path,
        time::Duration,
    };

    // Subprocess-only test protocol, not a production CLI or failpoint.
    #[test]
    fn native_replacement_probe() {
        let Ok(root) = std::env::var("COVEN_REPLACEMENT_PROBE") else {
            return;
        };
        let guard = match WorkspaceGuard::acquire(Path::new(&root), Duration::ZERO) {
            Ok(guard) => guard,
            Err(error) if error.code() == "workspace_lock_busy" => {
                println!("BUSY");
                io::stdout().flush().unwrap();
                return;
            }
            Err(error) => panic!("native replacement lock: {error}"),
        };
        println!("LOCKED");
        io::stdout().flush().unwrap();
        for line in io::stdin().lock().lines() {
            let line = line.unwrap();
            if line == "release" {
                break;
            }
            let index: u32 = line.parse().unwrap();
            let mut bytes = format!("R{index:06}:").into_bytes();
            bytes.extend(std::iter::repeat_n(b'R', 262_144));
            assert_eq!(
                atomic_replace_bytes(&guard, Path::new("state/snapshot"), &bytes).unwrap(),
                Delivery::Committed
            );
            println!("REPLACED {index}");
            io::stdout().flush().unwrap();
        }
    }

    #[test]
    fn python_and_native_replacement_writers_share_lock_and_whole_snapshots() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let result = std::process::Command::new("python3")
            .arg(root.join("tests/_replacement_writers.py"))
            .arg("--probe")
            .arg(std::env::current_exe().unwrap())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        println!("{}", String::from_utf8_lossy(&result.stdout));
    }
}
