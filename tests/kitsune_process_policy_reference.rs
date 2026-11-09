//! Frozen characterization, not a replacement classifier or a Coven parity claim.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const BYTES: &[u8] = include_bytes!("fixtures/kitsune_process_policy/reference.json");
const AGENTS: [&str; 22] = [
    "pi",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "devin",
    "agy",
    "cline",
    "omp",
    "mastracode",
    "opencode",
    "copilot",
    "kimi",
    "kiro",
    "droid",
    "amp",
    "grok",
    "hermes",
    "kilo",
    "qodercli",
    "maki",
    "djinn",
];

fn fixture() -> Value {
    serde_json::from_slice(BYTES).unwrap()
}
fn case<'a>(f: &'a Value, name: &str) -> &'a Value {
    f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}
fn agent(c: &Value) -> &Value {
    &c["identify_agent_in_job"]["agent"]
}
fn selected(c: &Value) -> &Value {
    &c["windows_selected_job"]["process_group_id"]
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

fn valid_pid(value: &Value, allow_zero: bool) -> bool {
    value
        .as_u64()
        .is_some_and(|pid| pid <= u32::MAX.into() && (allow_zero || pid > 0))
}

fn valid_process(value: &Value, entry: bool) -> bool {
    let keys: &[&str] = if entry {
        &["pid", "parent_pid", "name", "argv0", "argv", "cmdline"]
    } else {
        &["pid", "name", "argv0", "argv", "cmdline"]
    };
    exact_keys(value, keys)
        && valid_pid(&value["pid"], false)
        && (!entry || valid_pid(&value["parent_pid"], true))
        && value["name"].is_string()
        && ["argv0", "cmdline"]
            .iter()
            .all(|key| value[key].is_null() || value[key].is_string())
        && (value["argv"].is_null()
            || value["argv"]
                .as_array()
                .is_some_and(|args| args.iter().all(Value::is_string)))
}

fn valid_identification(value: &Value) -> bool {
    value.is_null()
        || (exact_keys(value, &["agent", "candidate"])
            && value["agent"]
                .as_str()
                .is_some_and(|agent| AGENTS.contains(&agent))
            && value["candidate"].is_string())
}

fn valid_record(value: &Value) -> bool {
    if !exact_keys(
        value,
        &[
            "name",
            "root_pid",
            "entries",
            "identify_agent_in_job",
            "windows_selected_job",
        ],
    ) || !value["name"].is_string()
        || !valid_pid(&value["root_pid"], false)
        || !valid_identification(&value["identify_agent_in_job"])
    {
        return false;
    }
    let Some(entries) = value["entries"].as_array() else {
        return false;
    };
    let mut pids = HashSet::new();
    if !entries
        .iter()
        .all(|entry| valid_process(entry, true) && pids.insert(entry["pid"].as_u64().unwrap()))
    {
        return false;
    }
    let job = &value["windows_selected_job"];
    if job.is_null() {
        return true;
    }
    if !exact_keys(job, &["process_group_id", "processes", "identification"])
        || !valid_pid(&job["process_group_id"], false)
        || !valid_identification(&job["identification"])
    {
        return false;
    }
    let Some(processes) = job["processes"].as_array() else {
        return false;
    };
    // Structural preservation only: missing parents, cycles and absent roots are valid inputs.
    processes.len() == 1
        && valid_process(&processes[0], false)
        && processes[0]["pid"] == job["process_group_id"]
        && entries.iter().any(|entry| {
            ["pid", "name", "argv0", "argv", "cmdline"]
                .iter()
                .all(|key| entry[key] == processes[0][key])
        })
}

#[test]
fn all_records_have_strict_schema_independent_of_checksum() {
    let f = fixture();
    let cases = f["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 209);
    for record in cases {
        assert!(valid_record(record), "invalid record: {}", record["name"]);
    }
}

#[test]
fn malformed_record_mutations_are_rejected_without_checksum() {
    let f = fixture();
    let empty = case(&f, "empty");
    let direct = case(&f, "direct/djinn");
    let mut mutations = Vec::new();
    let mut replace = |base: &Value, path: &str, replacement: Value| {
        let mut changed = base.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        mutations.push(changed);
    };
    for bad in [
        json!("10"),
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(4294967296u64),
        json!(10.5),
        json!([]),
        json!({}),
    ] {
        replace(empty, "/root_pid", bad.clone());
        replace(direct, "/entries/0/pid", bad.clone());
        replace(
            direct,
            "/windows_selected_job/process_group_id",
            bad.clone(),
        );
        replace(direct, "/windows_selected_job/processes/0/pid", bad);
    }
    for bad in [
        json!("0"),
        Value::Null,
        json!(false),
        json!(-1),
        json!(4294967296u64),
        json!(0.5),
    ] {
        replace(direct, "/entries/0/parent_pid", bad);
    }
    for prefix in ["/entries/0", "/windows_selected_job/processes/0"] {
        for (field, bad) in [
            ("name", Value::Null),
            ("name", json!(7)),
            ("argv0", json!([])),
            ("argv0", json!(true)),
            ("argv", json!("djinn")),
            ("argv", json!(["djinn", 7])),
            ("cmdline", json!({})),
            ("cmdline", json!(0)),
        ] {
            replace(direct, &format!("{prefix}/{field}"), bad);
        }
    }
    for path in [
        "/identify_agent_in_job",
        "/windows_selected_job/identification",
    ] {
        for bad in [
            json!(false),
            json!([]),
            json!({}),
            json!({"agent":"unknown","candidate":"djinn"}),
            json!({"agent":7,"candidate":"djinn"}),
            json!({"agent":"djinn","candidate":null}),
            json!({"agent":"djinn"}),
            json!({"candidate":"djinn"}),
            json!({"agent":"djinn","candidate":"djinn","extra":true}),
        ] {
            replace(direct, path, bad);
        }
    }
    for (path, bad) in [
        ("/name", json!(false)),
        ("/entries", Value::Null),
        ("/entries", json!({})),
        (
            "/entries",
            json!([direct["entries"][0].clone(), direct["entries"][0].clone()]),
        ),
        ("/windows_selected_job", json!(true)),
        ("/windows_selected_job", json!([])),
        ("/windows_selected_job/processes", Value::Null),
        ("/windows_selected_job/processes", json!({})),
        ("/windows_selected_job/processes", json!([])),
        (
            "/windows_selected_job/processes",
            json!([
                direct["windows_selected_job"]["processes"][0].clone(),
                direct["windows_selected_job"]["processes"][0].clone()
            ]),
        ),
        ("/windows_selected_job/process_group_id", json!(99)),
        ("/windows_selected_job/processes/0/name", json!("altered")),
        ("/windows_selected_job/processes/0/argv0", json!("altered")),
        ("/windows_selected_job/processes/0/argv", json!(["altered"])),
        (
            "/windows_selected_job/processes/0/cmdline",
            json!("altered"),
        ),
    ] {
        replace(direct, path, bad);
    }
    // Exercise missing required fields (including nullable ones) and unknown keys at every level.
    for path in [
        "",
        "/entries/0",
        "/identify_agent_in_job",
        "/windows_selected_job",
        "/windows_selected_job/processes/0",
        "/windows_selected_job/identification",
    ] {
        let object = direct.pointer(path).unwrap().as_object().unwrap();
        for key in object.keys() {
            let mut changed = direct.clone();
            changed
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            mutations.push(changed);
        }
        let mut changed = direct.clone();
        changed
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        mutations.push(changed);
    }
    let mut parent_in_selected = direct.clone();
    parent_in_selected["windows_selected_job"]["processes"][0]["parent_pid"] = json!(0);
    mutations.push(parent_in_selected);
    let mut absent_selected_entry = direct.clone();
    absent_selected_entry["windows_selected_job"]["process_group_id"] = json!(99);
    absent_selected_entry["windows_selected_job"]["processes"][0]["pid"] = json!(99);
    mutations.push(absent_selected_entry);
    assert_eq!(mutations.len(), 122);
    for (index, changed) in mutations.iter().enumerate() {
        assert!(
            !valid_record(changed),
            "accepted mutation {index}: {changed}"
        );
    }
}

#[test]
fn frozen_bytes_and_source_provenance_integrity() {
    assert_eq!(
        format!("{:x}", Sha256::digest(BYTES)),
        "8309440254a20d5dc747d5f1b1b32acb2acfb25586e37260fb57a968533e7e6d"
    );
    let f = fixture();
    let r = &f["reference"];
    assert_eq!(r["commit"], "01d7cef6e98e2ccab216cd669d569c0731abfbce");
    assert_eq!(r["repository"], "kitsune");
    assert_eq!(r["schema_version"], 1);
    assert_eq!(r["harness"], "tools/harnesses/kitsune-process-reference");
    assert!(r["filesystem"].as_str().unwrap().starts_with("not pure:"));
    assert!(
        r["coven_equivalent"]
            .as_str()
            .unwrap()
            .contains("No public pure observer classification API")
    );
    let files = r["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    for (file, (path, blob, sha, ranges)) in files.iter().zip([
        (
            "src/detect/mod.rs",
            "7bbc2ca8113c76da70dcb17a007503811604de91",
            "dab5dd718de9afaebcc3b673a6e2ad967e451e6a20cd45281c47ed422158b339",
            vec![
                (41, 243,
                 "/// Which agent we detected running in a pane.\n",
                 "\n/// Detect the state of an agent from the live terminal tail snapshot.\n",
                 "9113be7447b3160bd0c740ac95075b965d123d07d77d38549bbd24d7056efa6b"),
                (329, 614,
                 "fn normalized_process_name(process: &crate::platform::ForegroundProcess) -> String {\n",
                 "\n// ---------------------------------------------------------------------------\n// Tests\n",
                 "517b80cec6dbd77bf6564f611e4ff6ef38088820e139816841a843c581c18da7"),
            ],
        ),
        (
            "src/platform/mod.rs",
            "5c168acb23d39efb43085d230c5221e2e753a882",
            "4e7c7a10ee386a75c7f9d0508b19f3080402e50dacab855180eae207d037fbd6",
            vec![(6, 19,
                  "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ForegroundProcess {\n",
                  "\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum Signal {\n",
                  "251539509b6ea83fd6f449f066e8a95a9d8e45f19e18be935b2a70e71db431b7")],
        ),
        (
            "src/platform/windows.rs",
            "7f1008326f68bd0da6d31346c3357bfdb2c284ed",
            "c55571f5ff88c672b21944ece645084d7dac574db10340ac62ebf2bf9121af3d",
            vec![
                (77, 85,
                 "#[derive(Debug, Clone, PartialEq, Eq)]\nstruct WindowsProcessEntry {\n",
                 "\npub fn raise_server_nofile_limit() {}\n",
                 "099edcf9593e646eae87f32f37937eed6b2d82cccc0a6272c097c37b1a7b0f98"),
                (292, 415,
                 "fn select_pane_foreground_job(\n",
                 "\nfn snapshot_processes() -> Vec<WindowsProcessEntry> {\n",
                 "c2adf1abbfb5642309209583a37d770b8ed55a9446381c717dd2e1a2d5afbf22"),
            ],
        ),
    ]) {
        assert_eq!(file["path"], path);
        assert_eq!(file["git_blob"], blob);
        assert_eq!(file["sha256"], sha);
        let blocks = file["compiled_blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), ranges.len());
        for (block, (first, last, start, end, sha)) in blocks.iter().zip(ranges) {
            assert_eq!(
                block,
                &json!({"first_line":first,"last_line":last,"start_marker":start,
                    "end_marker":end,"sha256":sha}),
                "reviewed block metadata only; no embedded source"
            );
        }
    }
    let cases = f["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 209);
    let names: HashSet<_> = cases.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), cases.len());
}

#[test]
fn all_provider_labels_aliases_and_suffixes() {
    let f = fixture();
    for label in AGENTS {
        let direct = case(&f, &format!("direct/{label}"));
        assert_eq!(agent(direct), label);
        assert_eq!(selected(direct), 10);
        let child = case(&f, &format!("descendant/{label}"));
        assert_eq!(agent(child), label);
        assert_eq!(selected(child), 20);
        assert_eq!(
            child["windows_selected_job"]["identification"]["agent"],
            label
        );
    }
    let aliases = [
        ("claude-code", "claude"),
        ("cursor-agent", "cursor"),
        ("devin-cli", "devin"),
        ("devin cli", "devin"),
        ("antigravity", "agy"),
        ("antigravity-cli", "agy"),
        ("mastra-code", "mastracode"),
        ("mastra code", "mastracode"),
        ("open-code", "opencode"),
        ("github-copilot", "copilot"),
        ("ghcs", "copilot"),
        ("kimi-code", "kimi"),
        ("kimi code", "kimi"),
        ("kiro-cli", "kiro"),
        ("amp-local", "amp"),
        ("grok-build", "grok"),
        ("hermes-agent", "hermes"),
        ("kilo-code", "kilo"),
        ("kilo code", "kilo"),
        ("qoderclicn", "qodercli"),
        ("qoder", "qodercli"),
        ("qodercn", "qodercli"),
        ("djinn-ui", "djinn"),
    ];
    for (alias, label) in aliases {
        assert_eq!(agent(case(&f, &format!("alias/{alias}"))), label);
    }
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
        let c = case(&f, &format!("suffix/{suffix}"));
        assert_eq!(agent(c), "djinn");
        assert_eq!(
            c["identify_agent_in_job"]["candidate"],
            format!("DJINN{suffix}")
        );
    }
}

