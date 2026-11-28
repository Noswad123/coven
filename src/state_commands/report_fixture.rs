use std::{collections::BTreeMap, fs, path::Path};

mod captured {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/workspace/source_fixture.rs"
    ));
}

const WORKSPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EPOCH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn strings(value: &serde_json::Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect()
}

fn observed(root: &Path, relative: &Path, files: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(root.join(relative)).unwrap() {
        let entry = entry.unwrap();
        let path = relative.join(entry.file_name());
        assert!(!entry.file_type().unwrap().is_symlink());
        if entry.file_type().unwrap().is_dir() {
            observed(root, &path, files);
        } else if path != Path::new("state/.workspace.lock") && path != Path::new("dashboard.md") {
            assert!(entry.file_type().unwrap().is_file());
            files.insert(
                path.to_str().unwrap().to_owned(),
                fs::read_to_string(entry.path()).unwrap(),
            );
        }
    }
}

fn check(root: &Path, expected: &BTreeMap<String, String>) {
    let mut actual = BTreeMap::new();
    observed(root, Path::new(""), &mut actual);
    assert_eq!(
        &actual, expected,
        "complete report preinput, excluding only lock and dashboard outputs"
    );
}

pub(super) fn snapshot(root: &Path, pretty: bool) -> String {
    let row = captured::case(if pretty {
        "report-inspect-pretty"
    } else {
        "report-inspect-compact"
    });
    check(root, &strings(&row["inputs"]));
    row["stdout"]
        .as_str()
        .unwrap()
        .strip_suffix('\n')
        .unwrap()
        .to_owned()
}

pub(super) struct Reports {
    files: BTreeMap<String, String>,
    workspace: String,
    epoch: String,
    old_digest: String,
    digest: String,
}

impl Reports {
    pub(super) fn bind(root: &Path) -> Self {
        let row = captured::case("ledger-seeded");
        let files = strings(&row["artifacts"]);
        // Only the two generated UUIDs come from native metadata, never report output.
        let marker: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("state/task-authority.json")).unwrap())
                .unwrap();
        let workspace = marker["workspace_id"].as_str().unwrap().to_owned();
        let epoch = marker["epoch_id"].as_str().unwrap().to_owned();
        for id in [&workspace, &epoch] {
            assert_eq!(id.len(), 32);
            assert!(
                id.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
        let ids = |raw: &str| raw.replace(WORKSPACE, &workspace).replace(EPOCH, &epoch);
        let frame_path = format!("state/task-ledger/{EPOCH}/frames/00000000000000000000.frame");
        let frame = &files[&frame_path];
        // Hash the historical payload slice after UUID binding. No production
        // encoder, native frame bytes, or actual report contributes expected hashes.
        let (_, tail) = frame.split_once(",\"payload\":").unwrap();
        let (payload, tail) = tail.rsplit_once(",\"sha256\":\"").unwrap();
        assert_eq!(payload.matches(WORKSPACE).count(), 2);
        assert_eq!(payload.matches(EPOCH).count(), 2);
        let payload_digest = tail.split_once('"').unwrap().0;
        assert_eq!(captured::hash(payload.as_bytes()), payload_digest);
        let bound_frame =
            ids(frame).replace(payload_digest, &captured::hash(ids(payload).as_bytes()));
        let old_digest = captured::hash(frame.as_bytes());
        let digest = captured::hash(bound_frame.as_bytes());
        let manifest_path = format!("state/task-ledger/{EPOCH}/manifest.json");
        let manifest = &files[&manifest_path];
        let bound_manifest = ids(manifest).replace(&old_digest, &digest);
        let old_manifest_digest = captured::hash(manifest.as_bytes());
        let manifest_digest = captured::hash(bound_manifest.as_bytes());
        let files = files
            .into_iter()
            .map(|(path, raw)| {
                let bound = if path == frame_path {
                    bound_frame.clone()
                } else {
                    ids(&raw)
                        .replace(&old_digest, &digest)
                        .replace(&old_manifest_digest, &manifest_digest)
                };
                (path.replace(EPOCH, &epoch), bound)
            })
            .collect();
        let reports = Self {
            files,
            workspace,
            epoch,
            old_digest,
            digest,
        };
        reports.check(root, false);
        reports
    }

    pub(super) fn check(&self, root: &Path, pending: bool) {
        let mut files = self.files.clone();
        if pending {
            files.remove("state/task-authority.json").unwrap();
        }
        check(root, &files);
    }

    pub(super) fn expected(&self, mode: &str, pretty: bool) -> String {
        assert!(["import", "repair", "resume"].contains(&mode));
        let row = captured::case(&format!(
            "report-{mode}-{}",
            if pretty { "pretty" } else { "compact" }
        ));
        assert_eq!(
            strings(&row["inputs"]),
            strings(&captured::case("ledger-seeded")["artifacts"])
        );
        let stdout = row["stdout"].as_str().unwrap();
        assert_eq!(stdout.matches(WORKSPACE).count(), 2);
        assert_eq!(stdout.matches(EPOCH).count(), 2);
        assert_eq!(stdout.matches(&self.old_digest).count(), 1);
        stdout
            .strip_suffix('\n')
            .unwrap()
            .replace(WORKSPACE, &self.workspace)
            .replace(EPOCH, &self.epoch)
            .replace(&self.old_digest, &self.digest)
    }
}
