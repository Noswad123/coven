use super::*;

#[path = "attention_regression_tests.rs"]
mod regressions;

#[path = "metadata_regression_tests.rs"]
mod metadata_regressions;

fn pending() -> Value {
    let digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([
                crate::decision_native::CONTRACT,
                "question",
                "ask_private",
                REQUEST,
                [["Choose", "Header", [["one", "Description"]], false, false]],
                null
            ]))
            .unwrap()
        )
    );
    json!({"kind":"question","request_id":"ask_private","occurrence_id":REQUEST,"details_sha256":digest,
        "questions":[{"question":"Choose","header":"Header","options":[{"label":"one","description":"Description"}],"multiple":false,"custom":false}],"tool":null})
}
fn setup() -> (Peer, Kit, Observer) {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    {
        let mut s = peer.state.lock().unwrap();
        s.association = Some(kit.association());
        s.decisions = Some(vec![pending()]);
        s.grant.as_mut().unwrap().can_submit = false;
    }
    let mut observer = kit.observer(&peer, 1);
    let mut config = peer.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    (peer, kit, observer)
}
fn item(observer: &Observer) -> Value {
    let attention = observer.attention("alice");
    assert_eq!(attention["state"], "observed", "{attention}");
    assert_eq!(
        attention["items"].as_array().unwrap().len(),
        1,
        "{attention}"
    );
    attention["items"][0].clone()
}
fn id(row: &Value) -> &str {
    row["id"].as_str().unwrap()
}

