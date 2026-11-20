use super::*;

#[path = "attention_fence_tests.rs"]
mod fences;

#[test]
fn unavailable_authorized_competitor_is_not_uniqueness_proof_or_cached_authority() {
    for sample_ambiguity in [false, true] {
        let (peer, kit, mut observer) = setup();
        let other = Peer::new();
        let other_grant = other.state.lock().unwrap().grant.take();
        other.state.lock().unwrap().association = Some(kit.association());
        let mut config = peer.config();
        config.conversation_sources[0].decision_logins = vec!["alice".into()];
        let mut second = other.config().conversation_sources.remove(0);
        second.id = "competitor".into();
        config.conversation_sources.push(second);
        observer.conversations = Conversations::new(&config, "epoch").unwrap();
        let row = item(&observer);
        other.state.lock().unwrap().grant = other_grant;
        if sample_ambiguity {
            let list = observer.conversation_list("alice");
            assert!(
                list["conversations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["association_status"] == "ambiguous")
            );
        }
        std::fs::remove_file(&other.socket).unwrap();
        // Even before a card refresh commits, failed discovery revokes cached authority.
        observer.conversations.list("alice");
        assert!(!observer.decision_response_allowed("alice", id(&row)));
        assert!(
            observer
                .verify_card_binding(
                    "alice",
                    row["fleet_agent_id"].as_str().unwrap(),
                    row["conversation_id"].as_str().unwrap(),
                    row["binding_id"].as_str().unwrap()
                )
                .is_err()
        );
        let list = observer.conversation_list("alice");
        assert!(
            list["conversations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["association_status"] != "verified")
        );
        assert_eq!(observer.attention("alice")["items"], json!([]));
        assert_eq!(
            observer
                .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
                .unwrap()["delivery"],
            "unsent"
        );
        assert_eq!(peer.submits.load(Ordering::Acquire), 0);
        // An ACL-excluded descriptor is not a competitor for this principal.
        config.conversation_sources[1].allowed_logins = vec!["bob".into()];
        config.conversation_sources[1].submit_logins.clear();
        observer.conversations = Conversations::new(&config, "epoch").unwrap();
        assert_eq!(item(&observer)["can_respond"], true);
    }
}

fn large_pending() -> Vec<Value> {
    (0..32).map(|n| {
        // Quotes are valid native IDs and exercise worst-case JSON escaping.
        let request=format!("{}{n:03}","\"".repeat(509));
        let occurrence=format!("{n:08x}-2222-4333-8444-555555555555");
        let patterns=vec!["x".repeat(900);32];
        let tuple=json!([crate::decision_native::CONTRACT,"permission",request,occurrence,"shell",patterns,[],"{}",null,"session_pending"]);
        let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(&tuple).unwrap()));
        json!({"kind":"permission","request_id":request,"occurrence_id":occurrence,"details_sha256":digest,
            "permission":"shell","patterns":patterns,"always_patterns":[],"metadata_json":"{}","tool":null,"deny_scope":"session_pending"})
    }).collect()
}

#[test]
fn large_valid_detail_survives_two_large_summaries_and_response_workflow() {
    let (peer, _kit, observer) = setup();
    let pending = large_pending();
    assert!((29000..32768).contains(&serde_json::to_vec(&pending[0]).unwrap().len()));
    peer.state.lock().unwrap().decisions = Some(pending);
    let a = observer.attention("alice");
    assert_eq!(a["items"].as_array().unwrap().len(), 32, "{a}");
    let row = &a["items"][0];
    let d = observer.decision_details("alice", id(row)).unwrap();
    assert_eq!(d["decision"]["patterns"].as_array().unwrap().len(), 32);
    assert_eq!(
        observer
            .respond_decision("alice", id(row), OTHER, &json!({"type":"permission_once"}))
            .unwrap()["outcome"],
        "applied"
    );
}

