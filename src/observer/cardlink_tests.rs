use super::*;

#[path = "attention_workflow_tests.rs"]
mod attention_workflow_tests;

struct Kit {
    socket: String,
    state: Arc<Mutex<Value>>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pause: Arc<Mutex<Option<(usize, StatusPause)>>>,
}

impl Kit {
    fn new(peer: &Peer) -> Self {
        let socket = peer.root.path().join("k").to_str().unwrap().to_owned();
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(
            json!({"pane":"pane","runtime":RUNTIME,"availability":"live"}),
        ));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let pause: Arc<Mutex<Option<(usize, StatusPause)>>> = Arc::new(Mutex::new(None));
        let paused = pause.clone();
        let (s, r, stopping) = (state.clone(), requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = String::new();
                BufReader::new(&stream).read_line(&mut raw).unwrap();
                if raw.is_empty() {
                    continue;
                }
                let request: Value = serde_json::from_str(&raw).unwrap();
                let method = request["method"].as_str().unwrap();
                r.lock().unwrap().push(method.to_owned());
                let state = s.lock().unwrap();
                let pane = json!({"terminal_id":"terminal","pane_id":state["pane"],"workspace_id":"workspace","tab_id":"tab"});
                let result = match method {
                    "session.snapshot" => json!({"type":"session_snapshot","snapshot":{
                        "protocol":18,"version":"fixture-v1","terminal_navigation":{"version":1},
                        "panes":[pane],"agents":[{"terminal_id":"terminal","pane_id":state["pane"],
                            "workspace_id":"workspace","tab_id":"tab","agent_status":"idle","agent":"djinn"}]}}),
                    "terminal.inspect" => {
                        json!({"type":"terminal_inspected","terminal_id":"terminal",
                        "runtime_generation":state["runtime"],"availability":state["availability"],
                        "workspace_id":"workspace","tab_id":"tab","pane_id":state["pane"]})
                    }
                    _ => panic!("unexpected Kit method {method}"),
                };
                drop(state);
                let wait = {
                    let mut slot = paused.lock().unwrap();
                    match slot.as_mut() {
                        Some((skip, _)) if *skip > 0 => {
                            *skip -= 1;
                            None
                        }
                        Some(_) => slot.take().map(|(_, pause)| pause),
                        None => None,
                    }
                };
                if let Some(wait) = wait {
                    let mut flags = wait.0.lock().unwrap();
                    flags.0 = true;
                    wait.1.notify_all();
                    let _ = wait
                        .1
                        .wait_timeout_while(flags, Duration::from_secs(2), |f| !f.1)
                        .unwrap();
                }
                let mut bytes =
                    serde_json::to_vec(&json!({"id":request["id"],"result":result})).unwrap();
                bytes.push(b'\n');
                let _ = stream.write_all(&bytes);
            }
        });
        Self {
            socket,
            state,
            requests,
            stop,
            thread: Some(thread),
            pause,
        }
    }

    fn association(&self) -> Value {
        let endpoint = crate::observation_socket::select(&self.socket).unwrap();
        json!({"instance_id":OTHER,"session_id":"ses_private","receiver_epoch":"a".repeat(32),
            "scope":"local_full_tui_root","lease_ms":5000,
            "target":{"socket":endpoint.resolved,"device":endpoint.device,"inode":endpoint.inode,
                "terminal_id":"terminal","runtime_generation":RUNTIME}})
    }

    fn observer(&self, peer: &Peer, aliases: usize) -> Observer {
        let mut config = peer.config();
        config.endpoints = (0..aliases)
            .map(|n| EndpointConfig {
                id: format!("kit{n}"),
                label: "Kit".into(),
                socket: self.socket.clone(),
                ssh: None,
            })
            .collect();
        let endpoint = crate::observation_socket::select(&self.socket).unwrap();
        let generation = digest(&[
            &endpoint.resolved,
            &endpoint.device.to_string(),
            &endpoint.inode.to_string(),
            "fixture-v1",
            "18",
        ]);
        let endpoints = config
            .endpoints
            .iter()
            .map(|config| {
                let e = Arc::new(super::super::super::Endpoint {
                    config: config.clone(),
                    cache: Mutex::new(Cache::default()),
                    busy: AtomicBool::new(false),
                    refresh_due: AtomicBool::new(false),
                });
                e.accept(
                    "epoch",
                    Inventory {
                        generation: generation.clone(),
                        agents: vec![model::Agent {
                            terminal_id: "terminal".into(),
                            pane_id: "pane".into(),
                            workspace_id: "workspace".into(),
                            tab_id: "tab".into(),
                            title: "Djinn".into(),
                            provider: Some("djinn".into()),
                            project: None,
                            activity: "idle".into(),
                            evidence: "kitsune_snapshot".into(),
                            provider_candidates: vec!["djinn".into()],
                        }],
                        discovery_status: "unsupported".into(),
                        unclassified_panes: 0,
                    },
                );
                e
            })
            .collect();
        Observer {
            attention: Mutex::new(super::super::super::attention::State::default()),
            conversations: Conversations::new(&config, "epoch").unwrap(),
            cardlinks: Mutex::new(cardlinks::State::default()),
            endpoints,
            allow_output: false,
            transport: Arc::new(NativeTransport),
            shutdown: Arc::new((Mutex::new(false), Condvar::new())),
            pollers: Vec::new(),
        }
    }
}

