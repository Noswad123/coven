use super::*;
use crate::observer::metadata::MetadataService;

fn unavailable_candidate(peer: &Peer, runtime: &str) {
    let file = peer.registry.join(format!("{runtime}.json"));
    std::fs::write(&file, serde_json::to_vec(&json!({"contract":"djinn.web-discovery.v1","version":1,
        "native_socket":peer.root.path().join(format!("absent-{}", &runtime[..8])),"expected_runtime":runtime})).unwrap()).unwrap();
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn partial_metadata_retains_valid_rows_and_unknown_attention_without_card_uniqueness() {
    let (peer, _kit, observer) = setup();
    unavailable_candidate(&peer, OTHER);
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    for (serial, pending_request, complete) in [(1, false, true), (2, true, true), (3, true, false)]
    {
        {
            let mut state = peer.state.lock().unwrap();
            state.decisions = Some(if pending_request {
                vec![pending()]
            } else {
                vec![]
            });
            state.decision_complete = complete;
            state.decision_omitted = complete.then_some(0);
        }
        let original = observer.attention("alice");
        let original_list = observer.conversation_list("alice");
        assert_eq!(original_list["conversations"].as_array().unwrap().len(), 1);
        assert_eq!(original_list["sources"][0]["status"], "observed");
        assert!(observer.conversations.card_proof("alice").is_none());
        assert_eq!(original["state"], "partial");
        assert_eq!(original["streams"].as_array().unwrap().len(), 1);
        assert_eq!(original["items"], json!([]));
        assert_eq!(original["unavailable_count"], 1);
        assert_eq!(original["omitted_count"], Value::Null);
        service.refresh("alice", serial);
        let list = service.read("alice", false);
        let attention = service.read("alice", true);
        assert_eq!(
            list["conversations"], original_list["conversations"],
            "{list}"
        );
        assert_eq!(list["sources"], original_list["sources"]);
        assert_eq!(
            list["conversations"][0]["association_status"],
            "unavailable"
        );
        assert!(list["conversations"][0]["card"].is_null());
        for field in [
            "state",
            "streams",
            "items",
            "unavailable_count",
            "unlinked_count",
            "omitted_count",
        ] {
            assert_eq!(attention[field], original[field], "{attention}");
        }
        assert_eq!(attention["streams"][0]["complete"], complete);
        assert_eq!(attention["unlinked_count"], usize::from(pending_request));
        for document in [&list, &attention] {
            let remaining = document["freshness"]["remaining_ms"].as_u64().unwrap();
            assert!(remaining > 0 && remaining <= 5000);
        }
        assert_eq!(
            list["freshness"]["generation"],
            attention["freshness"]["generation"]
        );
        assert!(!observer.decision_response_allowed("alice", &"a".repeat(64)));
    }
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !matches!(
                r["method"].as_str(),
                Some(
                    "shared_conversation"
                        | "conversation"
                        | "shared_decision"
                        | "shared_decide"
                        | "shared_submit"
                )
            ))
    );
}