#[test]
fn shared_metadata_scheduler_pairs_documents_and_push_reads_without_native_recollection() {
    use crate::observer::metadata::MetadataService;
    let (peer, kit, observer) = setup();
    let observer = Arc::new(observer);
    let service = MetadataService::start(&peer.config(), &observer).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let attention = loop {
        let attention = service.read("alice", true);
        if attention["state"] == "observed" {
            break attention;
        }
        assert!(Instant::now() < deadline, "{attention}");
        std::thread::sleep(Duration::from_millis(5));
    };
    let calls = peer.state.lock().unwrap().requests.len();
    let kits = kit.requests.lock().unwrap().len();
    let list = service.read("alice", false);
    assert_eq!(
        attention["items"][0]["conversation_id"],
        list["conversations"][0]["id"]
    );
    assert_eq!(
        attention["freshness"]["generation"],
        list["freshness"]["generation"]
    );
    assert!(attention["freshness"]["remaining_ms"].as_u64().unwrap() <= 5000);
    // This is also the exact cache-only callback used by PushService.
    for _ in 0..50 {
        assert_eq!(service.read("alice", true)["items"], attention["items"]);
        assert_eq!(
            service.read("alice", false)["conversations"],
            list["conversations"]
        );
    }
    assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    assert_eq!(kit.requests.lock().unwrap().len(), kits);
    // Unchanged fleet renewal neither discards this pair nor renews its deadline.
    let endpoint = &observer.endpoints[0];
    let inventory = endpoint.cache.lock().unwrap().inventory.clone().unwrap();
    endpoint.accept("epoch", inventory);
    let unchanged = service.read("alice", true);
    assert_eq!(unchanged["state"], "observed");
    assert_eq!(
        unchanged["freshness"]["generation"],
        attention["freshness"]["generation"]
    );
    assert!(
        unchanged["freshness"]["remaining_ms"].as_u64().unwrap()
            <= attention["freshness"]["remaining_ms"].as_u64().unwrap()
    );
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !matches!(
                r["method"].as_str(),
                Some("shared_conversation" | "conversation" | "shared_submit" | "decision_details")
            ))
    );
    // Lease renewal publishes a new generation even when displayed references agree.
    let until = Instant::now() + Duration::from_secs(5);
    let renewed = loop {
        let next = service.read("alice", true);
        if next["freshness"]["generation"] != attention["freshness"]["generation"] {
            break next;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(renewed["items"], attention["items"]);
    assert!(renewed["freshness"]["remaining_ms"].as_u64().unwrap() > 0);
    // An explicit synchronous refresh supersedes both documents together.
    observer.attention("alice");
    for attention in [true, false] {
        let stale = service.read("alice", attention);
        assert_eq!(stale["state"], "unavailable");
        assert_eq!(stale["freshness"]["remaining_ms"], 0);
    }
    peer.state.lock().unwrap().wrong_runtime = true;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let failed = service.read("alice", true);
        if failed["freshness"]["generation"] != renewed["freshness"]["generation"] {
            assert_eq!(failed["state"], "unavailable");
            assert_eq!(failed["freshness"]["remaining_ms"], 0);
            assert_eq!(failed["streams"], json!([]));
            let list = service.read("alice", false);
            assert_eq!(list["conversations"], json!([]));
            assert_eq!(list["freshness"]["remaining_ms"], 0);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    drop(service);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn exact_detail_and_applied_response_without_followup_model_authority_or_transcripts() {
    let (peer, _kit, observer) = setup();
    let row = item(&observer);
    assert_eq!(row.as_object().unwrap().len(), 10);
    assert_eq!(row["can_respond"], true);
    assert!(observer.decision_response_allowed("alice", id(&row)));
    assert!(
        !observer
            .conversations
            .submission_allowed("alice", row["conversation_id"].as_str().unwrap())
    );
    let detail = observer.decision_details("alice", id(&row)).unwrap();
    assert_eq!(
        detail["decision"],
        json!({"kind":"question","questions":pending()["questions"]})
    );
    let response = observer
        .respond_decision(
            "alice",
            id(&row),
            OTHER,
            &json!({"type":"question_answer","answers":[["one"]]}),
        )
        .unwrap();
    assert_eq!(response["outcome"], "applied");
    assert_eq!(response["delivery"], "applied");
    assert_eq!(response["action"], "question_answer");
    assert!(
        peer.state
            .lock()
            .unwrap()
            .decisions
            .as_ref()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap()["outcome"],
        "uncertain"
    );
    assert!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| !matches!(
                r["method"].as_str(),
                Some("shared_submit" | "shared_conversation" | "conversation")
            ))
    );
    for private in [
        "ask_private",
        REQUEST,
        "details_sha256",
        "request_id",
        "occurrence_id",
    ] {
        assert!(!serde_json::to_string(&detail).unwrap().contains(private));
    }
}

#[test]
fn source_acl_principal_ref_grant_and_card_fences_precede_native_io() {
    let (peer, kit, observer) = setup();
    let row = item(&observer);
    let calls = peer.state.lock().unwrap().requests.len();
    assert!(observer.decision_details("bob", id(&row)).is_err());
    assert!(!observer.decision_response_allowed("bob", id(&row)));
    assert_eq!(
        observer
            .respond_decision("bob", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap()["delivery"],
        "unsent"
    );
    assert_eq!(observer.attention("bob")["state"], "unavailable");
    assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    assert!(!observer.decision_response_allowed("alice", &"a".repeat(64)));
    kit.state.lock().unwrap()["pane"] = json!("replaced");
    assert!(observer.decision_details("alice", id(&row)).is_err());
    assert!(!observer.decision_response_allowed("alice", id(&row)));
}

#[test]
fn unsupported_unknown_coverage_unlinked_and_confirmed_zero_are_distinct() {
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decisions = None;
    let a = observer.attention("alice");
    assert_eq!(a["state"], "unavailable");
    assert!(a["omitted_count"].is_null());
    {
        let mut s = peer.state.lock().unwrap();
        s.decisions = Some(vec![]);
        s.decision_complete = false;
        s.decision_omitted = None;
    }
    let a = observer.attention("alice");
    assert_eq!(a["state"], "partial");
    assert!(a["omitted_count"].is_null());
    {
        let mut s = peer.state.lock().unwrap();
        s.decision_complete = true;
        s.decision_omitted = Some(0);
    }
    let a = observer.attention("alice");
    assert_eq!(a["state"], "observed");
    assert_eq!(a["omitted_count"], 0);
    {
        let mut s = peer.state.lock().unwrap();
        s.decisions = Some(vec![pending()]);
        s.association = None;
    }
    let a = observer.attention("alice");
    assert_eq!(a["unlinked_count"], 1);
    assert_eq!(a["items"], json!([]));
    assert_eq!(a["state"], "partial");
}

#[test]
fn unreachable_discovery_target_is_unknown_not_confirmed_zero() {
    let (peer, _kit, observer) = setup();
    let _row = item(&observer);
    std::fs::remove_file(&peer.socket).unwrap();
    let a = observer.attention("alice");
    assert_eq!(a["state"], "unavailable");
    assert!(a["omitted_count"].is_null());
    assert_eq!(a["unavailable_count"], 1);
    assert_eq!(a["items"], json!([]));
}

#[test]
fn default_writer_acl_and_native_readmode_are_independent_and_cache_only() {
    let (peer, kit, mut observer) = setup();
    let config = peer.config();
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let row = item(&observer);
    assert_eq!(row["can_respond"], false);
    let calls = peer.state.lock().unwrap().requests.len();
    assert!(!observer.decision_response_allowed("alice", id(&row)));
    assert_eq!(
        observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"permission_once"}))
            .unwrap()["delivery"],
        "unsent"
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    assert!(observer.decision_details("alice", id(&row)).is_ok());
    let _keep_kit = &kit;
}

