//! E1 expected-side fixtures. No runner imports, process execution, or production
//! action/checkpoint encoder is used to construct an expectation.
use super::*;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const CORPUS: &[u8] = include_bytes!("../../../../tests/retirement/fixtures/source_actions.json");
const PROOF: &[u8] =
    include_bytes!("../../../../tests/retirement/fixtures/source_actions_provenance.json");
const IDS: &[&str] = &[
    "inventory",
    "reviewed",
    "completed",
    "failed",
    "import",
    "imported_inputs",
    "reapproved",
    "unknown_ack",
    "guidance_yes",
    "guidance_test",
    "inventory_herdr",
];
const CP: &str = "runtime-launch-cccccccccccccccccccccccccccccccc";

fn hash(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}

pub(super) fn case(id: &str) -> Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    let data = DATA.get_or_init(|| {
        assert_eq!(
            hash(CORPUS),
            "def8a98f85c36dda24c82e2797ec5ccdcc006d440b94534b84cde7e69f5ff882"
        );
        assert_eq!(
            hash(PROOF),
            "fad747f54c59dc304d4c36766c6f495115e11d03f1a11d834e302aebf92e5dbc"
        );
        let data = parse_authority(CORPUS).unwrap();
        let proof = parse_authority(PROOF).unwrap();
        assert_eq!(
            text(&proof, "historical_commit").unwrap(),
            "a249a5dfe8d32fd75022f8ed2ad524264b7436a0"
        );
        assert_eq!(text(&proof, "corpus_sha256").unwrap(), hash(CORPUS));
        assert_eq!(
            text(&proof, "capture_tool_sha256").unwrap(),
            "e1077b1155d799c4d6d3107c0a78e78db4ad13748895ce24c34fa135c852bc37"
        );
        let pins = get(&proof, "source_sha256").unwrap().as_object().unwrap();
        assert_eq!(pins.entries().len(), 47);
        assert_eq!(
            pins.entries()
                .iter()
                .filter(|(p, _)| p.starts_with("runner/") && p.ends_with(".py"))
                .count(),
            25
        );
        for (key, sha) in [
            (
                "runner/actions.py",
                "0f44f26a9394644b8d54c09052ff71019dac4bf8d84a8cd5423e7f24b87562c4",
            ),
            (
                "runner/checkpoints.py",
                "6d9af7842ed2d82dc72219115ff750fa79596254c2d5ae871f59b8d83ef2a8dc",
            ),
            (
                "src/actions/mutation/tests.rs",
                "d1b6bf08488039cec6795301fdbabbd64764b011b1316f103d85d1cecc6c6699",
            ),
            (
                "src/checkpoint_guidance/tests.rs",
                "258b429f79724f300ee5a25227701dbcf81f7da0bac5ef10d1a95d71dc9b6351",
            ),
        ] {
            assert_eq!(pins.get(key).unwrap().as_str().unwrap(), sha);
        }
        for field in ["helper_original", "guidance_original", "guidance_script"] {
            assert_eq!(
                text(&proof, &format!("{field}_sha256")).unwrap(),
                hash(text(&proof, field).unwrap().as_bytes())
            );
        }
        let cases = get(&data, "cases").unwrap().as_array().unwrap();
        assert_eq!(cases.len(), IDS.len());
        assert_eq!(
            get(&data, "case_ids").unwrap().canonical().unwrap(),
            Value::Array(IDS.iter().map(|id| string(*id)).collect())
                .canonical()
                .unwrap()
        );
        assert_eq!(
            get(&data, "case_ids").unwrap().canonical().unwrap(),
            get(&proof, "case_ids").unwrap().canonical().unwrap()
        );
        for (entry, id) in cases.iter().zip(IDS) {
            assert_eq!(text(entry, "id").unwrap(), *id);
            for field in ["input", "stdout"] {
                assert_eq!(
                    text(entry, &format!("{field}_sha256")).unwrap(),
                    hash(text(entry, field).unwrap().as_bytes())
                );
            }
            assert_eq!(
                get(entry, "stdout_bytes").unwrap().canonical().unwrap(),
                text(entry, "stdout").unwrap().len().to_string().as_bytes()
            );
            assert_eq!(text(entry, "stderr").unwrap(), "");
            assert_eq!(
                get(entry, "exit_status").unwrap().canonical().unwrap(),
                b"0"
            );
            let files = get(entry, "files").unwrap().as_object().unwrap();
            let hashes = get(entry, "files_sha256").unwrap().as_object().unwrap();
            assert_eq!(files.entries().len(), hashes.entries().len());
            for (path, raw) in files.entries() {
                assert_eq!(
                    hashes.get(path).unwrap().as_str().unwrap(),
                    hash(raw.as_str().unwrap().as_bytes())
                );
            }
        }
        // Independently assert the pure fixture builder's complete output shape
        // against both original prepare callers, including retained history.
        let pending = text(&cases[0], "stdout").unwrap().as_bytes();
        let approved = approval(CP, pending, "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "reviewed");
        let view = parse_authority(text(&cases[1], "stdout").unwrap().as_bytes()).unwrap();
        assert_eq!(approved, text(&view, "content").unwrap().as_bytes());
        assert_eq!(
            revision("snapshot", CP, &approved),
            text(&view, "revision").unwrap()
        );
        let edited = String::from_utf8(approved)
            .unwrap()
            .replace("unrestricted", "read-only");
        let changed = approval(
            CP,
            edited.as_bytes(),
            "ffffffffffffffffffffffffffffffff",
            "changed scope",
        );
        let view = parse_authority(text(&cases[6], "stdout").unwrap().as_bytes()).unwrap();
        assert_eq!(changed, text(&view, "content").unwrap().as_bytes());
        assert_eq!(
            revision("snapshot", CP, &changed),
            text(&view, "revision").unwrap()
        );
        data
    });
    get(data, "cases")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| text(entry, "id").unwrap() == id)
        .unwrap()
        .clone()
}

