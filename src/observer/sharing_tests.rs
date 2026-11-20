use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    sync::atomic::AtomicUsize,
};

const RUNTIME: &str = "11111111-2222-4333-8444-555555555555";
const OTHER: &str = "21111111-2222-4333-8444-555555555555";
const GRANT: &str = "31111111-2222-4333-8444-555555555555";
const REQUEST: &str = "41111111-2222-4333-8444-555555555555";

type StatusPause = Arc<(Mutex<(bool, bool)>, Condvar)>;

struct PeerState {
    decisions: Option<Vec<Value>>,
    decision_writable: bool,
    decision_complete: bool,
    decision_omitted: Option<u64>,
    decision_error: Option<String>,
    association: Option<Value>,
    association_delay_ms: u64,
    status_pause_skip: usize,
    runtime: String,
    grant: Option<crate::shared_native::Grant>,
    lose_receipt: bool,
    corrupt_receipt: bool,
    duplicate_frame: bool,
    wrong_runtime: bool,
    delay: bool,
    requests: Vec<Value>,
    status_pause: Option<StatusPause>,
    attention_pause: Option<StatusPause>,
}

struct Peer {
    root: tempfile::TempDir,
    socket: PathBuf,
    registry: PathBuf,
    state: Arc<Mutex<PeerState>>,
    stop: Arc<AtomicBool>,
    submits: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("s").tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let registry = root.path().join("r");
        std::fs::create_dir(&registry).unwrap();
        std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("s");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let state = Arc::new(Mutex::new(PeerState {
            decisions: None,
            decision_writable: true,
            decision_complete: true,
            decision_omitted: Some(0),
            decision_error: None,
            association: None,
            association_delay_ms: 0,
            status_pause_skip: 0,
            runtime: RUNTIME.into(),
            grant: Some(crate::shared_native::Grant {
                grant_id: GRANT.into(),
                session_id: "ses_private".into(),
                can_submit: true,
                lease_ms: 5000,
            }),
            lose_receipt: false,
            corrupt_receipt: false,
            duplicate_frame: false,
            wrong_runtime: false,
            delay: false,
            requests: Vec::new(),
            status_pause: None,
            attention_pause: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let submits = Arc::new(AtomicUsize::new(0));
        let (s, stopping, count) = (state.clone(), stop.clone(), submits.clone());
        let thread = std::thread::spawn(move || {
            std::thread::scope(|scope| {
                while !stopping.load(Ordering::Acquire) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(pair) => pair,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        Err(e) => panic!("{e}"),
                    };
                    let (s, count) = (s.clone(), count.clone());
                    scope.spawn(move || {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = String::new();
                if BufReader::new(&stream).read_line(&mut raw).is_err() {
                    return;
                }
                if raw.is_empty() {
                    return;
                }
                let request: Value = serde_json::from_str(&raw).unwrap();
                let mut state = s.lock().unwrap();
                state.requests.push(request.clone());
                let method = request["method"].as_str().unwrap();
                let native_runtime = state.runtime.clone();
                let mut wire_error = None;
                let mut result = match method {
                    "capabilities" => {
                        let mut caps = json!({"type":"capabilities","contract":"djinn.interactive-prompt.v1","version":1,
                        "runtime_id":native_runtime,"max_prompt_bytes":65536,"web_sharing":{"contract":crate::shared_native::CONTRACT,"version":1}});
                        if state.association.is_some() {
                            caps["card_association"] = json!({"contract":crate::shared_native::ASSOCIATION_CONTRACT,"version":1});
                        }
                        if state.decisions.is_some() { caps["decision_control"] = crate::decision_native::capability(); }
                        caps
                    }
                    "shared_attention" => {
                        let grant = state.grant.as_ref().unwrap();
                        assert_eq!(request["params"],json!({"expected_runtime_id":native_runtime,"session_id":grant.session_id,"grant_id":grant.grant_id}));
                        let requests: Vec<_> = state.decisions.as_ref().unwrap().iter().map(|d| json!({"kind":d["kind"],"request_id":d["request_id"],"occurrence_id":d["occurrence_id"],"details_sha256":d["details_sha256"],"details_available":!d["details_sha256"].is_null()})).collect();
                        json!({"type":"shared_attention","contract":crate::decision_native::CONTRACT,
                            "grant_id":grant.grant_id,"session_id":grant.session_id,"requests":requests,
                            "complete":state.decision_complete,"omitted_count":state.decision_omitted,"can_respond":state.decision_writable})
                    }
                    "shared_decision" | "shared_decide" => {
                        let p = &request["params"];
                        let grant = state.grant.as_ref().unwrap().clone();
                        let matched = state.decisions.as_ref().unwrap().iter().position(|d|
                            ["kind","request_id","occurrence_id","details_sha256"].iter().all(|k| d[*k] == p[*k]));
                        let code = if p["expected_runtime_id"] != native_runtime || p["session_id"] != grant.session_id || p["grant_id"] != grant.grant_id { Some("grant_unavailable".to_owned()) }
                            else if method == "shared_decide" && state.decision_error.is_some() { state.decision_error.clone() }
                            else if matched.is_none() { Some("decision_stale".into()) }
                            else if method == "shared_decide" && !state.decision_writable { Some("sharing_read_only".into()) }
                            else { None };
                        if let Some(code) = code { wire_error = Some(json!({"code":code,"delivery":"rejected"})); Value::Null }
                        else if method == "shared_decision" {
                            json!({"type":"shared_decision","contract":crate::decision_native::CONTRACT,"grant_id":grant.grant_id,"session_id":grant.session_id,"decision":state.decisions.as_ref().unwrap()[matched.unwrap()]})
                        } else {
                            // Synthetic source-side compare-and-remove, not a gateway acceptance.
                            state.decisions.as_mut().unwrap().remove(matched.unwrap());
                            count.fetch_add(1,Ordering::AcqRel);
                            json!({"type":"shared_decision_applied","contract":crate::decision_native::CONTRACT,"grant_id":grant.grant_id,"session_id":grant.session_id,"request_id":p["request_id"],"occurrence_id":p["occurrence_id"],"action":p["action"]["type"]})
                        }
                    }
                    "share_association" => {
                        let grant = state.grant.as_ref().unwrap();
                        assert_eq!(request["params"], json!({"expected_runtime_id":native_runtime,
                            "session_id":grant.session_id,"grant_id":grant.grant_id}));
                        json!({"type":"share_association","contract":crate::shared_native::ASSOCIATION_CONTRACT,
                            "grant_id":grant.grant_id,"session_id":grant.session_id,"association":state.association})
                    }
                    "share_status" => {
                        assert_eq!(
                            request["params"],
                            json!({"expected_runtime_id":native_runtime})
                        );
                        json!({"type":"share_status","contract":crate::shared_native::CONTRACT,"grant":state.grant.as_ref().map(|g|
                            json!({"grant_id":g.grant_id,"session_id":g.session_id,"can_submit":g.can_submit,"lease_ms":g.lease_ms}))})
                    }
                    "shared_conversation" => {
                        let messages = json!([["msg_private", "user", "fixture", false]]);
                        let hash = format!(
                            "{:x}",
                            Sha256::digest(serde_json::to_vec(&messages).unwrap())
                        );
                        json!({"type":"shared_conversation","contract":crate::shared_native::CONTRACT,"grant_id":GRANT,
                            "conversation":{"type":"conversation","contract":"djinn.conversation-read.v1","session_id":"ses_private",
                            "messages":[{"id":"msg_private","role":"user","text":"fixture","truncated":false}],
                            "has_older":false,"truncated":false,"omitted_parts":false,"projection_sha256":hash}})
                    }
                    "shared_submit" => {
                        count.fetch_add(1, Ordering::AcqRel);
                        let text = request["params"]["text"].as_str().unwrap();
                        assert_eq!(request["params"]["expected_runtime_id"], native_runtime);
                        assert_eq!(request["params"]["session_id"], "ses_private");
                        assert_eq!(
                            request["params"]["grant_id"],
                            state.grant.as_ref().unwrap().grant_id
                        );
                        json!({"type":"shared_prompt_accepted","contract":crate::shared_native::CONTRACT,"grant_id":request["params"]["grant_id"],
                            "session_id":"ses_private","message_id":"msg_private","prompt_sha256":format!("{:x}", Sha256::digest(text.as_bytes())),"prompt_bytes":text.len()})
                    }
                    _ => panic!("unexpected method"),
                };
                let lose = state.lose_receipt && matches!(method,"shared_submit" | "shared_decide");
                let duplicate = state.duplicate_frame && method == "shared_submit";
                let delay = state.delay && method == "shared_submit";
                let association_delay_ms = if method == "share_association" { state.association_delay_ms } else { 0 };
                if state.corrupt_receipt && matches!(method,"shared_submit" | "shared_decide") {
                    result["grant_id"] = json!(OTHER);
                }
                let runtime = if state.wrong_runtime {
                    OTHER
                } else {
                    &native_runtime
                };
                let pause = match method {
                    "share_status" if state.status_pause_skip > 0 => {
                        state.status_pause_skip -= 1;
                        None
                    }
                    "share_status" => state.status_pause.take(),
                    "shared_attention" => state.attention_pause.take(),
                    _ => None,
                };
                drop(state);
                if let Some(pause) = pause {
                    let mut flags = pause.0.lock().unwrap();
                    flags.0 = true;
                    pause.1.notify_all();
                    let _ = pause.1.wait_timeout_while(flags, Duration::from_secs(2), |f| !f.1).unwrap();
                }
                if delay {
                    std::thread::sleep(Duration::from_millis(150));
                }
                if association_delay_ms != 0 {
                    std::thread::sleep(Duration::from_millis(association_delay_ms));
                }
                if lose {
                    return;
                }
                let mut envelope = json!({"version":1,"id":request["id"],"runtime_id":runtime});
                if let Some(error) = wire_error { envelope["error"] = error; } else { envelope["result"] = result; }
                let mut frame = serde_json::to_vec(&envelope)
                .unwrap();
                frame.push(b'\n');
                if duplicate {
                    frame.extend_from_within(..);
                }
                let _ = stream.write_all(&frame);
                });
                }
            });
        });
        let peer = Self {
            root,
            registry,
            socket,
            state,
            stop,
            submits,
            thread: Some(thread),
        };
        peer.publish(RUNTIME);
        peer
    }