#[test]
fn native_rejection_lost_receipt_and_invalid_receipt_have_honest_delivery() {
    for mode in ["reject", "lost", "invalid"] {
        let (peer, _kit, observer) = setup();
        let row = item(&observer);
        {
            let mut s = peer.state.lock().unwrap();
            s.lose_receipt = mode == "lost";
            s.corrupt_receipt = mode == "invalid";
            if mode == "reject" {
                s.decision_writable = false;
            }
        }
        let r = observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap();
        if mode == "reject" {
            assert_eq!(r["delivery"], "unsent");
            assert_eq!(peer.submits.load(Ordering::Acquire), 0);
        } else {
            assert_eq!(r["outcome"], "uncertain");
            assert_eq!(r["delivery"], "uncertain");
            assert_eq!(peer.submits.load(Ordering::Acquire), 1);
        }
    }
}

#[test]
fn stable_alerts_survive_grant_and_daemon_epochs_but_refs_do_not() {
    let (peer, _kit, mut observer) = setup();
    let old = item(&observer);
    peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = OTHER.into();
    let new = item(&observer);
    assert_eq!(old["alert_id"], new["alert_id"]);
    assert_ne!(old["id"], new["id"]);
    assert!(observer.decision_details("alice", id(&old)).is_err());
    let mut config = peer.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    observer.conversations = Conversations::new(&config, "other-daemon-epoch").unwrap();
    let restarted = item(&observer);
    assert_eq!(new["alert_id"], restarted["alert_id"]);
    assert_ne!(new["id"], restarted["id"]);
    peer.state.lock().unwrap().decisions.as_mut().unwrap()[0]["occurrence_id"] = json!(GRANT);
    let next = item(&observer);
    assert_ne!(new["alert_id"], next["alert_id"]);
}

#[test]
fn response_runtime_authority_refuses_before_socket_construction() {
    let (peer, _kit, observer) = setup();
    let row = item(&observer);
    std::fs::create_dir(peer.root.path().join("state")).unwrap();
    let location = crate::platform::workspace_location(peer.root.path()).unwrap();
    std::fs::write(peer.root.path().join("state/runtime-launch-control.json"),serde_json::to_vec(&json!({
        "version":1,"action":"runtime_launch","workspace_id":"a".repeat(32),"epoch_id":"b".repeat(32),
        "location":{"root":location.root,"device":location.device,"inode":location.inode},"head":null,"claims":{}
    })).unwrap()).unwrap();
    let before = peer.state.lock().unwrap().requests.len();
    let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
    let report = observer
        .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
        .unwrap();
    assert_eq!(report["outcome"], "unavailable");
    assert_eq!(report["delivery"], "unsent");
    assert_eq!(report["diagnostics"], json!(["legacy_runtime_disabled"]));
    assert_eq!(probe.count(), 0);
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
}