#[test]
fn decision_aggregate_and_individual_frame_limits_remain_enforced() {
    use crate::{decision_native, session_prompts::fresh_id, shared_native::Connection};
    let (peer, _kit, observer) = setup();
    peer.state.lock().unwrap().decisions = Some(large_pending());
    let a = observer.attention("alice");
    let selected = observer
        .conversations
        .selected("alice", a["items"][0]["conversation_id"].as_str().unwrap())
        .unwrap();
    let mut c = Connection::decision_workflow(
        &selected.endpoint,
        &selected.runtime,
        crate::shared_native::deadline(),
    );
    let mut successes = 0;
    loop {
        match c.call(
            "shared_attention",
            c.params(&selected.grant),
            &fresh_id().unwrap(),
        ) {
            Ok(_) => successes += 1,
            Err(e) => {
                assert_eq!(e.code, "response_limit");
                break;
            }
        }
        assert!(successes < 8);
    }
    assert!(successes >= 3);
    let mut normal = Connection::new(
        &selected.endpoint,
        &selected.runtime,
        crate::shared_native::deadline(),
    );
    normal
        .call(
            "shared_attention",
            normal.params(&selected.grant),
            &fresh_id().unwrap(),
        )
        .unwrap();
    assert_eq!(
        normal
            .call(
                "shared_attention",
                normal.params(&selected.grant),
                &fresh_id().unwrap()
            )
            .unwrap_err()
            .code,
        "response_limit"
    );
    let mut c = Connection::decision_workflow(
        &selected.endpoint,
        &selected.runtime,
        crate::shared_native::deadline(),
    );
    let meta = decision_native::summary(&mut c, &selected.grant)
        .unwrap()
        .requests
        .remove(0);
    peer.state.lock().unwrap().decisions.as_mut().unwrap()[0]["patterns"] =
        json!(["x".repeat(70000)]);
    assert_eq!(
        c.call(
            "shared_decision",
            meta.params(&c, &selected.grant),
            &fresh_id().unwrap()
        )
        .unwrap_err()
        .code,
        "response_limit"
    );
}

#[test]
fn streams_are_scope_specific_complete_only_when_untruncated_and_include_unlinked_occurrences() {
    let (peer, _kit, observer) = setup();
    let a = observer.attention("alice");
    let stream = a["streams"][0].clone();
    assert_eq!(
        stream["id"],
        digest(&["alice", "one", RUNTIME, "ses_private"])
    );
    assert_eq!(stream["alert_ids"], json!([a["items"][0]["alert_id"]]));
    assert_eq!(stream["complete"], true);
    {
        let mut s = peer.state.lock().unwrap();
        s.association = None;
        s.decisions.as_mut().unwrap()[0]["details_sha256"] = Value::Null;
    }
    let a = observer.attention("alice");
    assert_eq!(a["items"], json!([]));
    assert_eq!(a["streams"][0], stream);
    for (complete, omitted) in [(false, None), (true, Some(1))] {
        let mut s = peer.state.lock().unwrap();
        s.decision_complete = complete;
        s.decision_omitted = omitted;
        drop(s);
        let a = observer.attention("alice");
        assert_eq!(a["streams"][0]["complete"], false);
        assert_eq!(a["streams"][0]["alert_ids"], stream["alert_ids"]);
    }
    {
        let mut s = peer.state.lock().unwrap();
        s.decision_complete = true;
        s.decision_omitted = Some(0);
        s.decisions = Some(vec![]);
    }
    let a = observer.attention("alice");
    assert_eq!(a["streams"][0]["complete"], true);
    assert_eq!(a["streams"][0]["alert_ids"], json!([]));
    peer.state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .session_id = "ses_other".into();
    let a = observer.attention("alice");
    assert_eq!(a["state"], "observed");
    assert_ne!(a["streams"][0]["id"], stream["id"]);
    assert_eq!(observer.attention("bob")["streams"], json!([]));
    peer.state.lock().unwrap().grant = None;
    assert_eq!(observer.attention("alice")["streams"], json!([]));
}

#[test]
fn orphan_authority_and_invalid_workspace_reports_are_known_unsent_without_connections() {
    for mode in ["orphan", "workspace"] {
        let (peer, _kit, observer) = setup();
        let row = item(&observer);
        if mode == "orphan" {
            std::fs::create_dir_all(peer.root.path().join("state/actions")).unwrap();
            std::fs::write(peer.root.path().join("state/actions/orphan.json"), "{}").unwrap();
        } else {
            std::fs::remove_file(peer.root.path().join("coven.json")).unwrap();
        }
        let calls = peer.state.lock().unwrap().requests.len();
        let probe = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        let r = observer
            .respond_decision("alice", id(&row), OTHER, &json!({"type":"question_reject"}))
            .unwrap();
        assert_eq!(r["outcome"], "unavailable");
        assert_eq!(r["delivery"], "unsent");
        if mode == "orphan" {
            assert_eq!(r["diagnostics"], json!(["action_orphan_bundles"]));
        }
        assert!(!r.to_string().contains(peer.root.path().to_str().unwrap()));
        assert_eq!(probe.count(), 0);
        assert_eq!(peer.state.lock().unwrap().requests.len(), calls);
    }
}