fn file(entry: &Value, name: &str) -> Vec<u8> {
    text(get(entry, "files").unwrap(), name)
        .unwrap()
        .as_bytes()
        .to_vec()
}

pub(super) fn revision(kind: &str, cp: &str, raw: &[u8]) -> String {
    let tag = format!("checkpoint-{kind}-v1");
    let mut digest = Sha256::new();
    for part in [tag.as_bytes(), cp.as_bytes(), raw] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{tag}:{:x}", digest.finalize())
}

fn scope(original: &str) -> String {
    let mut covered = String::new();
    let mut lines = original.split_inclusive('\n');
    while let Some(line) = lines.next() {
        if line.starts_with("<!-- coven-checkpoint-") {
            assert!(
                line == "<!-- coven-checkpoint-decision:v1\n"
                    || line == "<!-- coven-checkpoint-history:v1\n"
            );
            if line.contains("history") {
                assert!(covered.ends_with('\n'));
                covered.pop();
            }
            lines.next().unwrap();
            assert_eq!(lines.next(), Some("-->\n"));
        } else if !line.starts_with("Status: ")
            && !matches!(
                line,
                "- [ ] Approved\n" | "- [x] Approved\n" | "- [ ] Rejected / needs changes\n"
            )
        {
            covered.push_str(line);
        }
    }
    covered
}