    fn publish(&self, runtime: &str) {
        let file = self.registry.join(format!("{runtime}.json"));
        std::fs::write(&file, serde_json::to_vec(&json!({"contract":"djinn.web-discovery.v1","version":1,"native_socket":self.socket,"expected_runtime":runtime})).unwrap()).unwrap();
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn config(&self) -> Config {
        std::fs::write(
            self.root.path().join("coven.json"),
            br#"{"name":"Fixture","agents":["alpha"],"multiplexer":"herdr"}"#,
        )
        .unwrap();
        let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341","public_origin":"https://example.invalid",
            "allowed_logins":["alice","bob"],"endpoints":[{"id":"fleet","label":"Fleet","socket":"/absent/f.sock"}],
            "conversation_sources":[{"id":"one","label":"Selection","team":"research","directory":self.registry,
                "allowed_logins":["alice"],"submit_logins":["alice"],"control_workspace":self.root.path()}]})).unwrap();
        config.validate().unwrap();
        config
    }

    fn registry(&self) -> Conversations {
        Conversations::new(&self.config(), "epoch").unwrap()
    }
    fn id(&self, registry: &Conversations) -> String {
        registry.list("alice")["conversations"][0]["id"]
            .as_str()
            .unwrap()
            .into()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[path = "cardlink_tests.rs"]
mod cardlink_tests;

#[test]
fn selected_discovery_read_and_in_memory_submit_are_pinned_and_sanitized() {
    let peer = Peer::new();
    let registry = peer.registry();
    assert_eq!(registry.list("bob")["sources"], json!([]));
    assert!(peer.state.lock().unwrap().requests.is_empty());
    let id = peer.id(&registry);
    assert!(registry.submission_allowed("alice", &id));
    assert!(!registry.submission_allowed("bob", &id));
    let before = peer.state.lock().unwrap().requests.len();
    assert!(registry.read("bob", &id).is_err());
    assert!(
        registry
            .submit("bob", &id, REQUEST, "private browser text")
            .is_err()
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
    let conversation = registry.read("alice", &id).unwrap();
    assert_eq!(conversation["messages"][0]["text"], "fixture");
    assert!(!conversation.to_string().contains("ses_private"));
    let report = registry
        .submit("alice", &id, REQUEST, "private browser text")
        .unwrap();
    assert_eq!(report["outcome"], "accepted");
    assert_eq!(report["native_prompt_acceptance"], "accepted");
    for private in [
        "private browser text",
        "ses_private",
        "msg_private",
        GRANT,
        RUNTIME,
        peer.root.path().to_str().unwrap(),
    ] {
        assert!(!report.to_string().contains(private));
    }
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
    assert_eq!(
        registry
            .submit("alice", &id, REQUEST, "private browser text")
            .unwrap()["outcome"],
        "uncertain"
    );
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
    let methods = peer
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .map(|r| r["method"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        &methods[methods.len() - 3..],
        ["capabilities", "share_status", "shared_submit"]
    );
    assert!(!peer.root.path().join("prompt").exists());
    assert!(
        crate::transport_gate::TransportGuard::exclusive(peer.root.path(), Duration::ZERO).is_ok()
    );
}

#[test]
fn expired_reference_never_rebounds_and_null_status_revokes() {
    let peer = Peer::new();
    let registry = peer.registry();
    let old = peer.id(&registry);
    registry.state.lock().unwrap().refs[0].selected.expires = Instant::now();
    assert!(!registry.submission_allowed("alice", &old));
    let new = peer.id(&registry);
    assert_ne!(old, new);
    assert!(registry.read("alice", &old).is_err());
    peer.state.lock().unwrap().grant = None;
    assert!(registry.read("alice", &new).is_err());
    assert_eq!(
        registry.submit("alice", &new, REQUEST, "hello").unwrap()["delivery"],
        "unsent"
    );
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    assert!(
        registry.list("alice")["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!registry.submission_allowed("alice", &new));
}

#[test]
fn read_only_and_authority_refusal_precede_endpoint_access() {
    let peer = Peer::new();
    peer.state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .can_submit = false;
    let registry = peer.registry();
    let id = peer.id(&registry);
    assert!(!registry.submission_allowed("alice", &id));
    let before = peer.state.lock().unwrap().requests.len();
    assert!(registry.submit("alice", &id, REQUEST, "hello").is_err());
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
    peer.state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .can_submit = true;
    let id = peer.id(&registry);
    std::fs::create_dir_all(peer.root.path().join("state/actions")).unwrap();
    std::fs::write(peer.root.path().join("state/actions/orphan.json"), "{}").unwrap();
    std::fs::remove_file(&peer.socket).unwrap();
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    assert_eq!(
        registry
            .submit("alice", &id, REQUEST, "hello")
            .unwrap_err()
            .code(),
        "action_orphan_bundles"
    );
    assert_eq!(probe.count(), 0);
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
}

#[test]
fn unsafe_duplicate_and_bounded_descriptors_fail_closed() {
    use std::os::unix::fs::symlink;
    let peer = Peer::new();
    let file = peer.registry.join(format!("{RUNTIME}.json"));
    for mode in [0o644, 0o666, 0o700] {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            discovery::scan(
                peer.registry.to_str().unwrap(),
                crate::shared_native::deadline()
            )
            .is_err()
        );
    }
    peer.publish(RUNTIME);
    let original = std::fs::read(&file).unwrap();
    for raw in [
        vec![b' '; 8193],
        b"{\"version\":1,\"version\":1}".to_vec(),
        b"{}".to_vec(),
    ] {
        std::fs::write(&file, raw).unwrap();
        assert!(
            discovery::scan(
                peer.registry.to_str().unwrap(),
                crate::shared_native::deadline()
            )
            .is_err()
        );
    }
    std::fs::write(&file, &original).unwrap();
    peer.publish(OTHER);
    assert!(
        discovery::scan(
            peer.registry.to_str().unwrap(),
            crate::shared_native::deadline()
        )
        .is_err()
    );
    std::fs::remove_file(peer.registry.join(format!("{OTHER}.json"))).unwrap();
    std::fs::remove_file(&file).unwrap();
    symlink(peer.root.path().join("missing"), &file).unwrap();
    assert!(
        discovery::scan(
            peer.registry.to_str().unwrap(),
            crate::shared_native::deadline()
        )
        .is_err()
    );
    std::fs::remove_file(file).unwrap();
    for n in 0..65 {
        std::fs::write(peer.registry.join(format!("ignored{n}")), "").unwrap();
    }
    assert!(
        discovery::scan(
            peer.registry.to_str().unwrap(),
            crate::shared_native::deadline()
        )
        .is_err()
    );
    assert!(peer.state.lock().unwrap().requests.is_empty());
}

#[test]
fn stale_runtime_and_read_only_policy_cannot_grant_write() {
    let peer = Peer::new();
    peer.state.lock().unwrap().wrong_runtime = true;
    assert!(
        peer.registry().list("alice")["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    peer.state.lock().unwrap().wrong_runtime = false;
    let mut config = peer.config();
    config.conversation_sources[0].submit_logins.clear();
    config.conversation_sources[0].control_workspace = None;
    let registry = Conversations::new(&config, "epoch").unwrap();
    let id = peer.id(&registry);
    assert!(!registry.submission_allowed("alice", &id));
    assert!(registry.submit("alice", &id, REQUEST, "hello").is_err());
}

#[test]
fn lost_corrupt_and_duplicate_receipts_are_uncertain_and_suppressed_across_grants() {
    for mode in 0..3 {
        let peer = Peer::new();
        {
            let mut state = peer.state.lock().unwrap();
            state.lose_receipt = mode == 0;
            state.corrupt_receipt = mode == 1;
            state.duplicate_frame = mode == 2;
        }
        let registry = peer.registry();
        let id = peer.id(&registry);
        assert_eq!(
            registry.submit("alice", &id, REQUEST, "hello").unwrap()["delivery"],
            "uncertain"
        );
        peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = OTHER.into();
        let new = peer.id(&registry);
        assert_ne!(id, new);
        assert_eq!(
            registry.submit("alice", &new, REQUEST, "hello").unwrap()["delivery"],
            "uncertain"
        );
        assert_eq!(peer.submits.load(Ordering::Acquire), 1);
    }
}

#[test]
fn alias_singleflight_and_attempt_capacity_do_not_create_new_lanes() {
    let peer = Peer::new();
    let mut config = peer.config();
    let mut alias = config.conversation_sources[0].clone();
    alias.id = "alias".into();
    config.conversation_sources.push(alias);
    let registry = Arc::new(Conversations::new(&config, "epoch").unwrap());
    let rows = registry.list("alice");
    let one = rows["conversations"][0]["id"].as_str().unwrap().to_owned();
    let two = rows["conversations"][1]["id"].as_str().unwrap().to_owned();
    peer.state.lock().unwrap().delay = true;
    std::thread::scope(|scope| {
        let first = scope.spawn(|| registry.submit("alice", &one, REQUEST, "hello").unwrap());
        let until = Instant::now() + Duration::from_secs(2);
        while peer.submits.load(Ordering::Acquire) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(peer.submits.load(Ordering::Acquire), 1);
        assert_eq!(
            registry.submit("alice", &two, OTHER, "hello").unwrap()["diagnostics"],
            json!(["submission_busy"])
        );
        assert_eq!(first.join().unwrap()["outcome"], "accepted");
    });
    assert!(registry.state.lock().unwrap().active.is_empty());
    {
        let mut state = registry.state.lock().unwrap();
        for n in 0..4096 {
            state.attempts.insert(format!("{n}"));
        }
    }
    assert_eq!(
        registry.submit("alice", &two, GRANT, "hello").unwrap()["diagnostics"],
        json!(["request_capacity"])
    );
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
}

#[test]
fn utf8_and_json_escaped_prompt_limits_are_exact() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    let text = format!("{}{}", "\u{1f642}".repeat(8192), "\n\t\"\\".repeat(8192));
    assert_eq!(text.len(), 65536);
    let report = registry.submit("alice", &id, REQUEST, &text).unwrap();
    assert_eq!(report["outcome"], "accepted");
    assert_eq!(report["prompt_bytes"], 65536);
    assert!(registry.submit("alice", &id, OTHER, &(text + "x")).is_err());
    assert!(registry.submit("alice", &id, OTHER, "\u{85}").is_err());
    assert!(registry.submit("alice", &id, "bad", "hello").is_err());
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
}

#[test]
fn source_schema_defaults_acl_and_workspace_policy_are_closed() {
    let peer = Peer::new();
    let base = serde_json::to_value(peer.config()).unwrap();
    for (key, value) in [
        ("id", json!("../escape")),
        ("label", json!("\n")),
        ("team", json!("")),
        ("directory", json!("ssh://remote/registry")),
        ("directory", json!("/private/../registry")),
        ("allowed_logins", json!([])),
        ("allowed_logins", json!(["outsider"])),
        ("allowed_logins", json!(["alice", "alice"])),
        ("submit_logins", json!(["bob"])),
        ("submit_logins", json!(["alice", "alice"])),
        ("control_workspace", Value::Null),
        ("control_workspace", json!("relative")),
        ("control_workspace", json!(peer.root.path().join("absent"))),
    ] {
        let mut bad = base.clone();
        bad["conversation_sources"][0][key] = value;
        assert!(
            serde_json::from_value::<Config>(bad)
                .unwrap()
                .validate()
                .is_err(),
            "{key}"
        );
    }
    let mut unknown = base.clone();
    unknown["conversation_sources"][0]["native_socket"] = json!(peer.socket);
    assert!(serde_json::from_value::<Config>(unknown).is_err());
    let mut too_many = base.clone();
    too_many["conversation_sources"] = json!(vec![base["conversation_sources"][0].clone(); 9]);
    assert_eq!(
        serde_json::from_value::<Config>(too_many)
            .unwrap()
            .validate()
            .unwrap_err()
            .code(),
        "conversation_source_limit"
    );
    let mut defaults = base.clone();
    let source = defaults["conversation_sources"][0].as_object_mut().unwrap();
    source.remove("submit_logins");
    source.remove("control_workspace");
    source.insert("directory".into(), json!(peer.root.path().join("absent")));
    let config: Config = serde_json::from_value(defaults).unwrap();
    config.validate().unwrap();
    assert!(
        Conversations::new(&config, "epoch").unwrap().list("alice")["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut no_sources = base;
    no_sources
        .as_object_mut()
        .unwrap()
        .remove("conversation_sources");
    assert!(
        serde_json::from_value::<Config>(no_sources)
            .unwrap()
            .conversation_sources
            .is_empty()
    );
}

#[test]
fn native_status_rechecks_write_permission_and_lease_bounds() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    peer.state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .can_submit = false;
    let report = registry.submit("alice", &id, REQUEST, "hello").unwrap();
    assert_eq!(report["delivery"], "unsent");
    assert_eq!(report["diagnostics"], json!(["sharing_read_only"]));
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    for lease_ms in [0, 5001, u64::MAX] {
        peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = lease_ms;
        assert!(
            registry.list("alice")["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn cache_only_authorization_and_final_send_fence_do_not_touch_discovery() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    let cached = registry.cached("alice", &id).unwrap();
    std::fs::remove_dir_all(&peer.registry).unwrap();
    let before = peer.state.lock().unwrap().requests.len();
    assert!(registry.submission_allowed("alice", &id));
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
    let report = crate::session_prompts::shared_submit(
        peer.root.path(),
        &cached.selected.endpoint,
        &cached.selected.runtime,
        &cached.selected.grant,
        REQUEST,
        "hello",
        || {
            // Model a refresh/revocation after the native metadata checks.
            registry.state.lock().unwrap().refs.clear();
            registry.submission_allowed("alice", &id)
        },
    )
    .unwrap();
    assert_eq!(report["delivery"], "unsent");
    assert_eq!(report["diagnostics"], json!(["grant_unavailable"]));
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    assert!(
        crate::transport_gate::TransportGuard::exclusive(peer.root.path(), Duration::ZERO).is_ok()
    );
}

#[test]
fn same_sid_and_grant_on_different_runtimes_have_independent_flights() {
    let first = Peer::new();
    let second = Peer::new();
    second.state.lock().unwrap().runtime = OTHER.into();
    std::fs::remove_file(second.registry.join(format!("{RUNTIME}.json"))).unwrap();
    second.publish(OTHER);
    let mut config = first.config();
    let mut source = second.config().conversation_sources.remove(0);
    source.id = "second".into();
    config.conversation_sources.push(source);
    let registry = Conversations::new(&config, "epoch").unwrap();
    let list = registry.list("alice");
    let one = list["conversations"][0]["id"].as_str().unwrap();
    let two = list["conversations"][1]["id"].as_str().unwrap();
    assert_ne!(one, two);
    first.state.lock().unwrap().delay = true;
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| registry.submit("alice", one, REQUEST, "hello").unwrap());
        let until = Instant::now() + Duration::from_secs(2);
        while first.submits.load(Ordering::Acquire) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            registry.submit("alice", two, GRANT, "hello").unwrap()["outcome"],
            "accepted"
        );
        assert_eq!(pending.join().unwrap()["outcome"], "accepted");
    });
    assert_eq!(first.submits.load(Ordering::Acquire), 1);
    assert_eq!(second.submits.load(Ordering::Acquire), 1);
}

#[test]
fn enabled_runtime_authority_refuses_before_native_fence() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    std::fs::create_dir(peer.root.path().join("state")).unwrap();
    let location = crate::platform::workspace_location(peer.root.path()).unwrap();
    std::fs::write(peer.root.path().join("state/runtime-launch-control.json"), serde_json::to_vec(&json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),"epoch_id":"b".repeat(32),
        "location":{"root":location.root,"device":location.device,"inode":location.inode},"head":null,"claims":{}
    })).unwrap()).unwrap();
    let before = peer.state.lock().unwrap().requests.len();
    std::fs::remove_file(&peer.socket).unwrap();
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    assert_eq!(
        registry
            .submit("alice", &id, REQUEST, "hello")
            .unwrap_err()
            .code(),
        "legacy_runtime_disabled"
    );
    assert_eq!(probe.count(), 0);
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
}

#[test]
fn aggregate_limit_reports_omissions_and_fences_hidden_handles() {
    let peer = Peer::new();
    let mut config = peer.config();
    config.conversations = (0..64)
        .map(|n| config::ConversationConfig {
            id: format!("registered{n}"),
            label: "Registered".into(),
            team: "research".into(),
            allowed_logins: vec!["alice".into()],
            native_socket: peer.socket.to_str().unwrap().into(),
            expected_runtime: RUNTIME.into(),
            session: "ses_private".into(),
        })
        .collect();
    config.validate().unwrap();
    let registry = Conversations::new(&config, "epoch").unwrap();
    let list = registry.list("alice");
    assert_eq!(list["conversations"].as_array().unwrap().len(), 64);
    assert_eq!(list["omitted_conversations"], 1);
    assert!(
        list["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["can_submit"] == false)
    );
    // Exercise the unchanged browser validator, not a reimplementation of it.
    assert!(std::process::Command::new("node").current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--input-type=module", "-e", "import {validateConversations} from './clients/web/app.js'; if (validateConversations(JSON.parse(process.argv[1])).length !== 64) process.exit(1);"])
        .arg(list.to_string()).status().unwrap().success());
    let hidden = registry.state.lock().unwrap().refs[0].id.clone();
    {
        let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        assert!(!registry.submission_allowed("alice", &hidden));
        assert!(registry.read("alice", &hidden).is_err());
        assert!(registry.submit("alice", &hidden, REQUEST, "hello").is_err());
        assert_eq!(probe.count(), 0);
    }
    for target in &mut config.conversations {
        target.allowed_logins = vec!["bob".into()];
    }
    let registry = Conversations::new(&config, "epoch").unwrap();
    let list = registry.list("alice");
    assert_eq!(list["conversations"].as_array().unwrap().len(), 1);
    assert_eq!(list["omitted_conversations"], 0);
    assert!(registry.submission_allowed("alice", list["conversations"][0]["id"].as_str().unwrap()));
}

fn wait_for_paused_status(pause: &StatusPause) {
    let (flags, timeout) = pause
        .1
        .wait_timeout_while(pause.0.lock().unwrap(), Duration::from_secs(2), |f| !f.0)
        .unwrap();
    assert!(
        flags.0 && !timeout.timed_out(),
        "status did not reach barrier"
    );
}

fn release_status(pause: &StatusPause) {
    pause.0.lock().unwrap().1 = true;
    pause.1.notify_all();
}

#[test]
fn unchanged_refresh_keeps_live_handles_for_busy_list_read_and_submit() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let refreshing = scope.spawn(|| registry.list("alice"));
        wait_for_paused_status(&pause);
        assert!(registry.submission_allowed("alice", &id));
        let busy = registry.list("alice");
        assert_eq!(busy["sources"][0]["status"], "busy");
        assert_eq!(busy["conversations"][0]["id"], id);
        assert_eq!(
            registry.read("alice", &id).unwrap()["messages"][0]["text"],
            "fixture"
        );
        assert_eq!(
            registry.submit("alice", &id, REQUEST, "hello").unwrap()["outcome"],
            "accepted"
        );
        release_status(&pause);
        assert_eq!(refreshing.join().unwrap()["conversations"][0]["id"], id);
    });
    assert!(registry.submission_allowed("alice", &id));
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
    std::fs::set_permissions(&peer.registry, std::fs::Permissions::from_mode(0o755)).unwrap();
    let failed = registry.list("alice");
    assert_eq!(failed["sources"][0]["status"], "unavailable");
    assert!(failed["conversations"].as_array().unwrap().is_empty());
    assert!(!registry.submission_allowed("alice", &id));
    assert!(registry.state.lock().unwrap().refs.is_empty());
}