#[test]
fn wrappers_modes_nonclaims_and_filesystem_behavior() {
    let f = fixture();
    for (name, label) in [
        ("node-script", "codex"),
        ("node-value-option", "codex"),
        ("bun-script", "omp"),
        ("python-script", "codex"),
        ("python-script-option", "codex"),
        ("shell-script", "pi"),
        ("bash-script", "codex"),
        ("zsh-script", "codex"),
        ("fish-script", "codex"),
        ("cmd-command", "codex"),
        ("cmd-keep", "codex"),
        ("powershell-file", "claude"),
        ("pwsh-command", "djinn"),
        ("pwsh-positional", "djinn"),
        ("package-cli", "pi"),
        ("controlled-symlink", "cursor"),
        ("shell-login-combined", "codex"),
    ] {
        assert_eq!(agent(case(&f, &format!("wrapper/{name}"))), label, "{name}");
    }
    for name in [
        "node-eval",
        "node-inline-eval",
        "node-long-eval",
        "node-print",
        "bun-eval",
        "python-command",
        "python-module",
        "shell-command",
        "powershell-encoded",
        "powershell-nonagent-command",
        "tmux-nonclaim",
        "unknown-script",
        "package-non-cli",
        "controlled-missing-path",
    ] {
        assert!(
            case(&f, &format!("wrapper/{name}"))["identify_agent_in_job"].is_null(),
            "{name}"
        );
    }
    for mode in ["--help", "--version", "serve", "config", "maintenance"] {
        assert_eq!(
            agent(case(&f, &format!("mode-not-excluded/{mode}"))),
            "djinn"
        );
    }
}

