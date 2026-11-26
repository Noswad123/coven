use super::*;
use serde_json::{Value as Json, json};

const CASES: [&str; 5] = [
    "complete",
    "version",
    "missing",
    "missing-failure",
    "missing-parse",
];

fn corpus() -> Json {
    let raw = include_bytes!("../../../tests/retirement/fixtures/worker_readiness.json");
    let provenance =
        include_bytes!("../../../tests/retirement/fixtures/worker_readiness_provenance.json");
    assert_eq!(
        digest(raw),
        "d22a5b3f2ec8b056e60a65dcdc97754e3e4d4fbd5ee11ec33f33d74772b29f71"
    );
    assert_eq!(
        digest(provenance),
        "65ad6c1c11e40dd10e71efdf217ac556e7dede56cfbf479c902982ec502a3d3d"
    );
    let data: Json = serde_json::from_slice(raw).unwrap();
    let proof: Json = serde_json::from_slice(provenance).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(
        proof["historical_commit"],
        "6e92c180ca19fed05950bdb08ea8a3e7b10f5526"
    );
    assert_eq!(proof["corpus_sha256"], digest(raw));
    let pins = proof["source_sha256"].as_object().unwrap();
    assert_eq!(pins.len(), 41);
    assert_eq!(
        pins.keys()
            .filter(|p| p.starts_with("runner/") && p.ends_with(".py"))
            .count(),
        25
    );
    for path in [
        "src/workers/tests.rs",
        "src/workers/oracle.py",
        "src/workers.rs",
        "src/workers/help.rs",
        "runner/workers.py",
        "runner/coven.py",
    ] {
        assert_eq!(pins[path].as_str().unwrap().len(), 64);
    }
    let parser = data["parser"].as_array().unwrap();
    assert_eq!(parser.len(), 31);
    let stdout = proof["parser_stdout"].as_str().unwrap();
    assert_eq!(digest(stdout.as_bytes()), proof["parser_stdout_sha256"]);
    let parsed: Vec<Json> = serde_json::from_str(stdout).unwrap();
    assert_eq!(parsed.len(), 31);
    for i in 0..31 {
        assert_eq!(parser[i]["expected"], parsed[i]);
    }
    assert_eq!(parsed.iter().filter(|v| !v.is_null()).count(), 16);
    assert_eq!(parsed.iter().filter(|v| v.is_null()).count(), 15);
    let reports = data["reports"].as_array().unwrap();
    let captures = proof["report_captures"].as_array().unwrap();
    assert_eq!(reports.len(), CASES.len());
    assert_eq!(captures.len(), CASES.len());
    for i in 0..CASES.len() {
        let entry = &reports[i];
        let capture = &captures[i];
        assert_eq!(entry["id"], CASES[i]);
        assert_eq!(capture["id"], CASES[i]);
        let stdout = capture["stdout"].as_str().unwrap();
        assert_eq!(digest(stdout.as_bytes()), capture["stdout_sha256"]);
        let record: Json = serde_json::from_str(stdout).unwrap();
        assert_eq!(record, capture["record"]);
        let mut restored = entry["expected"].clone();
        let ledger = entry["bindings"].as_array().unwrap();
        assert_eq!(ledger.len(), [16, 17, 11, 11, 11][i]);
        let inventory = inventory([7, 8, 2, 2, 2][i]);
        assert_eq!(ledger.len(), inventory.len());
        for j in 0..inventory.len() {
            let (pointer, binding) = &inventory[j];
            assert_eq!(ledger[j]["pointer"], pointer.as_str());
            assert_eq!(ledger[j]["binding"], *binding);
            let slot = restored.pointer_mut(pointer).unwrap();
            assert!(slot.is_null());
            *slot = ledger[j]["captured"].clone();
        }
        let parts = restored["human"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        restored["human"] = json!(format!(
            "{}{}{}",
            parts[0].as_str().unwrap(),
            capture["human_segment"].as_str().unwrap(),
            parts[1].as_str().unwrap()
        ));
        assert_eq!(restored, record);
        assert_eq!(capture["roundtrip_equal"], true);
    }
    data
}

fn inventory(probes: usize) -> Vec<(String, &'static str)> {
    let mut slots = vec![("/report/context/workspace".into(), "workspace")];
    for field in ["selected_path", "resolved_path"] {
        slots.push((format!("/report/executable/{field}"), "executable"));
    }
    for identity in ["identity", "identity_after"] {
        for field in ["device", "inode", "mtime_ns"] {
            slots.push((format!("/report/executable/{identity}/{field}"), field));
        }
    }
    for i in 0..probes {
        slots.push((format!("/report/probes/{i}/argv/0"), "executable"));
    }
    slots
}

pub(super) fn parser(vectors: &[(Vec<u8>, Vec<&str>)]) -> Vec<Json> {
    let data = corpus();
    let cases = data["parser"].as_array().unwrap();
    assert_eq!(vectors.len(), 31);
    assert_eq!(vectors.len(), cases.len());
    for i in 0..vectors.len() {
        assert_eq!(
            json!({"data": vectors[i].0, "route": vectors[i].1}),
            cases[i]["input"],
            "vector {i}"
        );
    }
    cases.iter().map(|case| case["expected"].clone()).collect()
}

#[cfg(unix)]
pub(super) fn report(case: &str, root: &Path) -> Json {
    use std::os::unix::fs::MetadataExt;
    let data = corpus();
    let index = CASES.iter().position(|name| *name == case).unwrap();
    let entry = &data["reports"][index];
    let mut expected = entry["expected"].clone();
    // Path components discard redundant dots like Python Path, but preserve the
    // declared symlink spelling. Only resolved slots use filesystem resolution.
    let selected: PathBuf = root.join("fixture-é\n").components().collect();
    let resolved = fs::canonicalize(&selected).unwrap();
    let workspace = fs::canonicalize(root).unwrap();
    let root_text = resolved.parent().unwrap().to_str().unwrap();
    // The ASCII prefix needs no escaping. The tested Unicode/newline suffix is
    // literal historical evidence, not a copy of the production text renderer.
    assert!(
        root_text.bytes().all(|b| (32..127).contains(&b)),
        "fixture root must be printable ASCII"
    );
    let info = fs::symlink_metadata(&resolved).unwrap();
    assert!(info.is_file() && !info.file_type().is_symlink());
    let integer = |value: i128| -> Json {
        let text = value.to_string();
        let parsed: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.to_string(), text);
        parsed
    };
    let probes = expected["report"]["probes"].as_array().unwrap().len();
    let resolved_slots: Vec<_> = std::iter::once("/report/executable/resolved_path".to_owned())
        .chain((0..probes).map(|i| format!("/report/probes/{i}/argv/0")))
        .collect();
    let mut selected_bindings = 0;
    for slot in entry["bindings"].as_array().unwrap() {
        let pointer = slot["pointer"].as_str().unwrap();
        let value = match slot["binding"].as_str().unwrap() {
            "workspace" => json!(workspace.to_str().unwrap()),
            "executable" if pointer == "/report/executable/selected_path" => {
                selected_bindings += 1;
                json!(selected.to_str().unwrap())
            }
            "executable" => {
                assert!(resolved_slots.iter().any(|slot| slot == pointer));
                json!(resolved.to_str().unwrap())
            }
            "device" => integer(i128::from(info.dev())),
            "inode" => integer(i128::from(info.ino())),
            "mtime_ns" => {
                integer(i128::from(info.mtime()) * 1_000_000_000 + i128::from(info.mtime_nsec()))
            }
            binding => panic!("undeclared binding: {binding}"),
        };
        let target = expected.pointer_mut(pointer).unwrap();
        assert!(target.is_null());
        *target = value;
    }
    assert_eq!(selected_bindings, 1);
    let parts = expected["human"].as_array().unwrap();
    expected["human"] = json!(format!(
        "{}{root_text}/fixture-é\\u000a{}",
        parts[0].as_str().unwrap(),
        parts[1].as_str().unwrap()
    ));
    expected
}

