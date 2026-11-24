//! Test-only decoding of independently frozen reference bytes, never a renderer.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    encoding: String,
    lines: Vec<String>,
    cases: BTreeMap<String, Vec<usize>>,
    inputs: BTreeMap<String, String>,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn corpus() -> &'static Corpus {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/retirement/fixtures/presentation.json"
        ));
        let provenance = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/retirement/fixtures/presentation_provenance.json"
        ));
        assert_eq!(
            sha(raw.as_bytes()),
            "504f54faacbea9fc8d9ffc440ba7b51305a56dd73950b5f755b72d37a921e6ae"
        );
        assert_eq!(
            sha(provenance.as_bytes()),
            "8624352016a4798067814c19370c4eb402fa149f0c992a4eceb802cd2abb1d3f"
        );
        let corpus: Corpus = serde_json::from_str(raw).unwrap();
        let provenance: serde_json::Value = serde_json::from_str(provenance).unwrap();
        assert_eq!(
            corpus.encoding,
            "UTF-8 literal line pool; concatenate indexed lines without separators"
        );
        let mut inventory: BTreeSet<String> = [
            "standup.print",
            "standup.buckets",
            "agent_status.approved",
            "standup.ledger",
            "dashboard.missing",
            "dashboard.ledger",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for status in ["approved", "approval_stale", "decision_invalid", "pending"] {
            for tasks in ["empty", "done"] {
                assert!(inventory.insert(format!("standup.checkpoint.{status}.{tasks}")));
            }
        }
        for lead in 0..3 {
            for goal in 0..4 {
                assert!(inventory.insert(format!("dashboard.lead{lead}.goal{goal}")));
            }
        }
        for checkpoint in 0..4 {
            assert!(inventory.insert(format!("dashboard.checkpoint.{checkpoint}")));
        }
        assert_eq!(inventory.len(), 30);
        assert_eq!(
            corpus.cases.keys().cloned().collect::<BTreeSet<_>>(),
            inventory
        );
        assert_eq!(provenance["case_ids"].as_array().unwrap().len(), 30);
        assert_eq!(
            provenance["case_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_owned())
                .collect::<BTreeSet<_>>(),
            inventory
        );
        assert_eq!(
            provenance["output_sha256"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            inventory
        );
        for (id, indices) in &corpus.cases {
            let text: String = indices
                .iter()
                .map(|&index| corpus.lines.get(index).unwrap().as_str())
                .collect();
            assert_eq!(
                sha(text.as_bytes()),
                provenance["output_sha256"][id].as_str().unwrap(),
                "{id}"
            );
        }
        assert_eq!(
            corpus.inputs.keys().map(String::as_str).collect::<Vec<_>>(),
            ["plan.approved", "review.approved"]
        );
        assert_eq!(provenance["input_sha256"].as_object().unwrap().len(), 2);
        for (id, text) in &corpus.inputs {
            assert_eq!(
                sha(text.as_bytes()),
                provenance["input_sha256"][id].as_str().unwrap(),
                "{id}"
            );
        }
        corpus
    })
}

pub(super) fn input(id: &str) -> String {
    corpus().inputs.get(id).unwrap().clone()
}

pub(super) struct Cases {
    remaining: BTreeSet<String>,
}

impl Cases {
    pub(super) fn new(prefix: &str, count: usize) -> Self {
        let remaining: BTreeSet<_> = corpus()
            .cases
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect();
        assert_eq!(remaining.len(), count, "{prefix}");
        Self { remaining }
    }

    pub(super) fn expected(&mut self, id: &str) -> String {
        assert!(
            self.remaining.remove(id),
            "missing, repeated or out-of-group case: {id}"
        );
        let corpus = corpus();
        corpus
            .cases
            .get(id)
            .unwrap()
            .iter()
            .map(|&index| corpus.lines.get(index).unwrap().as_str())
            .collect()
    }
}

impl Drop for Cases {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                self.remaining.is_empty(),
                "unexercised frozen cases: {:?}",
                self.remaining
            );
        }
    }
}
