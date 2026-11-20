use super::*;

#[test]
fn lost_applied_receipt_stays_uncertain_after_attention_removes_reference() {
    let (peer, _kit, observer) = setup();
    let row = item(&observer);
    let action = json!({"type":"question_reject"});
    peer.state.lock().unwrap().lose_receipt = true;
    let first = observer
        .respond_decision("alice", id(&row), OTHER, &action)
        .unwrap();
    assert_eq!(first["outcome"], "uncertain");
    assert_eq!(first["delivery"], "uncertain");
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
    assert!(
        peer.state
            .lock()
            .unwrap()
            .decisions
            .as_ref()
            .unwrap()
            .is_empty()
    );
    assert_eq!(observer.attention("alice")["items"], json!([]));
    assert!(!observer.decision_response_allowed("alice", id(&row)));
    let calls = peer.state.lock().unwrap().requests.len();
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    assert!(observer.decision_details("alice", id(&row)).is_err());
    let replay = observer
        .respond_decision("alice", id(&row), OTHER, &action)
        .unwrap();
    assert_eq!(replay["outcome"], "uncertain");
    assert_eq!(replay["delivery"], "uncertain");
    assert_eq!(replay["diagnostics"], json!(["request_already_seen"]));
    assert!(!observer.conversations.known_attempt(REQUEST));
    let fresh = observer
        .respond_decision("alice", id(&row), REQUEST, &action)
        .unwrap();
    assert_eq!(fresh["outcome"], "unavailable");
    assert_eq!(fresh["delivery"], "unsent");
    assert!(!observer.conversations.known_attempt(REQUEST));
    assert_eq!(probe.count(), 0);
    assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    assert_eq!(peer.submits.load(Ordering::Acquire), 1);
}

#[test]
fn candidate_commit_during_last_card_verification_blocks_native_decide() {
    let (peer, kit, mut observer) = setup();
    let other = Peer::new();
    let grant = other.state.lock().unwrap().grant.take();
    other.state.lock().unwrap().association = Some(kit.association());
    let mut config = peer.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    let mut second = other.config().conversation_sources.remove(0);
    second.id = "second".into();
    config.conversation_sources.push(second);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let row = item(&observer);
    let before = observer.conversations.card_proof("alice");
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    // Each enrichment has two snapshot/inspect pairs. Pause the LAST response check.
    *kit.pause.lock().unwrap() = Some((4, pause.clone()));
    std::thread::scope(|scope| {
        let response = scope.spawn(|| {
            observer
                .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
                .unwrap()
        });
        wait_for_paused_status(&pause);
        other.state.lock().unwrap().grant = grant;
        let list = observer.conversations.list("alice");
        assert_eq!(list["conversations"].as_array().unwrap().len(), 2);
        assert_ne!(observer.conversations.card_proof("alice"), before);
        assert!(!observer.decision_response_allowed("alice", id(&row)));
        release_status(&pause);
        let result = response.join().unwrap();
        assert_eq!(result["outcome"], "unavailable");
        assert_eq!(result["delivery"], "unsent");
    });
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| r["method"] != "shared_decide")
    );
    assert!(!observer.bound(
        "alice",
        row["fleet_agent_id"].as_str().unwrap(),
        row["conversation_id"].as_str().unwrap(),
        row["binding_id"].as_str().unwrap()
    ));
}

#[test]
fn unchanged_refresh_and_excluded_principal_commit_preserve_held_proof() {
    let (peer, kit, mut observer) = setup();
    let other = Peer::new();
    other.state.lock().unwrap().grant = None;
    let mut config = peer.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    let mut second = other.config().conversation_sources.remove(0);
    second.id = "second".into();
    second.allowed_logins = vec!["bob".into()];
    second.submit_logins.clear();
    config.conversation_sources.push(second);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let row = item(&observer);
    let proof = observer.conversations.card_proof("alice");
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    *kit.pause.lock().unwrap() = Some((0, pause.clone()));
    std::thread::scope(|scope| {
        let verification = scope.spawn(|| {
            observer.verify_card_binding(
                "alice",
                row["fleet_agent_id"].as_str().unwrap(),
                row["conversation_id"].as_str().unwrap(),
                row["binding_id"].as_str().unwrap(),
            )
        });
        wait_for_paused_status(&pause);
        observer.conversations.list("alice");
        observer.conversations.list("bob");
        assert_eq!(observer.conversations.card_proof("alice"), proof);
        assert!(observer.decision_response_allowed("alice", id(&row)));
        release_status(&pause);
        verification.join().unwrap().unwrap();
    });
    assert!(observer.decision_response_allowed("alice", id(&row)));
}