pub(super) fn remove_observed_at(report: &mut Json) {
    for probe in report["probes"].as_array_mut().unwrap() {
        let timestamp = probe["observed_at"].as_str().unwrap();
        let bytes = timestamp.as_bytes();
        assert!((25..=32).contains(&bytes.len()));
        for (i, byte) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
            assert_eq!(bytes[i], byte);
        }
        assert!(timestamp.ends_with("+00:00"));
        let end = bytes.len() - 6;
        if end != 19 {
            assert_eq!(bytes[19], b'.');
            assert!((1..=6).contains(&(end - 20)));
            assert!(bytes[20..end].iter().all(u8::is_ascii_digit));
        }
        let number = |start: usize, end: usize| -> u32 {
            assert!(bytes[start..end].iter().all(u8::is_ascii_digit));
            timestamp[start..end].parse().unwrap()
        };
        let year = number(0, 4);
        assert!(year > 0);
        let month = number(5, 7);
        assert!((1..=12).contains(&month));
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        assert!((1..=days[month as usize - 1]).contains(&number(8, 10)));
        assert!(number(11, 13) < 24 && number(14, 16) < 60 && number(17, 19) < 60);
        // This is the sole legacy comparison exception, never a recursive scrub.
        assert!(
            probe
                .as_object_mut()
                .unwrap()
                .remove("observed_at")
                .is_some()
        );
    }
}