impl Drop for Kit {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn pause_enrichment(peer: &Peer) -> StatusPause {
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let mut state = peer.state.lock().unwrap();
    state.status_pause = Some(pause.clone());
    // Pass discovery's status, then hold the first grant check inside enrichment.
    state.status_pause_skip = 1;
    pause
}

fn wait_for_pause(pause: &StatusPause) {
    let (flags, wait) = pause
        .1
        .wait_timeout_while(pause.0.lock().unwrap(), Duration::from_secs(1), |flags| {
            !flags.0
        })
        .unwrap();
    assert!(!wait.timed_out() && flags.0);
}

fn resume(pause: &StatusPause) {
    pause.0.lock().unwrap().1 = true;
    pause.1.notify_all();
}

#[test]
fn cardlinks_verified_stable_private_metadata_and_stale_click_never_reads_content() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = kit.observer(&peer, 1);
    assert!(
        observer.conversation_list("bob")["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(kit.requests.lock().unwrap().is_empty());
    assert!(peer.state.lock().unwrap().requests.is_empty());
    let list = observer.conversation_list("alice");
    let row = &list["conversations"][0];
    assert_eq!(row["association_status"], "verified", "{list}");
    let id = row["id"].as_str().unwrap();
    let agent = row["card"]["fleet_agent_id"].as_str().unwrap();
    let binding = row["card"]["binding_id"].as_str().unwrap();
    assert!(valid_id(agent) && valid_id(binding));
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["card"],
        row["card"]
    );
    for private in [
        "ses_private",
        RUNTIME,
        GRANT,
        OTHER,
        REQUEST,
        &kit.socket,
        "terminal",
        "runtime_generation",
    ] {
        assert!(!list.to_string().contains(private), "{private}");
    }
    assert!(
        !peer
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|r| r["method"] == "shared_conversation")
    );
    assert_eq!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .unwrap()["messages"][0]["text"],
        "fixture"
    );
    let reads = || {
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"] == "shared_conversation")
            .count()
    };
    let before = reads();
    kit.state.lock().unwrap()["runtime"] = json!(OTHER);
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_err()
    );
    assert_eq!(reads(), before);
    kit.state.lock().unwrap()["runtime"] = json!(RUNTIME);
    let restored = observer.conversation_list("alice");
    assert_ne!(restored["conversations"][0]["card"]["binding_id"], binding);
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_err()
    );
    assert_eq!(reads(), before);
}

#[test]
fn cardlinks_old_capability_fallback_and_configured_local_only() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    let observer = kit.observer(&peer, 1);
    let list = observer.conversation_list("alice");
    assert_eq!(
        list["conversations"][0]["association_status"],
        "unavailable"
    );
    assert!(kit.requests.lock().unwrap().is_empty());
    assert!(
        !peer
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|r| r["method"] == "share_association")
    );
    let id = list["conversations"][0]["id"].as_str().unwrap();
    assert!(observer.conversations.read("alice", id).is_ok());
    peer.state.lock().unwrap().association = Some(kit.association());
    let mut observer = kit.observer(&peer, 1);
    Arc::get_mut(&mut observer.endpoints[0]).unwrap().config.ssh = Some(config::SshConfig {
        target: "remote".into(),
        helper: "coven".into(),
    });
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "unavailable"
    );
    assert!(kit.requests.lock().unwrap().is_empty());
}

