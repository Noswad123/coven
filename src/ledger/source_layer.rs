//! Private, inert historical inputs shared only by the owned test modules.
#![allow(dead_code)] // Each owning test module uses a different subset.

use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const CORPUS: &[u8] = include_bytes!("../../tests/retirement/fixtures/source_ledger.json");

pub(super) fn row(id: &str) -> serde_json::Value {
    assert_eq!(
        digest(CORPUS),
        "e9cf3db6bc90ea5cd89dd2ff794bab38261da9ebe08912e002c230f4adf1cf5b"
    );
    let corpus: serde_json::Value = serde_json::from_slice(CORPUS).unwrap();
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 13);
    let matches: Vec<_> = rows.iter().filter(|row| row["id"] == id).collect();
    assert_eq!(matches.len(), 1, "{id}");
    let row = matches[0];
    assert_eq!(row["exit_status"], 0);
    assert_eq!(row["stderr"], "");
    for stream in ["stdout", "stderr"] {
        assert_eq!(
            digest(row[stream].as_str().unwrap().as_bytes()),
            row[format!("{stream}_sha256")].as_str().unwrap()
        );
    }
    row.clone()
}

fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

fn bytes(file: &serde_json::Value) -> Vec<u8> {
    let hex = file["hex"].as_str().unwrap();
    assert_eq!(hex.len() % 2, 0);
    let raw: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(digest(&raw), file["sha256"].as_str().unwrap());
    raw
}

pub(super) fn projection(id: &str) -> Vec<u8> {
    let row = row(id);
    let file = row["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "state/tasks.json")
        .unwrap();
    bytes(file)
}

pub(super) fn snapshot_revision(raw: Option<&[u8]>) -> String {
    raw.map_or_else(
        || "tasks-missing:v1".into(),
        |raw| format!("tasks-sha256:{}", digest(raw)),
    )
}

pub(super) fn assert_view(id: &str, view: &crate::ledger::View) {
    let row = row(id);
    let head = row["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["path"].as_str().unwrap().ends_with(".frame"))
        .last()
        .unwrap();
    let raw = bytes(head);
    let envelope = crate::values::parse(&raw, crate::values::Profile::Authority).unwrap();
    let payload = envelope
        .as_object()
        .unwrap()
        .get("payload")
        .unwrap()
        .as_object()
        .unwrap();
    let revision = payload
        .get("revision")
        .or_else(|| {
            payload
                .get("result")
                .and_then(|r| r.as_object().unwrap().get("after_revision"))
        })
        .unwrap();
    assert_eq!(view.revision(), revision.as_str().unwrap());
    assert_eq!(view.head_digest(), head["sha256"].as_str().unwrap());
    let expected = if row["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "state/tasks.json")
    {
        projection(id)
    } else {
        b"[]\n".to_vec()
    };
    assert_eq!(view.projection_bytes().unwrap(), expected);
}

pub(super) fn install(root: &Path, id: &str, execution_only: bool) {
    use std::io::Write;
    let row = row(id);
    let selected = |relative: &str| {
        assert!(
            Path::new(relative)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        );
        !execution_only || relative == "state/execution" || relative.starts_with("state/execution/")
    };
    for entry in row["directories"].as_array().unwrap() {
        let relative = entry["path"].as_str().unwrap();
        if selected(relative) {
            let path = root.join(relative);
            fs::create_dir_all(&path).unwrap();
            mode(&path, entry["mode"].as_u64().unwrap());
        }
    }
    for file in row["files"].as_array().unwrap() {
        let relative = file["path"].as_str().unwrap();
        if selected(relative) {
            let path = root.join(relative);
            // Never silently replace poison, symlinks, or an existing fixture.
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            output.write_all(&bytes(file)).unwrap();
            mode(&path, file["mode"].as_u64().unwrap());
        }
    }
}

fn mode(path: &Path, mode: u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode.try_into().unwrap())).unwrap();
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}
