//! Test-only, hash-pinned historical bytes. No oracle process or shell parser.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const AGENTS: [&str; 13] = [
    "orchestrator",
    "architect",
    "implementer",
    "reviewer",
    "clippy",
    "srikanth",
    "jamal",
    "kris",
    "eric",
    "CLIPPY",
    "CUSTOM",
    "caf\u{e9} \u{1f600}",
    "quote\"\\\n\r\t\u{8}\u{c}\0\u{1f}\u{7f}",
];

fn keys(value: &Value, expected: &[&str]) {
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        expected.iter().copied().collect::<BTreeSet<_>>()
    );
}

fn expand(value: &mut Value, texts: &[String], used: &mut BTreeSet<usize>) {
    match value {
        Value::Object(object) if object.contains_key("$text") => {
            assert_eq!(object.len(), 1);
            let index = usize::try_from(object["$text"].as_u64().unwrap()).unwrap();
            used.insert(index);
            *value = Value::String(texts[index].clone());
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                expand(value, texts, used);
            }
        }
        Value::Array(array) => {
            for value in array {
                expand(value, texts, used);
            }
        }
        Value::String(_) => panic!("unencoded fixture string"),
        _ => {}
    }
}

fn load(name: &str, raw: &[u8]) -> Value {
    // Do not mutate process-global environment in parallel tests. Callers run
    // with both selectors unset, so native comparison uses the embedded demo.
    for key in ["COVEN_EXAMPLES_DIR", "COVEN_HOME"] {
        assert!(
            std::env::var_os(key).is_none(),
            "unset {key} for embedded asset parity"
        );
    }
    let provenance =
        include_bytes!("../../tests/retirement/fixtures/workspace_assets_provenance.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(provenance)),
        "1d434b77e13bf773069e7f41950d875b345266102d769ebf2313af085a04ea2e"
    );
    let provenance: Value = serde_json::from_slice(provenance).unwrap();
    assert_eq!(
        provenance["oracle_commit"],
        "8e41fe10ab041b4355205688e3458d5b8906c80c"
    );
    assert_eq!(provenance["source_sha256"].as_object().unwrap().len(), 39);
    assert_eq!(
        provenance["inventory"],
        json!({
            "assets": 22, "launches": 44, "teams": 48, "agents": 13,
            "defaults": 3, "checkpoints": 2, "example_assets": 7
        })
    );
    assert_eq!(
        raw.len() as u64,
        provenance["fixtures"][name]["bytes"].as_u64().unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(raw)),
        provenance["fixtures"][name]["sha256"]
    );
    let mut packed: Value = serde_json::from_slice(raw).unwrap();
    keys(&packed, &["lines", "texts", "data"]);
    let lines = packed["lines"].as_array().unwrap();
    let mut used_lines = BTreeSet::new();
    let texts: Vec<String> = packed["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|text| {
            text.as_array()
                .unwrap()
                .iter()
                .map(|index| {
                    let index = usize::try_from(index.as_u64().unwrap()).unwrap();
                    used_lines.insert(index);
                    lines[index].as_str().unwrap()
                })
                .collect()
        })
        .collect();
    assert_eq!(used_lines, (0..lines.len()).collect());
    let mut used_texts = BTreeSet::new();
    expand(&mut packed["data"], &texts, &mut used_texts);
    assert_eq!(used_texts, (0..texts.len()).collect());
    packed["data"].take()
}