fn approval(cp: &str, raw: &[u8], decision: &str, notes: &str) -> Vec<u8> {
    assert!(cp.starts_with("runtime-launch-") && cp.len() == 47);
    assert!(decision.bytes().all(|b| b.is_ascii_hexdigit()) && decision.len() == 32);
    assert!(matches!(notes, "reviewed" | "changed scope"));
    let original = std::str::from_utf8(raw).unwrap();
    assert_eq!(original.matches("Status: ").count(), 1);
    assert!(original.contains("# Runtime Launch Review\n"));
    assert!(original.contains("## User Decision\n"));
    let scope_revision = revision("scope", cp, scope(original).as_bytes());
    let reviewed = revision("snapshot", cp, raw);
    let fields = format!(
        "\"action\":\"approved\",\"checkpoint\":\"{cp}\",\"decided_at\":\"now\",\"decision_id\":\"{decision}\",\"reviewed_revision\":\"{reviewed}\",\"scope_revision\":\"{scope_revision}\",\"source\":\"cli\",\"version\":1"
    );
    let body = if original.starts_with("<!-- coven-checkpoint-decision:v1\n") {
        original.split_once("\n-->\n").unwrap().1
    } else {
        original
    };
    let body = body
        .replace("Status: pending\n", "Status: approved\n")
        .replace("- [ ] Approved\n", "- [x] Approved\n");
    let history = format!(
        "\"action\":\"approved\",\"checkpoint\":\"{cp}\",\"decided_at\":\"now\",\"decision_id\":\"{decision}\",\"notes\":\"{notes}\",\"reviewed_revision\":\"{reviewed}\",\"scope_revision\":\"{scope_revision}\",\"source\":\"cli\",\"version\":1"
    );
    let result = format!(
        "<!-- coven-checkpoint-decision:v1\n{{{fields}}}\n-->\n{body}\n<!-- coven-checkpoint-history:v1\n{{{history}}}\n-->\n"
    );
    assert_eq!(scope(&result), scope(original));
    result.into_bytes()
}

pub(super) fn approve(cp: &str, raw: &[u8], decision: &str, notes: &str) -> Vec<u8> {
    case("reviewed");
    approval(cp, raw, decision, notes)
}

fn bind(value: &Value, substitutions: &[(&str, &str)]) -> Value {
    match value {
        Value::String(s) => {
            let mut s = s.clone();
            for (before, after) in substitutions {
                s = s.replace(before, after);
            }
            string(s)
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| bind(v, substitutions)).collect()),
        Value::Object(o) => Value::object(
            o.entries()
                .iter()
                .map(|(k, v)| (k.clone(), bind(v, substitutions)))
                .collect(),
        )
        .unwrap(),
        other => other.clone(),
    }
}

fn environment() -> Value {
    object(
        ["HOME", "LANG", "LC_ALL", "PATH", "SHELL", "TERM"]
            .into_iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k, string(v))))
            .collect(),
    )
    .unwrap()
}

fn fixture_location(root: &Path) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let root = root.canonicalize().unwrap();
        let metadata = fs::metadata(&root).unwrap();
        object(vec![
            ("root", string(root.to_str().unwrap())),
            (
                "device",
                Value::integer(&metadata.dev().to_string()).unwrap(),
            ),
            (
                "inode",
                Value::integer(&metadata.ino().to_string()).unwrap(),
            ),
        ])
        .unwrap()
    }
    #[cfg(not(unix))]
    panic!("E1 fixtures require supported Unix workspace identity")
}

fn inventory(root: &Path, imported: bool) -> Value {
    let entry = case(if imported {
        "imported_inputs"
    } else {
        runtime_case()
    });
    let mut expected = if imported {
        parse_authority(text(&entry, "stdout").unwrap().as_bytes()).unwrap()
    } else {
        get(
            &parse_authority(text(&entry, "input").unwrap().as_bytes()).unwrap(),
            "inputs",
        )
        .unwrap()
        .clone()
    };
    expected
        .set_field("location", fixture_location(root))
        .unwrap();
    if imported {
        let template =
            parse_authority(text(&case(runtime_case()), "input").unwrap().as_bytes()).unwrap();
        expected
            .set_field(
                "effective_runtime",
                get(get(&template, "inputs").unwrap(), "effective_runtime")
                    .unwrap()
                    .clone(),
            )
            .unwrap();
    }
    let selection = object(vec![(
        "COVEN_MULTIPLEXER",
        std::env::var("COVEN_MULTIPLEXER").map_or(Value::Null, string),
    )])
    .unwrap();
    expected
        .set_field(
            "environment",
            object(vec![("selection", selection), ("transport", environment())]).unwrap(),
        )
        .unwrap();
    expected
}