#[test]
fn cardlinks_wrong_identity_mapping_expiry_aliases_and_grant_invalidate() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let aliases = kit.observer(&peer, 2);
    assert_eq!(
        aliases.conversation_list("alice")["conversations"][0]["association_status"],
        "ambiguous"
    );
    let observer = kit.observer(&peer, 1);
    for (pointer, wrong) in [
        ("/target/inode", json!(1)),
        ("/target/socket", json!("/not/configured")),
        ("/target/terminal_id", json!("other")),
        ("/target/runtime_generation", json!(OTHER)),
    ] {
        let mut association = kit.association();
        *association.pointer_mut(pointer).unwrap() = wrong;
        peer.state.lock().unwrap().association = Some(association);
        assert_eq!(
            observer.conversation_list("alice")["conversations"][0]["association_status"],
            "unavailable",
            "{pointer}"
        );
    }
    peer.state.lock().unwrap().association = Some(kit.association());
    kit.state.lock().unwrap()["pane"] = json!("other");
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "unavailable"
    );
    kit.state.lock().unwrap()["pane"] = json!("pane");
    observer.endpoints[0].cache.lock().unwrap().fresh = None;
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "unavailable"
    );
    peer.state.lock().unwrap().grant = None;
    assert!(
        observer.conversation_list("alice")["conversations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cardlinks_same_card_authorized_conversations_are_all_ambiguous() {
    let peer = Peer::new();
    let other = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let mut association = kit.association();
    association["session_id"] = json!("ses_other");
    other.state.lock().unwrap().association = Some(association);
    other
        .state
        .lock()
        .unwrap()
        .grant
        .as_mut()
        .unwrap()
        .session_id = "ses_other".into();
    let mut observer = kit.observer(&peer, 1);
    let mut config = peer.config();
    let mut second = config.conversation_sources[0].clone();
    second.id = "second".into();
    second.team = "otherteam".into();
    second.directory = other.registry.to_str().unwrap().into();
    config.conversation_sources.push(second);
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let list = observer.conversation_list("alice");
    assert_eq!(list["conversations"].as_array().unwrap().len(), 2);
    assert!(
        list["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["association_status"] == "ambiguous" && row["card"].is_null())
    );
    config.conversation_sources[1].allowed_logins = vec!["bob".into()];
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "verified"
    );
}

#[test]
fn cardlinks_replaced_configured_endpoint_is_not_proven_by_stable_terminal_id() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = kit.observer(&peer, 1);
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "verified"
    );
    std::fs::rename(&kit.socket, peer.root.path().join("old-kit")).unwrap();
    let replacement = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(replacement.association());
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "unavailable"
    );
}

#[test]
fn cardlinks_actual_http_fake_kit_native_exact_route_and_late_stale_click() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = Arc::new(kit.observer(&peer, 1));
    let request = |path: &str, login| http::tests::card_request(observer.clone(), path, login);
    assert_eq!(request("/api/v1/conversations", None).0, 403);
    assert!(peer.state.lock().unwrap().requests.is_empty());
    let (status, list) = request("/api/v1/conversations", Some("alice"));
    assert_eq!(status, 200);
    let row = &list["conversations"][0];
    assert_eq!(row["association_status"], "verified", "{list}");
    let path = format!(
        "/api/v1/agents/{}/conversations/{}/bindings/{}/messages",
        row["card"]["fleet_agent_id"].as_str().unwrap(),
        row["id"].as_str().unwrap(),
        row["card"]["binding_id"].as_str().unwrap()
    );
    assert!(
        !peer
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|r| r["method"] == "shared_conversation")
    );
    assert_eq!(request(&path, Some("bob")).0, 404);
    let (status, messages) = request(&path, Some("alice"));
    assert_eq!(status, 200);
    assert_eq!(messages["messages"][0]["text"], "fixture");
    let reads = peer
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r["method"] == "shared_conversation")
        .count();
    kit.state.lock().unwrap()["runtime"] = json!(OTHER);
    assert_eq!(request(&path, Some("alice")).0, 409);
    assert_eq!(
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"] == "shared_conversation")
            .count(),
        reads
    );
    assert_eq!(request("/api/v1/fleet", Some("alice")).0, 200);
}