#[test]
fn candidate_aba_and_expiration_cannot_reuse_an_old_proof() {
    let (peer, kit, mut observer) = setup();
    let other = Peer::new();
    let grant = other.state.lock().unwrap().grant.take();
    let mut config = peer.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    let mut second = other.config().conversation_sources.remove(0);
    second.id = "second".into();
    config.conversation_sources.push(second);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let row = item(&observer);
    let proof = observer.conversations.card_proof("alice");
    other.state.lock().unwrap().grant = grant;
    observer.conversations.list("alice");
    {
        let mut state = observer.conversations.state.lock().unwrap();
        state
            .refs
            .iter_mut()
            .find(|r| r.source == 1)
            .unwrap()
            .selected
            .expires = Instant::now();
    }
    assert!(observer.conversations.card_proof("alice").is_none());
    other.state.lock().unwrap().grant = None;
    observer.conversations.list("alice");
    assert_ne!(observer.conversations.card_proof("alice"), proof);
    assert!(!observer.decision_response_allowed("alice", id(&row)));
    let new = item(&observer);
    assert_ne!(new["binding_id"], row["binding_id"]);
    let _keep = &kit;
}

#[test]
fn final_attention_fence_drops_assembled_revoked_items_refs_and_streams() {
    for replace in [false, true] {
        let (peer, _kit, mut observer) = setup();
        let second = Peer::new();
        let kit2 = Kit::new(&second);
        {
            let mut s = second.state.lock().unwrap();
            s.association = Some(kit2.association());
            s.decisions = Some(vec![pending()]);
        }
        let mut config = peer.config();
        config.conversation_sources[0].decision_logins = vec!["alice".into()];
        let mut source = second.config().conversation_sources.remove(0);
        source.id = "second".into();
        config.conversation_sources.push(source);
        let mut second_observer = kit2.observer(&second, 1);
        let mut endpoint = second_observer.endpoints.remove(0);
        Arc::get_mut(&mut endpoint).unwrap().config.id = "second-kit".into();
        config.endpoints.push(endpoint.config.clone());
        observer.endpoints.push(endpoint);
        observer.conversations = Conversations::new(&config, "epoch").unwrap();
        let initial = observer.attention("alice");
        let old = initial["items"][0].clone();
        assert_eq!(initial["items"].as_array().unwrap().len(), 2);
        let stream = digest(&["alice", "one", RUNTIME, "ses_private"]);
        let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
        second.state.lock().unwrap().attention_pause = Some(pause.clone());
        std::thread::scope(|scope| {
            let collecting = scope.spawn(|| observer.attention("alice"));
            wait_for_paused_status(&pause);
            if replace {
                peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = OTHER.into();
            } else {
                peer.state.lock().unwrap().grant = None;
            }
            observer.conversations.list("alice");
            release_status(&pause);
            let result = collecting.join().unwrap();
            assert_eq!(result["state"], "partial");
            assert!(result["omitted_count"].is_null());
            assert!(
                result["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["conversation_id"] != old["conversation_id"])
            );
            assert!(
                result["streams"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|s| s["id"] != stream)
            );
            // B remains a valid cardless stream, not global proof about A.
            assert_eq!(result["streams"].as_array().unwrap().len(), 1);
            assert_eq!(result["streams"][0]["complete"], true);
        });
        assert!(!observer.decision_response_allowed("alice", id(&old)));
        let calls = peer.state.lock().unwrap().requests.len();
        let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        let report = observer
            .respond_decision(
                "alice",
                id(&old),
                REQUEST,
                &json!({"type":"question_reject"}),
            )
            .unwrap();
        assert_eq!(report["delivery"], "unsent");
        assert_eq!(probe.count(), 0);
        assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
        assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    }
}

#[test]
fn preflight_then_reference_or_policy_invalidation_returns_known_unsent() {
    for mode in ["grant", "ref", "permission"] {
        let (peer, _kit, observer) = setup();
        let row = item(&observer);
        assert!(observer.decision_response_allowed("alice", id(&row)));
        match mode {
            "grant" => {
                peer.state.lock().unwrap().grant = None;
                observer.conversations.list("alice");
            }
            "ref" => {
                peer.state.lock().unwrap().decisions = Some(vec![]);
                observer.attention("alice");
            }
            _ => {
                peer.state.lock().unwrap().decision_writable = false;
                observer.attention("alice");
            }
        }
        let calls = peer.state.lock().unwrap().requests.len();
        let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        let r = observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap();
        assert_eq!(r["outcome"], "unavailable");
        assert_eq!(r["delivery"], "unsent");
        assert_eq!(r["action"], "question_reject");
        assert_eq!(probe.count(), 0);
        assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    }
}