#[test]
fn expiry_during_refresh_hides_old_reference_and_cannot_rebind_it() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    registry.state.lock().unwrap().refs[0].selected.expires =
        Instant::now() + Duration::from_millis(50);
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let refreshing = scope.spawn(|| registry.list("alice"));
        wait_for_paused_status(&pause);
        std::thread::sleep(Duration::from_millis(75));
        assert!(!registry.submission_allowed("alice", &id));
        assert!(
            registry.list("alice")["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        release_status(&pause);
        let completed = refreshing.join().unwrap();
        assert_ne!(completed["conversations"][0]["id"], id);
    });
    assert!(!registry.submission_allowed("alice", &id));
}

#[test]
fn obsolete_refresh_generation_cannot_restore_a_revoked_handle() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let refreshing = scope.spawn(|| registry.list("alice"));
        wait_for_paused_status(&pause);
        {
            let mut state = registry.state.lock().unwrap();
            state.refs.clear();
            registry.sources[0]
                .generation
                .fetch_add(1, Ordering::Release);
        }
        release_status(&pause);
        assert!(
            refreshing.join().unwrap()["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    });
    assert!(!registry.submission_allowed("alice", &id));
}

#[test]
fn gate_wait_past_wire_deadline_is_unsent_and_releases_lane_without_replay() {
    let peer = Peer::new();
    let registry = peer.registry();
    let id = peer.id(&registry);
    let gate =
        crate::transport_gate::TransportGuard::exclusive(peer.root.path(), Duration::ZERO).unwrap();
    let before = peer.state.lock().unwrap().requests.len();
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| {
            let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
            let report = registry.submit("alice", &id, REQUEST, "hello").unwrap();
            assert_eq!(probe.count(), 0);
            report
        });
        let until = Instant::now() + Duration::from_secs(1);
        while registry.state.lock().unwrap().active.is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(registry.state.lock().unwrap().active.len(), 1);
        std::thread::sleep(Duration::from_millis(4300));
        drop(gate);
        let report = pending.join().unwrap();
        assert_eq!(report["delivery"], "unsent");
        assert_eq!(report["diagnostics"], json!(["timeout"]));
    });
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    assert!(registry.state.lock().unwrap().active.is_empty());
    assert!(registry.state.lock().unwrap().attempts.contains(REQUEST));
    let fresh = peer.id(&registry);
    assert_eq!(
        registry.submit("alice", &fresh, REQUEST, "hello").unwrap()["diagnostics"],
        json!(["request_already_seen"])
    );
    assert!(
        crate::transport_gate::TransportGuard::exclusive(peer.root.path(), Duration::ZERO).is_ok()
    );
}