fn expected_bundle(root: &Path, id: &str, before: &[u8]) -> Value {
    let template =
        parse_authority(text(&case(runtime_case()), "input").unwrap().as_bytes()).unwrap();
    let control = parse_authority(before).unwrap();
    let old_root = text(get(&template, "location").unwrap(), "root").unwrap();
    let mut expected = bind(
        &template,
        &[
            (old_root, root.to_str().unwrap()),
            (
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                text(&control, "workspace_id").unwrap(),
            ),
            (
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                text(&control, "epoch_id").unwrap(),
            ),
            ("cccccccccccccccccccccccccccccccc", id),
        ],
    );
    expected
        .set_field("location", fixture_location(root))
        .unwrap();
    expected
        .set_field("inputs", inventory(root, false))
        .unwrap();
    expected
        .set_field(
            "control_revision",
            string(format!(
                "runtime-launch-control-v1:{}",
                hash(&control.canonical().unwrap())
            )),
        )
        .unwrap();
    let mut plan = get(&expected, "plan").unwrap().clone();
    plan.set_field("env", environment()).unwrap();
    expected.set_field("plan", plan).unwrap();
    expected
}

// JSON string escaping is needed only for known path metadata, never arbitrary
// authority payloads. Payloads stay opaque raw strings or lossless values.
fn quoted(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap();
    let mut out = String::new();
    for c in json.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for word in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{:04x}", *word));
            }
        }
    }
    out.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('`', "\\u0060")
}

pub(super) fn prepared(root: &Path, id: &str, before: &[u8]) -> Vec<u8> {
    let expected = expected_bundle(root, id, before);
    let actual =
        parse_authority(&fs::read(root.join(format!("state/actions/{id}.json"))).unwrap()).unwrap();
    assert_eq!(actual.canonical().unwrap(), expected.canonical().unwrap());
    let pending = pending(&expected);
    assert_eq!(
        fs::read(root.join(format!("checkpoints/runtime-launch-{id}.md"))).unwrap(),
        pending
    );
    pending
}

pub(super) fn manifest(root: &Path, id: &str, before: &[u8]) -> String {
    format!(
        "runtime-launch-manifest-v1:{}",
        hash(&expected_bundle(root, id, before).canonical().unwrap())
    )
}

fn pending(expected: &Value) -> Vec<u8> {
    let entry = case(runtime_case());
    let template = parse_authority(text(&entry, "input").unwrap().as_bytes()).unwrap();
    let mut pending = text(&entry, "stdout").unwrap().to_owned();
    for key in ["action_id", "workspace_id", "epoch_id"] {
        pending = pending.replace(text(&template, key).unwrap(), text(&expected, key).unwrap());
    }
    let old_location = get(&template, "location").unwrap();
    let new_location = get(&expected, "location").unwrap();
    pending = pending.replace(
        &quoted(text(old_location, "root").unwrap()),
        &quoted(text(new_location, "root").unwrap()),
    );
    for key in ["device", "inode"] {
        let number =
            |v: &Value| String::from_utf8(get(v, key).unwrap().canonical().unwrap()).unwrap();
        let old = format!("\"{key}\": {}", number(old_location));
        assert_eq!(pending.matches(&old).count(), 1);
        pending = pending.replace(&old, &format!("\"{key}\": {}", number(new_location)));
    }
    pending = pending.replace(
        &format!(
            "runtime-launch-manifest-v1:{}",
            hash(&template.canonical().unwrap())
        ),
        &format!(
            "runtime-launch-manifest-v1:{}",
            hash(&expected.canonical().unwrap())
        ),
    );
    pending.into_bytes()
}

fn runtime_case() -> &'static str {
    if std::env::var("COVEN_MULTIPLEXER").is_ok_and(|v| v.trim().eq_ignore_ascii_case("herdr")) {
        "inventory_herdr"
    } else {
        "inventory"
    }
}

pub(super) fn import_tasks(root: &Path) {
    let entry = case("import");
    for (name, raw) in get(&entry, "files").unwrap().as_object().unwrap().entries() {
        if name == "state/task-authority.json"
            || name.starts_with("state/task-ledger/")
            || name == "state/tasks.json"
        {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, raw.as_str().unwrap()).unwrap();
        }
    }
    let view = crate::ledger::read(root).unwrap().unwrap();
    assert!(view.report(root).is_ok());
}

pub(super) fn imported_inputs(root: &Path) -> Vec<u8> {
    inventory(root, true).canonical().unwrap()
}