pub(super) fn assets() -> Value {
    let expected = load(
        "workspace_assets.json",
        include_bytes!("../../tests/retirement/fixtures/workspace_assets.json"),
    );
    let cases = expected.as_array().unwrap();
    assert_eq!(cases.len(), 22);
    let commands = [
        "oc",
        " oc --model 'a b' ",
        "oc\t--x",
        "oc\n--x",
        "oc  ",
        "opencode --flag",
        "\u{1c}oc\u{1f}",
        "oc $VAR; printf 'x'",
    ];
    let roots = ["/tmp/blank", "/tmp/caf\u{e9} \u{1f600}/a\"\u{7f}"];
    let mut ids = BTreeSet::new();
    for (index, agent) in AGENTS[..11].iter().enumerate() {
        for (variant, example) in [false, true].into_iter().enumerate() {
            let row = index * 2 + variant;
            let case = &cases[row];
            keys(
                case,
                &[
                    "root",
                    "agent",
                    "command",
                    "cli",
                    "example",
                    "role",
                    "prompt",
                    "starter",
                    "normalized",
                    "shell_quote",
                    "manifest",
                    "launches",
                    "case_id",
                ],
            );
            assert_eq!(case["case_id"], format!("asset-{row:02}"));
            assert!(ids.insert(case["case_id"].as_str().unwrap()));
            assert_eq!(case["agent"], *agent);
            assert_eq!(case["root"], roots[index % 2]);
            assert_eq!(case["command"], commands[index % commands.len()]);
            assert_eq!(
                case["cli"],
                if index % 2 == 0 {
                    "/tmp/a'b caf\u{e9}/coven"
                } else {
                    "coven"
                }
            );
            assert_eq!(case["example"], example);
            keys(&case["launches"], &["tmux", "herdr"]);
            for field in [
                "role",
                "prompt",
                "starter",
                "normalized",
                "shell_quote",
                "manifest",
            ] {
                assert!(case[field].is_string());
            }
            for backend in ["tmux", "herdr"] {
                assert!(case["launches"][backend].is_string());
            }
        }
    }
    assert_eq!(ids.len(), 22);
    expected
}

pub(in crate::workspace_assets) fn init() -> Value {
    let expected = load(
        "workspace_init.json",
        include_bytes!("../../tests/retirement/fixtures/workspace_init.json"),
    );
    keys(
        &expected,
        &["teams", "agents", "defaults", "checkpoints", "example"],
    );
    let teams = expected["teams"].as_array().unwrap();
    assert_eq!(teams.len(), 48);
    let mut index = 0;
    let mut ids = BTreeSet::new();
    for roster in [&[][..], &AGENTS[..], &["clippy"][..], &["CUSTOM"][..]] {
        for name in [
            "",
            "A coven",
            "A 'name' \" \\ caf\u{e9} \u{1f600}\u{7f}\n\r\t\u{8}\u{c}\0\u{1f}",
        ] {
            for multiplexer in ["herdr", "tmux"] {
                for display_variant in [false, true] {
                    let case = &teams[index];
                    keys(
                        case,
                        &[
                            "name",
                            "agents",
                            "command",
                            "multiplexer",
                            "display_names",
                            "text",
                            "case_id",
                        ],
                    );
                    assert_eq!(case["case_id"], format!("team-{index:02}"));
                    assert!(ids.insert(case["case_id"].as_str().unwrap()));
                    assert_eq!(case["name"], name);
                    assert_eq!(case["agents"], json!(roster));
                    assert_eq!(
                        case["command"],
                        " oc --model \"caf\u{e9} \u{1f600}\" \\ \n\t\u{7f}"
                    );
                    assert_eq!(case["multiplexer"], multiplexer);
                    let names: serde_json::Map<String, Value> = if display_variant {
                        roster
                            .iter()
                            .enumerate()
                            .map(|(i, agent)| {
                                (
                                    (*agent).into(),
                                    json!(if i % 2 == 0 {
                                        format!("Display {agent} \u{1f600}")
                                    } else {
                                        String::new()
                                    }),
                                )
                            })
                            .collect()
                    } else {
                        serde_json::Map::new()
                    };
                    assert_eq!(case["display_names"], Value::Object(names));
                    assert!(case["text"].is_string());
                    index += 1;
                }
            }
        }
    }
    assert_eq!(index, 48);
    assert_eq!(ids.len(), 48);
    // The six empty-roster pairs must remain, even though their bytes coincide.
    for index in (0..12).step_by(2) {
        assert_eq!(teams[index]["text"], teams[index + 1]["text"]);
        assert_ne!(teams[index]["case_id"], teams[index + 1]["case_id"]);
    }
    keys(&expected["agents"], &AGENTS);
    assert_eq!(expected["agents"].as_object().unwrap().len(), 13);
    keys(&expected["defaults"], &["goal", "work", "output"]);
    keys(
        &expected["checkpoints"],
        &[
            "checkpoints/plan-approval.md",
            "checkpoints/pre-delivery-review.md",
        ],
    );
    keys(
        &expected["example"],
        &[
            "id",
            "seed_message",
            "goal",
            "spec",
            "work",
            "output",
            "context",
            "tasks",
        ],
    );
    assert_eq!(expected["example"]["tasks"].as_array().unwrap().len(), 6);
    expected
}