#[test]
fn partial_metadata_keeps_healthy_source_when_another_source_fails() {
    let (peer, _kit, mut observer) = setup();
    peer.state.lock().unwrap().decisions = Some(vec![]);
    let mut config = peer.config();
    let mut failed = config.conversation_sources[0].clone();
    failed.id = "unavailable".into();
    std::fs::write(peer.root.path().join("failed-registry"), b"not a directory").unwrap();
    failed.directory = peer
        .root
        .path()
        .join("failed-registry")
        .to_str()
        .unwrap()
        .into();
    config.conversation_sources.push(failed);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    let original = observer.attention("alice");
    let original_list = observer.conversation_list("alice");
    assert_eq!(original_list["conversations"].as_array().unwrap().len(), 1);
    assert_eq!(original_list["sources"][0]["status"], "observed");
    assert_eq!(original_list["sources"][1]["status"], "unavailable");
    assert!(observer.conversations.card_proof("alice").is_none());
    service.refresh("alice", 1);
    let list = service.read("alice", false);
    let attention = service.read("alice", true);
    assert_eq!(
        list["conversations"], original_list["conversations"],
        "{list}"
    );
    assert_eq!(list["sources"], original_list["sources"]);
    assert!(list["conversations"][0]["card"].is_null());
    assert_eq!(attention["state"], "partial");
    assert_eq!(attention["streams"], original["streams"]);
    assert_eq!(
        attention["unavailable_count"],
        original["unavailable_count"]
    );
    assert!(list["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    assert!(attention["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
}

#[test]
fn partial_metadata_provenance_distinguishes_coverage_identity_and_revocation_with_no_card_proof() {
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decisions = Some(vec![]);
    unavailable_candidate(&peer, OTHER);
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let first = service.read("alice", false);
    let original_id = first["conversations"][0]["id"].clone();
    assert!(first["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    assert!(observer.conversations.card_proof("alice").is_none());

    unavailable_candidate(&peer, REQUEST);
    observer.conversations.list("alice");
    assert!(observer.conversations.card_proof("alice").is_none());
    for attention in [false, true] {
        let superseded = service.read("alice", attention);
        assert_eq!(
            superseded["freshness"]["generation"],
            first["freshness"]["generation"]
        );
        assert_eq!(superseded["freshness"]["remaining_ms"], 0);
    }
    service.refresh("alice", 2);
    let changed_coverage = service.read("alice", false);
    assert_eq!(changed_coverage["conversations"][0]["id"], original_id);
    assert_eq!(service.read("alice", true)["unavailable_count"], 2);
    assert!(
        changed_coverage["freshness"]["remaining_ms"]
            .as_u64()
            .unwrap()
            > 0
    );

    peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = OTHER.into();
    observer.conversations.list("alice");
    assert!(observer.conversations.card_proof("alice").is_none());
    assert_eq!(service.read("alice", false)["freshness"]["remaining_ms"], 0);
    service.refresh("alice", 3);
    let changed_identity = service.read("alice", false);
    assert_ne!(changed_identity["conversations"][0]["id"], original_id);
    assert!(changed_identity["conversations"][0]["card"].is_null());
    assert!(
        changed_identity["freshness"]["remaining_ms"]
            .as_u64()
            .unwrap()
            > 0
    );

    let grant = peer.state.lock().unwrap().grant.take().unwrap();
    observer.conversations.list("alice");
    assert_eq!(service.read("alice", false)["freshness"]["remaining_ms"], 0);
    service.refresh("alice", 4);
    for attention in [false, true] {
        let unavailable = service.read("alice", attention);
        assert_eq!(unavailable["state"], "unavailable");
        assert_eq!(unavailable["freshness"]["remaining_ms"], 0);
        assert_eq!(
            unavailable[if attention {
                "streams"
            } else {
                "conversations"
            }],
            json!([])
        );
    }
    peer.state.lock().unwrap().grant = Some(grant);
    service.refresh("alice", 5);
    let restored = service.read("alice", false);
    assert_ne!(
        restored["conversations"][0]["id"],
        changed_identity["conversations"][0]["id"]
    );
    assert!(restored["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    assert!(observer.conversations.card_proof("alice").is_none());
}

#[test]
fn partial_metadata_expires_at_the_known_grant_lease_and_does_not_reuse_expired_ids() {
    let (peer, _kit, observer) = setup();
    {
        let mut state = peer.state.lock().unwrap();
        state.decisions = Some(vec![]);
        state.grant.as_mut().unwrap().lease_ms = 250;
    }
    unavailable_candidate(&peer, OTHER);
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let first = service.read("alice", false);
    let old_id = first["conversations"][0]["id"].clone();
    assert!(old_id.is_string());
    for attention in [false, true] {
        let document = service.read("alice", attention);
        let remaining = document["freshness"]["remaining_ms"].as_u64().unwrap();
        assert!(remaining > 0 && remaining <= 250);
    }
    std::thread::sleep(Duration::from_millis(260));
    for attention in [false, true] {
        let document = service.read("alice", attention);
        assert_eq!(
            document["freshness"]["generation"],
            first["freshness"]["generation"]
        );
        assert_eq!(document["freshness"]["remaining_ms"], 0);
        assert_eq!(document["state"], "unavailable");
    }
    peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 5000;
    service.refresh("alice", 2);
    let renewed = service.read("alice", false);
    assert_ne!(renewed["conversations"][0]["id"], old_id);
    assert!(renewed["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
}

#[test]
fn unavailable_decision_summary_does_not_hide_a_valid_conversation_list() {
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decisions = None;
    let original = observer.conversation_list("alice");
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let list = service.read("alice", false);
    assert_eq!(list["conversations"], original["conversations"]);
    assert!(list["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    let attention = service.read("alice", true);
    assert_eq!(attention["state"], "unavailable");
    assert_eq!(attention["freshness"]["remaining_ms"], 0);
    assert_eq!(attention["streams"], json!([]));
    assert_eq!(attention["items"], json!([]));
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !matches!(
                r["method"].as_str(),
                Some(
                    "shared_conversation"
                        | "conversation"
                        | "shared_decision"
                        | "shared_decide"
                        | "shared_submit"
                )
            ))
    );
}

#[test]
fn shorter_shared_source_lease_clamps_both_documents_without_changing_generation() {
    let (peer, _kit, mut observer) = setup();
    let mut config = peer.config();
    config.conversation_sources[0].allowed_logins = vec!["alice".into(), "bob".into()];
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let original = service.read("alice", true);
    assert_eq!(original["state"], "observed");
    assert!(original["freshness"]["remaining_ms"].as_u64().unwrap() > 200);
    let grant = peer.state.lock().unwrap().grant.clone().unwrap();
    peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 200;
    service.refresh("bob", 2);
    assert_eq!(
        peer.state.lock().unwrap().grant.as_ref().unwrap().grant_id,
        grant.grant_id
    );
    assert_eq!(service.read("bob", true)["state"], "observed");
    let short = [false, true].map(|attention| {
        let current = service.read("alice", attention);
        assert_eq!(
            current["freshness"]["generation"],
            original["freshness"]["generation"]
        );
        let remaining = current["freshness"]["remaining_ms"].as_u64().unwrap();
        assert!(remaining > 0 && remaining <= 200, "{current}");
        current
    });
    // Even a subsequent longer native renewal cannot restore the old deadline.
    peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 5000;
    observer.conversations.list("bob");
    for (attention, before) in [false, true].into_iter().zip(&short) {
        let current = service.read("alice", attention);
        assert_eq!(
            current["freshness"]["generation"],
            original["freshness"]["generation"]
        );
        assert!(
            current["freshness"]["remaining_ms"].as_u64().unwrap()
                <= before["freshness"]["remaining_ms"].as_u64().unwrap()
        );
    }
    std::thread::sleep(Duration::from_millis(210));
    for attention in [false, true] {
        let expired = service.read("alice", attention);
        assert_eq!(
            expired["freshness"]["generation"],
            original["freshness"]["generation"]
        );
        assert_eq!(expired["freshness"]["remaining_ms"], 0);
        assert_eq!(expired["state"], "unavailable");
    }
}

#[test]
fn another_principals_renewal_never_extends_the_original_snapshot_deadline() {
    let (peer, _kit, mut observer) = setup();
    peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 1500;
    let mut config = peer.config();
    config.conversation_sources[0].allowed_logins = vec!["alice".into(), "bob".into()];
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let alice = service.read("alice", true);
    assert_eq!(alice["state"], "observed");
    let original_deadline = Instant::now()
        + Duration::from_millis(alice["freshness"]["remaining_ms"].as_u64().unwrap());
    std::thread::sleep(Duration::from_millis(600));
    service.refresh("bob", 2);
    assert_eq!(service.read("alice", true)["state"], "observed");
    assert_eq!(service.read("bob", true)["state"], "observed");
    std::thread::sleep(
        original_deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
    );
    let expired = service.read("alice", true);
    assert_eq!(
        expired["freshness"]["generation"],
        alice["freshness"]["generation"]
    );
    assert_eq!(expired["freshness"]["remaining_ms"], 0);
    assert_eq!(expired["state"], "unavailable");
    assert_eq!(service.read("bob", true)["state"], "observed");
}

#[test]
fn shared_source_principals_keep_committed_snapshots_during_scans_and_unchanged_renewals() {
    let (peer, _kit, mut observer) = setup();
    let mut config = peer.config();
    config.conversation_sources[0].allowed_logins = vec!["alice".into(), "bob".into()];
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let alice = service.read("alice", true);
    assert_eq!(alice["state"], "observed", "{alice}");
    service.refresh("bob", 2);
    let bob = service.read("bob", true);
    assert_eq!(bob["state"], "observed", "{bob}");
    assert_eq!(
        alice["items"][0]["conversation_id"],
        bob["items"][0]["conversation_id"]
    );
    let assert_live = |login: &str, before: &Value| {
        for attention in [true, false] {
            let current = service.read(login, attention);
            assert_ne!(current["state"], "unavailable", "{login}: {current}");
            assert_eq!(
                current["freshness"]["generation"],
                before["freshness"]["generation"]
            );
            let remaining = current["freshness"]["remaining_ms"].as_u64().unwrap();
            assert!(
                remaining > 0 && remaining <= before["freshness"]["remaining_ms"].as_u64().unwrap()
            );
        }
    };
    // Exercise a real in-flight source acquisition, not an artificial fence edit.
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let scan = scope.spawn(|| observer.conversations.list("bob"));
        wait_for_pause(&pause);
        assert_live("alice", &alice);
        assert_live("bob", &bob);
        resume(&pause);
        assert_eq!(scan.join().unwrap()["sources"][0]["status"], "observed");
    });
    for _ in 0..2 {
        observer.conversations.list("alice");
        observer.conversations.list("bob");
        assert_live("alice", &alice);
        assert_live("bob", &bob);
    }
    service.refresh("bob", 3);
    assert_live("alice", &alice);
    let bob = service.read("bob", true);
    service.refresh("alice", 4);
    assert_live("bob", &bob);
    let alice = service.read("alice", true);
    assert_eq!(alice["state"], "observed");
    // A native grant revocation invalidates both committed snapshots at once.
    peer.state.lock().unwrap().grant = None;
    observer.conversations.list("bob");
    for login in ["alice", "bob"] {
        for attention in [true, false] {
            let current = service.read(login, attention);
            assert_eq!(current["state"], "unavailable");
            assert_eq!(current["freshness"]["remaining_ms"], 0);
        }
    }
}

#[test]
fn collected_old_list_cannot_borrow_a_concurrent_linked_refresh_binding_proof() {
    let (peer, _kit, observer) = setup();
    let observer = Arc::new(observer);
    let first = observer.conversation_list("alice")["conversations"][0].clone();
    let card = &first["card"];
    let service = MetadataService::manual(&observer);
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    observer.attention.lock().unwrap().collected_list_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let collection = scope.spawn(|| service.refresh("alice", 1));
        wait_for_pause(&pause);
        peer.state.lock().unwrap().association.as_mut().unwrap()["receiver_epoch"] =
            json!("b".repeat(32));
        // The explicit linked read refreshes metadata, rotates the binding, and
        // refuses the stale click before requesting any transcript.
        assert!(
            observer
                .linked_conversation(
                    "alice",
                    card["fleet_agent_id"].as_str().unwrap(),
                    first["id"].as_str().unwrap(),
                    card["binding_id"].as_str().unwrap()
                )
                .is_err()
        );
        let current = observer.conversation_list("alice");
        assert_ne!(
            current["conversations"][0]["card"]["binding_id"],
            card["binding_id"]
        );
        resume(&pause);
        collection.join().unwrap();
    });
    for attention in [true, false] {
        let current = service.read("alice", attention);
        assert_eq!(current["state"], "unavailable");
        assert_eq!(current["freshness"]["remaining_ms"], 0);
    }
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !matches!(
                r["method"].as_str(),
                Some("shared_conversation" | "conversation")
            ))
    );
    service.refresh("alice", 2);
    let current = service.read("alice", false);
    assert_ne!(
        current["conversations"][0]["card"]["binding_id"],
        card["binding_id"]
    );
    assert!(current["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
}

#[test]
fn ssh_churn_is_not_card_authority_but_all_local_competitors_are_fenced() {
    let (peer, _kit, mut observer) = setup();
    let inventory = observer.endpoints[0]
        .cache
        .lock()
        .unwrap()
        .inventory
        .clone()
        .unwrap();
    let mut remote_config = observer.endpoints[0].config.clone();
    remote_config.id = "remote".into();
    remote_config.ssh = Some(config::SshConfig {
        target: "remote".into(),
        helper: "coven".into(),
    });
    let remote = Arc::new(Endpoint {
        config: remote_config.clone(),
        cache: Mutex::default(),
        busy: AtomicBool::new(false),
        refresh_due: AtomicBool::new(false),
    });
    remote_config.id = "competitor".into();
    remote_config.ssh = None;
    let competitor = Arc::new(Endpoint {
        config: remote_config,
        cache: Mutex::default(),
        busy: AtomicBool::new(false),
        refresh_due: AtomicBool::new(false),
    });
    observer
        .endpoints
        .extend([remote.clone(), competitor.clone()]);
    let observer = Arc::new(observer);
    let service = MetadataService::manual(&observer);
    service.refresh("alice", 1);
    let first = service.read("alice", true);
    assert_eq!(first["state"], "observed", "{first}");
    for _ in 0..3 {
        remote.accept("remote-epoch", inventory.clone());
        remote.fail("timeout");
        let current = service.read("alice", true);
        assert_eq!(current["state"], "observed");
        assert_eq!(
            current["freshness"]["generation"],
            first["freshness"]["generation"]
        );
        assert!(
            current["freshness"]["remaining_ms"].as_u64().unwrap()
                <= first["freshness"]["remaining_ms"].as_u64().unwrap()
        );
    }
    competitor.accept("local-epoch", inventory);
    assert_eq!(service.read("alice", true)["freshness"]["remaining_ms"], 0);
    // Both native candidates now claim this card, so the next real join is ambiguous.
    service.refresh("alice", 2);
    let current = service.read("alice", false);
    assert_eq!(
        current["conversations"][0]["association_status"],
        "ambiguous"
    );
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| r["method"] != "shared_conversation")
    );
}
