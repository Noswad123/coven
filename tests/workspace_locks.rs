#[cfg(unix)]
mod unix {
    use coven::storage::WorkspaceGuard;
    use std::{
        fs,
        io::{self, Write},
        time::Duration,
    };

    #[test]
    fn native_lock_probe() {
        let Ok(root) = std::env::var("COVEN_NATIVE_LOCK_PROBE") else {
            return;
        };
        match WorkspaceGuard::acquire(std::path::Path::new(&root), Duration::ZERO) {
            Ok(_guard) => {
                println!("LOCKED");
                io::stdout().flush().unwrap();
                let mut release = String::new();
                io::stdin().read_line(&mut release).unwrap();
            }
            Err(error) if error.code() == "workspace_lock_busy" => {
                println!("BUSY");
                io::stdout().flush().unwrap();
            }
            Err(error) => panic!("unexpected native lock failure: {error}"),
        }
    }

    #[test]
    fn existing_lock_does_not_create_and_wrong_root_tokens_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("coven.json"), "marker").unwrap();
        assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
        assert!(!root.path().join("state").exists());
        let guard = WorkspaceGuard::acquire(root.path(), Duration::ZERO).unwrap();
        guard.validate_for(root.path()).unwrap();
        let other = tempfile::tempdir().unwrap();
        assert!(guard.validate_for(other.path()).is_err());
        assert!(WorkspaceGuard::existing(root.path(), Duration::ZERO).is_err());
        drop(guard);
        let again = WorkspaceGuard::existing(root.path(), Duration::ZERO).unwrap();
        assert_eq!(again.root(), root.path().canonicalize().unwrap());
    }

    #[test]
    fn managed_symlinks_cannot_redirect_native_lock_creation() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(root.path().join("coven.json"), "marker").unwrap();
        symlink(outside.path(), root.path().join("state")).unwrap();
        assert!(WorkspaceGuard::acquire(root.path(), Duration::ZERO).is_err());
        assert!(!outside.path().join(".workspace.lock").exists());
    }

    #[test]
    fn python_and_native_share_one_persistent_kernel_lock() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let result = std::process::Command::new("python3")
            .arg(root.join("tests/_lock_parity.py"))
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