#[test]
fn argv0_name_argv_and_cmdline_precedence() {
    let f = fixture();
    for (name, label, candidate) in [
        ("name-only", "djinn", "djinn"),
        ("argv0-wins", "djinn", "djinn"),
        ("argv-fallback", "djinn", "djinn"),
        ("cmdline-fallback", "copilot", "copilot"),
        ("argv-before-cmdline", "codex", "codex"),
        ("empty-argv-cmdline", "djinn", "djinn"),
        ("unrecognized-argv-cmdline", "djinn", "djinn"),
        ("trim-case", "djinn", " DJINN "),
    ] {
        assert_eq!(
            case(&f, &format!("fallback/{name}"))["identify_agent_in_job"],
            json!({"agent":label,"candidate":candidate})
        );
    }
    // cmdline fallback is whitespace splitting, not shell/Windows argv parsing.
    assert_eq!(
        agent(case(&f, "fallback/quoted-cmdline-first-token")),
        "djinn"
    );
    assert!(case(&f, "fallback/argv0-masks-name")["identify_agent_in_job"].is_null());
}

#[test]
fn native_windows_agent_aware_selection_across_every_permutation() {
    let f = fixture();
    for name in ["empty", "root-absent"] {
        assert!(case(&f, name)["windows_selected_job"].is_null());
    }
    assert_eq!(
        agent(case(&f, "root-absent")),
        "codex",
        "classification does not require a root entry"
    );
    assert_eq!(selected(case(&f, "root-plain")), 10);
    for (name, pid, count) in [
        ("plain-descendant", 10, 2),
        ("same-chain", 20, 6),
        ("same-chain-intermediary", 20, 24),
        ("siblings", 10, 6),
        ("mixed-nested", 10, 6),
        ("mixed-siblings", 10, 6),
        ("unrelated", 20, 6),
        ("missing-parent", 10, 2),
        ("disconnected-cycle", 10, 6),
        ("root-cycle", 20, 6),
        ("self-parent", 10, 2),
        ("root-agent-descendant-other", 20, 2),
        ("pgid-leader", 20, 2),
        ("priority-wrapped", 10, 6),
        ("priority-tie", 10, 6),
    ] {
        let prefix = format!("tree/{name}/");
        let variants: Vec<_> = f["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["name"].as_str().unwrap().starts_with(&prefix))
            .collect();
        assert_eq!(variants.len(), count);
        let first = &variants[0]["windows_selected_job"];
        for c in variants {
            assert_eq!(selected(c), pid, "{}", c["name"]);
            assert_eq!(
                &c["windows_selected_job"], first,
                "selection must be input-order invariant for unique-PID trees"
            );
        }
    }
}

#[test]
fn pgid_priority_and_input_order_ties_are_separate_from_windows_selection() {
    let f = fixture();
    for c in f["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        if name.starts_with("tree/pgid-leader/") {
            assert_eq!(agent(c), "claude");
        }
        if name.starts_with("tree/root-agent-descendant-other/") {
            assert_eq!(agent(c), "djinn");
        }
        if name.starts_with("tree/priority-wrapped/") {
            assert_eq!(agent(c), "djinn");
        }
        if name.starts_with("tree/priority-tie/") {
            let first = c["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["pid"] != 10)
                .unwrap();
            assert_eq!(
                agent(c),
                &first["name"],
                "equal scores preserve input order"
            );
        }
    }
}