#[test]
fn cardlinks_refresh_start_keeps_live_exact_binding_and_grant_change_fences_it() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = kit.observer(&peer, 1);
    let list = observer.conversation_list("alice");
    let row = &list["conversations"][0];
    let id = row["id"].as_str().unwrap();
    let agent = row["card"]["fleet_agent_id"].as_str().unwrap();
    let binding = row["card"]["binding_id"].as_str().unwrap();
    let pause = Arc::new((Mutex::new((false, false)), Condvar::new()));
    peer.state.lock().unwrap().status_pause = Some(pause.clone());
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| observer.conversation_list("alice"));
        let (flags, wait) = pause
            .1
            .wait_timeout_while(pause.0.lock().unwrap(), Duration::from_secs(1), |flags| {
                !flags.0
            })
            .unwrap();
        assert!(!wait.timed_out() && flags.0);
        drop(flags);
        assert!(
            observer
                .linked_conversation("alice", agent, id, binding)
                .is_ok()
        );
        pause.0.lock().unwrap().1 = true;
        pause.1.notify_all();
        assert_eq!(
            refresh.join().unwrap()["conversations"][0]["card"]["binding_id"],
            binding
        );
    });
    peer.state.lock().unwrap().grant.as_mut().unwrap().grant_id = OTHER.into();
    let reads = || {
        peer.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r["method"] == "shared_conversation")
            .count()
    };
    let before = reads();
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_err()
    );
    assert_eq!(reads(), before);
    assert_ne!(
        observer.conversation_list("alice")["conversations"][0]["id"],
        id
    );
}

#[test]
fn cardlinks_total_metadata_budget_and_expired_provenance_preserve_native_rows() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    let mut association = kit.association();
    association["lease_ms"] = json!(1);
    peer.state.lock().unwrap().association = Some(association);
    peer.state.lock().unwrap().association_delay_ms = 5;
    let observer = kit.observer(&peer, 1);
    assert_eq!(
        observer.conversation_list("alice")["conversations"][0]["association_status"],
        "unavailable"
    );
    peer.state.lock().unwrap().association = Some(kit.association());
    peer.state.lock().unwrap().association_delay_ms = 2100;
    let started = Instant::now();
    let list = observer.conversation_list("alice");
    assert!(started.elapsed() < Duration::from_millis(2800));
    assert_eq!(list["conversations"].as_array().unwrap().len(), 1);
    assert_eq!(
        list["conversations"][0]["association_status"],
        "unavailable"
    );
    assert!(
        !peer
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|r| r["method"] == "shared_conversation")
    );
}

#[test]
fn cardlinks_other_principal_does_not_supersede_held_native_enrichment() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = kit.observer(&peer, 1);
    let first = observer.conversation_list("alice");
    let pause = pause_enrichment(&peer);
    std::thread::scope(|scope| {
        let alice = scope.spawn(|| observer.conversation_list("alice"));
        wait_for_pause(&pause);
        assert!(
            observer.conversation_list("bob")["conversations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        resume(&pause);
        let alice = alice.join().unwrap();
        assert_eq!(alice["conversations"][0]["association_status"], "verified");
        assert_eq!(
            alice["conversations"][0]["card"],
            first["conversations"][0]["card"]
        );
    });
}

#[test]
fn cardlinks_same_principal_overlap_reconciles_only_live_committed_binding() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    let observer = kit.observer(&peer, 1);
    let first = observer.conversation_list("alice");
    let row = &first["conversations"][0];
    let id = row["id"].as_str().unwrap();
    let agent = row["card"]["fleet_agent_id"].as_str().unwrap();
    let binding = row["card"]["binding_id"].as_str().unwrap();
    let pause = pause_enrichment(&peer);
    std::thread::scope(|scope| {
        let older = scope.spawn(|| observer.conversation_list("alice"));
        wait_for_pause(&pause);
        let newer = observer.conversation_list("alice");
        assert_eq!(newer["conversations"][0]["card"], row["card"]);
        assert!(
            observer
                .linked_conversation("alice", agent, id, binding)
                .is_ok()
        );
        resume(&pause);
        let older = older.join().unwrap();
        assert_eq!(older["conversations"][0]["association_status"], "verified");
        assert_eq!(older["conversations"][0]["card"], row["card"]);
    });
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_ok()
    );

    let pause = pause_enrichment(&peer);
    std::thread::scope(|scope| {
        let older = scope.spawn(|| observer.conversation_list("alice"));
        wait_for_pause(&pause);
        kit.state.lock().unwrap()["runtime"] = json!(OTHER);
        assert_eq!(
            observer.conversation_list("alice")["conversations"][0]["association_status"],
            "unavailable"
        );
        // Restore the sample's identity so its own I/O succeeds. It still must
        // not restore the binding removed by the completed newer acquisition.
        kit.state.lock().unwrap()["runtime"] = json!(RUNTIME);
        resume(&pause);
        let older = older.join().unwrap();
        assert_eq!(
            older["conversations"][0]["association_status"],
            "unavailable"
        );
        assert!(older["conversations"][0]["card"].is_null());
    });
    let before = peer.state.lock().unwrap().requests.len();
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_err()
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
    assert_ne!(
        observer.conversation_list("alice")["conversations"][0]["card"]["binding_id"],
        binding
    );
}

