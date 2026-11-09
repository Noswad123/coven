use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const FIXTURE_SHA: &str = "8309440254a20d5dc747d5f1b1b32acb2acfb25586e37260fb57a968533e7e6d";
const BASELINE_COMMIT: &str = "01d7cef6e98e2ccab216cd669d569c0731abfbce";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn explicit_path(key: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed={key}");
    let path = PathBuf::from(env::var_os(key).expect("explicit absolute input required"));
    assert!(path.is_absolute(), "{key} must be absolute");
    path
}

fn main() {
    let root = explicit_path("KITSUNE_CANDIDATE_ROOT");
    let fixture = explicit_path("KITSUNE_PROCESS_FIXTURE");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed={}", fixture.display());
    let bytes = fs::read(&fixture).expect("read baseline fixture");
    assert_eq!(sha(&bytes), FIXTURE_SHA, "frozen fixture SHA mismatch");
    let baseline: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(baseline["cases"].as_array().unwrap().len(), 209);
    let reference = &baseline["reference"];
    assert_eq!(reference["commit"], BASELINE_COMMIT);
    assert_eq!(reference["repository"], "kitsune");
    assert_eq!(reference["schema_version"], 1);
    assert_eq!(
        reference["harness"],
        "tools/harnesses/kitsune-process-reference"
    );
    for (file, path, blob, hash) in [
        (
            0,
            "src/detect/mod.rs",
            "7bbc2ca8113c76da70dcb17a007503811604de91",
            "dab5dd718de9afaebcc3b673a6e2ad967e451e6a20cd45281c47ed422158b339",
        ),
        (
            1,
            "src/platform/mod.rs",
            "5c168acb23d39efb43085d230c5221e2e753a882",
            "4e7c7a10ee386a75c7f9d0508b19f3080402e50dacab855180eae207d037fbd6",
        ),
        (
            2,
            "src/platform/windows.rs",
            "7f1008326f68bd0da6d31346c3357bfdb2c284ed",
            "c55571f5ff88c672b21944ece645084d7dac574db10340ac62ebf2bf9121af3d",
        ),
    ] {
        assert_eq!(reference["files"][file]["path"], path);
        assert_eq!(reference["files"][file]["git_blob"], blob);
        assert_eq!(reference["files"][file]["sha256"], hash);
    }
    fs::write(out.join("fixture.json"), bytes).unwrap();

    let mut files = Vec::new();
    for (path, output, ranges) in [
        (
            "src/detect/mod.rs",
            "detect.rs",
            vec![
                (
                    "/// Which agent we detected running in a pane.\n",
                    "\n/// Detect the state of an agent from the live terminal tail snapshot.\n",
                ),
                (
                    "#[cfg(test)]\nfn normalized_process_name",
                    "\n// ---------------------------------------------------------------------------\n// Tests\n",
                ),
            ],
        ),
        (
            "src/platform/mod.rs",
            "platform.rs",
            vec![(
                "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ForegroundProcess {\n",
                "\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum Signal {\n",
            )],
        ),
        (
            "src/platform/windows.rs",
            "windows.rs",
            vec![(
                "fn select_pane_foreground_job(shell_pid:",
                "\nfn snapshot_processes()",
            )],
        ),
        (
            "src/platform/process_selection.rs",
            "process_selection.rs",
            vec![],
        ),
        ("src/detect/resolver_tests.rs", "resolver_tests.rs", vec![]),
    ] {
        let supplied = root.join(path);
        println!("cargo:rerun-if-changed={}", supplied.display());
        let bytes = fs::read(&supplied).expect("read current candidate source");
        let text = std::str::from_utf8(&bytes).expect("UTF-8 candidate source");
        let mut spans = Vec::new();
        if ranges.is_empty() {
            spans.push((0, text.len(), "whole file", "EOF"));
        } else {
            for (start, end) in ranges {
                assert_eq!(text.matches(start).count(), 1, "nonunique start: {path}");
                assert_eq!(text.matches(end).count(), 1, "nonunique end: {path}");
                let begin = text.find(start).unwrap();
                let finish = text.find(end).unwrap();
                assert!(
                    begin < finish && finish - begin < 100_000,
                    "invalid bounded block"
                );
                assert!(begin == 0 || text.as_bytes()[begin - 1] == b'\n');
                spans.push((begin, finish, start, end));
            }
        }
        let mut compiled = String::new();
        let mut blocks = Vec::new();
        for (begin, finish, start, end) in spans {
            let source = &text[begin..finish];
            compiled.push_str(source);
            compiled.push('\n');
            blocks.push(json!({"byte_start":begin,"byte_end_exclusive":finish,
                "first_line":text[..begin].bytes().filter(|b| *b == b'\n').count()+1,
                "last_line":text[..finish].lines().count(),
                "start_marker":start,"end_marker":end,"sha256":sha(source.as_bytes())}));
        }
        if output == "detect.rs" {
            compiled.push_str("\n#[cfg(test)]\nmod resolver_tests { include!(concat!(env!(\"OUT_DIR\"), \"/resolver_tests.rs\")); }\n");
        }
        fs::write(out.join(output), compiled).unwrap();
        files.push(json!({"path":path,"sha256":sha(&bytes),"compiled_blocks":blocks}));
    }
    let metadata = json!({"candidate_root":root,"candidate":"current working-tree files, not pinned baseline",
        "scope":"source subset only; not a full Windows or Kitsune application build",
        "fixture_path":fixture,"fixture_sha256":FIXTURE_SHA,"case_count":209,
        "baseline":reference,"candidate_files":files});
    fs::write(
        out.join("provenance.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
}