#[test]
fn global_attempt_suppression_spans_methods_and_changed_references_and_alias_flights() {
    let (peer, _kit, observer) = setup();
    peer.state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .can_submit = true;
    let row = item(&observer);
    let conversation = row["conversation_id"].as_str().unwrap();
    assert_eq!(
        observer
            .conversations
            .submit("alice", conversation, OTHER, "followup")
            .unwrap()["outcome"],
        "accepted"
    );
    assert_eq!(
        observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap()["diagnostics"],
        json!(["request_already_seen"])
    );
    peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = RUNTIME.into();
    let changed = item(&observer);
    assert_ne!(row["id"], changed["id"]);
    assert_eq!(
        observer
            .respond_decision(
                "alice",
                id(&changed),
                OTHER,
                &json!({"type":"question_reject"})
            )
            .unwrap()["diagnostics"],
        json!(["request_already_seen"])
    );
    let selected = observer
        .conversations
        .selected("alice", changed["conversation_id"].as_str().unwrap())
        .unwrap();
    let active = observer
        .conversations
        .claim(GRANT, &selected.runtime, &selected.grant.session_id)
        .unwrap();
    assert_eq!(
        observer
            .respond_decision(
                "alice",
                id(&changed),
                REQUEST,
                &json!({"type":"question_reject"})
            )
            .unwrap()["diagnostics"],
        json!(["submission_busy"])
    );
    drop(active);
    let attempt = "51111111-2222-4333-8444-555555555555";
    assert_eq!(
        observer
            .respond_decision(
                "alice",
                id(&changed),
                attempt,
                &json!({"type":"question_reject"})
            )
            .unwrap()["outcome"],
        "applied"
    );
    assert_eq!(
        observer
            .conversations
            .submit(
                "alice",
                changed["conversation_id"].as_str().unwrap(),
                attempt,
                "followup"
            )
            .unwrap()["diagnostics"],
        json!(["request_already_seen"])
    );
}

#[test]
fn source_cas_rejection_is_not_applied_or_uncertain() {
    let (peer, _kit, observer) = setup();
    let row = item(&observer);
    peer.state.lock().unwrap().decision_error = Some("decision_stale".into());
    let r = observer
        .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
        .unwrap();
    assert_eq!(r["outcome"], "rejected");
    assert_eq!(r["delivery"], "rejected");
    assert_eq!(r["diagnostics"], json!(["decision_stale"]));
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    assert_eq!(
        peer.state.lock().unwrap().decisions.as_ref().unwrap().len(),
        1
    );
}

#[test]
fn unsupported_details_and_invalid_action_cannot_submit_or_expose_partial_details() {
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decisions.as_mut().unwrap()[0]["details_sha256"] = Value::Null;
    let row = item(&observer);
    assert_eq!(row["details_available"], false);
    assert_eq!(row["can_respond"], false);
    let calls = peer.state.lock().unwrap().requests.len();
    assert_eq!(
        observer
            .decision_details("alice", id(&row))
            .unwrap_err()
            .code(),
        "decision_unavailable"
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    peer.state.lock().unwrap().decisions = Some(vec![pending()]);
    let row = item(&observer);
    for action in [
        json!({"type":"permission_always"}),
        json!({"type":"question_reject","extra":true}),
        json!({"type":"permission_deny","scope":"future"}),
    ] {
        let calls = peer.state.lock().unwrap().requests.len();
        assert_eq!(
            observer
                .respond_decision("alice", id(&row), OTHER, &action)
                .unwrap_err()
                .code(),
            "invalid_decision"
        );
        assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    }
    let response = observer
        .respond_decision("alice", id(&row), OTHER, &json!({"type":"permission_once"}))
        .unwrap();
    assert_eq!(response["delivery"], "unsent");
    assert_eq!(response["diagnostics"], json!(["invalid_decision"]));
    assert_eq!(peer.submits.load(Ordering::Acquire), 0);
}

#[test]
fn in_flight_refresh_preserves_valid_cache_but_loser_cannot_publish_refs() {
    let (peer, _kit, observer) = setup();
    let row = item(&observer);
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let first = scope.spawn(|| observer.attention("alice"));
        wait_for_paused_status(&pause);
        assert!(observer.decision_response_allowed("alice", id(&row)));
        let second = observer.attention("alice");
        assert_eq!(second["items"][0]["id"], row["id"]);
        release_status(&pause);
        let losing = first.join().unwrap();
        assert_eq!(losing["state"], "unavailable");
        assert_eq!(losing["items"], json!([]));
    });
    assert!(observer.decision_response_allowed("alice", id(&row)));
}

#[test]
fn decision_acl_config_is_default_empty_subset_readers_and_requires_control_context() {
    let peer = Peer::new();
    let mut config = peer.config();
    assert!(config.conversation_sources[0].decision_logins.is_empty());
    config.conversation_sources[0].decision_logins = vec!["bob".into()];
    assert!(config.validate().is_err());
    config.conversation_sources[0].decision_logins = vec!["alice".into(), "alice".into()];
    assert!(config.validate().is_err());
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    config.validate().unwrap();
    config.conversation_sources[0].submit_logins.clear();
    config.conversation_sources[0].control_workspace = None;
    assert!(config.validate().is_err());
}

