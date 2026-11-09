use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

const COMMIT: &str = "01d7cef6e98e2ccab216cd669d569c0731abfbce";

fn main() {
    println!("cargo:rerun-if-env-changed=KITSUNE_REFERENCE_ROOT");
    let root =
        PathBuf::from(env::var_os("KITSUNE_REFERENCE_ROOT").expect("explicit reference root"));
    assert!(root.is_absolute(), "reference root must be absolute");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .arg("--no-replace-objects")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(result.status.success(), "git object verification failed");
        result.stdout
    };
    let mut files = Vec::new();
    for (key, path, blob, output, ranges) in [
        (
            "DETECT",
            "src/detect/mod.rs",
            "7bbc2ca8113c76da70dcb17a007503811604de91",
            "detect.rs",
            vec![
                (
                    41,
                    243,
                    "/// Which agent we detected running in a pane.\n",
                    "\n/// Detect the state of an agent from the live terminal tail snapshot.\n",
                ),
                (
                    329,
                    614,
                    "fn normalized_process_name(process: &crate::platform::ForegroundProcess) -> String {\n",
                    "\n// ---------------------------------------------------------------------------\n// Tests\n",
                ),
            ],
        ),
        (
            "PLATFORM",
            "src/platform/mod.rs",
            "5c168acb23d39efb43085d230c5221e2e753a882",
            "platform.rs",
            vec![(
                6,
                19,
                "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ForegroundProcess {\n",
                "\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum Signal {\n",
            )],
        ),
        (
            "WINDOWS",
            "src/platform/windows.rs",
            "7f1008326f68bd0da6d31346c3357bfdb2c284ed",
            "windows.rs",
            vec![
                (
                    77,
                    85,
                    "#[derive(Debug, Clone, PartialEq, Eq)]\nstruct WindowsProcessEntry {\n",
                    "\npub fn raise_server_nofile_limit() {}\n",
                ),
                (
                    292,
                    415,
                    "fn select_pane_foreground_job(\n",
                    "\nfn snapshot_processes() -> Vec<WindowsProcessEntry> {\n",
                ),
            ],
        ),
    ] {
        let variable = format!("KITSUNE_REFERENCE_{key}");
        println!("cargo:rerun-if-env-changed={variable}");
        let supplied =
            PathBuf::from(env::var_os(&variable).expect("explicit source file required"));
        assert!(supplied.is_absolute(), "source file must be absolute");
        let object = format!("{COMMIT}:{path}");
        assert_eq!(
            String::from_utf8(git(&["rev-parse", &object]))
                .unwrap()
                .trim(),
            blob
        );
        let bytes = fs::read(&supplied).expect("read supplied source");
        assert_eq!(
            bytes,
            git(&["show", &object]),
            "supplied source differs from pinned Git blob"
        );
        println!("cargo:rerun-if-changed={}", supplied.display());
        let text = std::str::from_utf8(&bytes).unwrap();
        let mut compiled = String::new();
        let mut blocks = Vec::new();
        for (first, last, start, end) in ranges {
            assert_eq!(text.matches(start).count(), 1, "nonunique start boundary");
            assert_eq!(text.matches(end).count(), 1, "nonunique end boundary");
            let begin = text.find(start).unwrap();
            let finish = text.find(end).unwrap();
            assert!(begin < finish);
            assert_eq!(
                text[..begin].bytes().filter(|b| *b == b'\n').count() + 1,
                first
            );
            assert_eq!(text[..finish].bytes().filter(|b| *b == b'\n').count(), last);
            let source = &text[begin..finish];
            compiled.push_str(source);
            compiled.push('\n');
            blocks.push(
                json!({"first_line":first,"last_line":last,"start_marker":start,
                "end_marker":end,"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}),
            );
        }
        fs::write(out.join(output), compiled).unwrap();
        files.push(
            json!({"path":path,"git_blob":blob,"sha256":format!("{:x}",Sha256::digest(&bytes)),
            "compiled_blocks":blocks}),
        );
    }
    let provenance = json!({"repository":"kitsune","commit":COMMIT,"schema_version":1,
        "harness":"tools/harnesses/kitsune-process-reference","execution":"Linux; unchanged source blocks in OUT_DIR; synthetic data only",
        "filesystem":"not pure: canonicalize is reachable; private tempfile cwd; relative controlled paths only; no external symlinks",
        "coven_equivalent":"No public pure observer classification API; private exact_posix_argv0_basename remains unchanged; no broader recognition parity claimed",
        "files":files});
    fs::write(
        out.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance).unwrap(),
    )
    .unwrap();
}