#[test]
fn cardlinks_concurrent_lease_countdown_preserves_identity_not_expiration() {
    let peer = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 4000;
    let observer = kit.observer(&peer, 1);
    let first = observer.conversation_list("alice");
    let row = &first["conversations"][0];
    let id = row["id"].as_str().unwrap();
    let pause = pause_enrichment(&peer);
    std::thread::scope(|scope| {
        let refresh = scope.spawn(|| observer.conversation_list("alice"));
        wait_for_pause(&pause);
        peer.state.lock().unwrap().grant.as_mut().unwrap().lease_ms = 3500;
        observer.conversations.list("alice");
        assert_eq!(
            observer
                .conversations
                .selected("alice", id)
                .unwrap()
                .grant
                .lease_ms,
            3500
        );
        resume(&pause);
        let refreshed = refresh.join().unwrap();
        assert_eq!(
            refreshed["conversations"][0]["association_status"],
            "verified"
        );
        assert_eq!(refreshed["conversations"][0]["card"], row["card"]);
    });
    let agent = row["card"]["fleet_agent_id"].as_str().unwrap();
    let binding = row["card"]["binding_id"].as_str().unwrap();
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_ok()
    );
    for cached in &mut observer.conversations.state.lock().unwrap().refs {
        cached.selected.expires = Instant::now();
    }
    let before = peer.state.lock().unwrap().requests.len();
    assert!(
        observer
            .linked_conversation("alice", agent, id, binding)
            .is_err()
    );
    assert_eq!(peer.state.lock().unwrap().requests.len(), before);
}

#[test]
fn cardlinks_omitted_competitor_blocks_join_without_exposing_hidden_handles() {
    let peer = Peer::new();
    let other = Peer::new();
    let kit = Kit::new(&peer);
    peer.state.lock().unwrap().association = Some(kit.association());
    other.state.lock().unwrap().association = Some(kit.association());
    let mut observer = kit.observer(&peer, 1);
    let mut config = peer.config();
    config.conversations = (0..63)
        .map(|n| config::ConversationConfig {
            id: format!("registered{n}"),
            label: "Static".into(),
            team: "research".into(),
            allowed_logins: vec!["alice".into()],
            native_socket: peer.socket.to_str().unwrap().into(),
            expected_runtime: RUNTIME.into(),
            session: format!("ses_static{n}"),
        })
        .collect();
    let mut second = config.conversation_sources[0].clone();
    second.id = "second".into();
    second.directory = other.registry.to_str().unwrap().into();
    config.conversation_sources.push(second);
    config.validate().unwrap();
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let list = observer.conversation_list("alice");
    assert_eq!(list["conversations"].as_array().unwrap().len(), 64);
    assert_eq!(list["omitted_conversations"], 1);
    assert!(
        list["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["association_status"] == "unavailable" && row["card"].is_null())
    );
    assert!(kit.requests.lock().unwrap().is_empty());
    // Exactly the two discovery RPCs per authorized source, no metadata expansion.
    assert_eq!(peer.state.lock().unwrap().requests.len(), 2);
    assert_eq!(other.state.lock().unwrap().requests.len(), 2);
    let hidden = observer
        .conversations
        .state
        .lock()
        .unwrap()
        .refs
        .iter()
        .find(|r| r.source == 1)
        .unwrap()
        .id
        .clone();
    assert!(observer.conversations.selected("alice", &hidden).is_err());
    assert!(observer.conversations.read("alice", &hidden).is_err());
    assert_eq!(other.state.lock().unwrap().requests.len(), 2);

    config.conversation_sources[1].allowed_logins = vec!["bob".into()];
    config.conversation_sources[1].submit_logins = vec!["bob".into()];
    config.validate().unwrap();
    observer.conversations = Conversations::new(&config, "epoch").unwrap();
    let list = observer.conversation_list("alice");
    assert_eq!(list["conversations"].as_array().unwrap().len(), 64);
    assert_eq!(list["omitted_conversations"], 0);
    assert_eq!(list["conversations"][63]["association_status"], "verified");
    assert_eq!(other.state.lock().unwrap().requests.len(), 2);
    assert_eq!(peer.state.lock().unwrap().requests.len(), 12);
    assert_eq!(kit.requests.lock().unwrap().len(), 4);
}