#[test]
fn permission_once_uses_decision_acl_not_followup_eligibility_and_preserves_all_patterns() {
    let (peer, _kit, observer) = setup();
    let tuple = json!([
        crate::decision_native::CONTRACT,
        "permission",
        "perm_private",
        REQUEST,
        "shell",
        ["first/**", "second/**"],
        ["native-only/**"],
        "{}",
        null,
        "session_pending"
    ]);
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&tuple).unwrap()));
    peer.state.lock().unwrap().decisions = Some(vec![
        json!({"kind":"permission","request_id":"perm_private","occurrence_id":REQUEST,"details_sha256":digest,
        "permission":"shell","patterns":["first/**","second/**"],"always_patterns":["native-only/**"],"metadata_json":"{}","tool":null,"deny_scope":"session_pending"}),
    ]);
    let row = item(&observer);
    assert_eq!(row["can_respond"], true);
    let details = observer.decision_details("alice", id(&row)).unwrap();
    assert_eq!(
        details["decision"]["patterns"],
        json!(["first/**", "second/**"])
    );
    assert_eq!(details["decision"]["deny_scope"], "session_pending");
    assert_eq!(
        observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"permission_once"}))
            .unwrap()["outcome"],
        "applied"
    );
}

#[test]
fn native_read_mode_can_inspect_but_cannot_respond() {
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decision_writable = false;
    let row = item(&observer);
    assert_eq!(row["can_respond"], false);
    assert!(!observer.decision_response_allowed("alice", id(&row)));
    assert_eq!(
        observer.decision_details("alice", id(&row)).unwrap()["can_respond"],
        false
    );
    let before = peer.state.lock().unwrap().requests.len();
    assert_eq!(
        observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap()["delivery"],
        "unsent"
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
}

#[test]
fn expired_or_replaced_grant_or_meaning_cannot_return_details() {
    for mode in ["grant", "meaning", "occurrence"] {
        let (peer, _kit, observer) = setup();
        let row = item(&observer);
        {
            let mut s = peer.state.lock().unwrap();
            match mode {
                "grant" => s.grant.as_mut().unwrap().grant_id = OTHER.into(),
                "meaning" => {
                    s.decisions.as_mut().unwrap()[0]["questions"][0]["question"] = json!("Changed")
                }
                _ => s.decisions.as_mut().unwrap()[0]["occurrence_id"] = json!(OTHER),
            }
        }
        assert!(
            observer.decision_details("alice", id(&row)).is_err(),
            "{mode}"
        );
        assert_eq!(peer.submits.load(Ordering::Acquire), 0);
    }
}

#[test]
fn metadata_cap_is_combined_across_native_targets_and_reports_known_omissions() {
    let (first, kit1, mut observer) = setup();
    let second = Peer::new();
    let kit2 = Kit::new(&second);
    {
        let mut s = second.state.lock().unwrap();
        s.association = Some(kit2.association());
        s.decisions = Some(vec![pending()]);
    }
    for peer in [&first, &second] {
        peer.state.lock().unwrap().decisions = Some(
            (0..20)
                .map(|n| {
                    let mut v = pending();
                    v["request_id"] = json!(format!("ask{n}"));
                    v["occurrence_id"] = json!(format!("{n:08x}-2222-4333-8444-555555555555"));
                    v
                })
                .collect(),
        );
    }
    let mut config = first.config();
    config.conversation_sources[0].decision_logins = vec!["alice".into()];
    let mut other = second.config().conversation_sources.remove(0);
    other.id = "second".into();
    config.conversation_sources.push(other);
    let mut second_observer = kit2.observer(&second, 1);
    let mut endpoint = second_observer.endpoints.remove(0);
    Arc::get_mut(&mut endpoint).unwrap().config.id = "second-kit".into();
    config.endpoints.push(endpoint.config.clone());
    observer.endpoints.push(endpoint);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let a = observer.attention("alice");
    assert_eq!(a["items"].as_array().unwrap().len(), 32, "{a}");
    assert_eq!(a["omitted_count"], 8);
    assert_eq!(a["streams"].as_array().unwrap().len(), 2);
    assert_eq!(
        a["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["alert_ids"].as_array().unwrap().len())
            .sum::<usize>(),
        40
    );
    assert_eq!(a["state"], "partial");
    let _keep_kit = &kit1;
}
