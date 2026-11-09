use std::{env, fs, path::PathBuf, process::Command};

const COMMIT: &str = "01d7cef6e98e2ccab216cd669d569c0731abfbce";
const BLOB: &str = "0c3b51bd19d73aa0ede31d6775cac35395447273";

fn main() {
    for key in ["KITSUNE_REFERENCE_ROOT", "KITSUNE_REFERENCE_SOURCE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let root = PathBuf::from(
        env::var_os("KITSUNE_REFERENCE_ROOT").expect("explicit KITSUNE_REFERENCE_ROOT required"),
    );
    let source = PathBuf::from(
        env::var_os("KITSUNE_REFERENCE_SOURCE")
            .expect("explicit KITSUNE_REFERENCE_SOURCE required"),
    );
    assert!(
        root.is_absolute() && source.is_absolute(),
        "reference paths must be absolute"
    );
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("--no-replace-objects")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("git");
        assert!(output.status.success(), "reference git verification failed");
        output.stdout
    };
    let object = format!("{COMMIT}:src/agent_resume.rs");
    assert_eq!(
        String::from_utf8(git(&["rev-parse", &object]))
            .unwrap()
            .trim(),
        BLOB
    );
    let bytes = fs::read(&source).expect("read explicit reference source");
    assert_eq!(
        bytes,
        git(&["show", &object]),
        "source differs from pinned reference blob"
    );
    println!("cargo:rerun-if-changed={}", source.display());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("agent_resume.rs"), bytes).unwrap();
}