pub(super) fn completed(root: &Path, review: &Reviewed, failed: bool, pre_transport: &[u8]) {
    let entry = case(if failed { "failed" } else { "completed" });
    let captured = parse_authority(&file(&entry, CONTROL)).unwrap();
    let native = parse_authority(&fs::read(root.join(CONTROL)).unwrap()).unwrap();
    let old_claim = get(
        get(&captured, "claims").unwrap(),
        "cccccccccccccccccccccccccccccccc",
    )
    .unwrap();
    let pre_transport = parse_authority(pre_transport).unwrap();
    let pre_claim = get(get(&pre_transport, "claims").unwrap(), &review.id).unwrap();
    let new_claim = get(get(&native, "claims").unwrap(), &review.id).unwrap();
    let mut expected = old_claim.clone();
    // Only nondeterministic identities and independently checked PRE-state plan
    // bindings are admitted. Outcome/status/report remain historical expectations.
    for (key, value) in [
        ("action_id", string(&review.id)),
        ("manifest_digest", string(&review.manifest)),
        ("checkpoint_revision", string(&review.checkpoint)),
        ("decision_id", string(&review.decision)),
        ("claim_id", get(pre_claim, "claim_id").unwrap().clone()),
    ] {
        expected.set_field(key, value).unwrap();
    }
    let bundle = expected_bundle(root, &review.id, &review.before);
    assert_eq!(
        fs::read(root.join(format!("state/actions/{}.json", review.id))).unwrap(),
        bundle.canonical().unwrap()
    );
    expected
        .set_field("plan", get(&bundle, "plan").unwrap().clone())
        .unwrap();
    let cp = format!("runtime-launch-{}", review.id);
    expected
        .set_field(
            "scope_revision",
            string(revision(
                "scope",
                &cp,
                scope(std::str::from_utf8(&pending(&bundle)).unwrap()).as_bytes(),
            )),
        )
        .unwrap();
    let mut expected_pre = expected.clone();
    expected_pre.set_field("status", string("claimed")).unwrap();
    expected_pre.set_field("report", Value::Null).unwrap();
    assert_eq!(
        pre_claim.canonical().unwrap(),
        expected_pre.canonical().unwrap()
    );
    assert_eq!(
        new_claim.canonical().unwrap(),
        expected.canonical().unwrap()
    );
    let mut claims = object(vec![]).unwrap();
    claims.set_field(&review.id, expected).unwrap();
    let mut expected_control = captured;
    for key in ["location", "workspace_id", "epoch_id"] {
        expected_control
            .set_field(key, get(&bundle, key).unwrap().clone())
            .unwrap();
    }
    expected_control
        .set_field("head", string(&review.id))
        .unwrap();
    expected_control.set_field("claims", claims).unwrap();
    assert_eq!(
        native.canonical().unwrap(),
        expected_control.canonical().unwrap()
    );
    assert!(
        super::super::super::inspect(root, &review.id)
            .unwrap()
            .as_object()
            .is_some()
    );
}

pub(super) fn acknowledged(root: &Path, review: &Reviewed, claim_id: &str) -> Vec<u8> {
    let mut control = parse_authority(&fs::read(root.join(CONTROL)).unwrap()).unwrap();
    let mut claim = get(get(&control, "claims").unwrap(), &review.id)
        .unwrap()
        .clone();
    assert_eq!(text(&claim, "claim_id").unwrap(), claim_id);
    assert_eq!(text(&claim, "status").unwrap(), "claimed");
    assert!(matches!(get(&claim, "report").unwrap(), Value::Null));
    let historical =
        parse_authority(text(&case("unknown_ack"), "stdout").unwrap().as_bytes()).unwrap();
    for key in ["status", "operator_acknowledged", "report"] {
        claim
            .set_field(key, get(&historical, key).unwrap().clone())
            .unwrap();
    }
    let mut claims = get(&control, "claims").unwrap().clone();
    claims.set_field(&review.id, claim.clone()).unwrap();
    control.set_field("claims", claims).unwrap();
    fs::write(root.join(CONTROL), control.canonical().unwrap()).unwrap();
    claim.canonical().unwrap()
}
