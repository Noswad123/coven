//! Only the outer manifest is decoded here; embedded record strings remain raw.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema_version: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: usize,
    raw: String,
    admitted: bool,
}

pub(super) fn compare(authored: &[String]) {
    if std::env::var_os("COVEN_REPOSITORY_ASSERT_PYTHON_ABSENT").is_some() {
        for path in [
            "/usr/bin/python3",
            "/bin/python3",
            "/usr/bin/python3.14",
            "/usr/local/bin/python3",
            "/home/linuxbrew/.linuxbrew/bin/python3",
            "/usr/lib/python3.14",
            "/usr/lib/libpython3.14.so",
        ] {
            let error = fs::symlink_metadata(path).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{path}");
            #[cfg(target_os = "linux")]
            assert_eq!(error.raw_os_error(), Some(libc::ENOENT), "{path}");
        }
    }
    let corpus = include_bytes!("../../tests/retirement/fixtures/repository_records.json");
    let provenance =
        include_bytes!("../../tests/retirement/fixtures/repository_records_provenance.json");
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    assert_eq!(
        digest(corpus),
        "8ab1985b5879b918709b1a233d0505425b0b128289951a6f435eed6bbf14ccd9"
    );
    assert_eq!(
        digest(provenance),
        "300a96a47169d355eb3b114906259261340efbc087e6453cf07df37be7caa9ef"
    );
    let frozen: Corpus = serde_json::from_slice(corpus).unwrap();
    let proof: Value = serde_json::from_slice(provenance).unwrap();
    assert_eq!(frozen.schema_version, 1);
    assert_eq!(proof["schema_version"], 1);
    assert_eq!(
        proof["historical_commit"],
        "b97823c7f4f30e53f768258347eaf66daec74053"
    );
    assert_eq!(proof["corpus_sha256"], digest(corpus));
    assert_eq!(proof["cases"], 1979);
    assert_eq!(proof["admitted"], 684);
    assert_eq!(proof["refused"], 1295);
    assert_eq!(
        proof["matrix"],
        json!({"base":1,"top_fields":13,"inodes":5,"raw_huge_integer":1,
        "paths":6,"path_targets":2,"dates":9,"separators":4,"clocks":9,"zones":6,"timestamps":1944,"tail":3})
    );
    assert_eq!(authored.len(), 1 + 13 + 5 + 1 + 6 * 2 + 9 * 4 * 9 * 6 + 3);
    assert_eq!(frozen.cases.len(), 1979);
    let inventory = proof["inventory"].as_array().unwrap();
    assert_eq!(inventory.len(), 1979);
    assert_eq!(
        frozen.cases.iter().filter(|case| case.admitted).count(),
        684
    );
    // Check every byte and every indexed entry before performing any admission comparison.
    for index in 0..1979 {
        let case = &frozen.cases[index];
        assert_eq!(case.id, index);
        assert_eq!(inventory[index]["id"], index);
        assert_eq!(inventory[index]["raw_sha256"], digest(case.raw.as_bytes()));
        assert_eq!(case.raw, authored[index], "authored case {index}");
    }
    for case in frozen.cases {
        assert_eq!(
            record::Authority::decode(case.raw.as_bytes().to_vec()).is_ok(),
            case.admitted,
            "case {}: {}",
            case.id,
            case.raw
        );
    }
}
