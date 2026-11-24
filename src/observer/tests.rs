use super::*;
use model::Agent;

fn config() -> Config {
    serde_json::from_value(json!({"listen":"127.0.0.1:7341","public_origin":"https://host.example.ts.net",
        "allowed_logins":["owner@example.invalid"],"endpoints":[{"id":"one","label":"One","socket":"/tmp/fixture.sock"}]})).unwrap()
}
fn inventory(terminal: &str) -> Inventory {
    Inventory {
        generation: "a".repeat(64),
        discovery_status: "unsupported".into(),
        unclassified_panes: 0,
        agents: vec![Agent {
            terminal_id: terminal.into(),
            pane_id: "pane-1".into(),
            workspace_id: "ws-1".into(),
            tab_id: "tab-1".into(),
            title: "Synthetic".into(),
            pane_name: None,
            provider: Some("djinn".into()),
            project: None,
            activity: "working".into(),
            evidence: "kitsune_snapshot".into(),
            provider_candidates: vec!["djinn".into()],
        }],
    }
}
fn endpoint(id: &str) -> Arc<Endpoint> {
    let mut config = config().endpoints.remove(0);
    config.id = id.into();
    Arc::new(Endpoint {
        config,
        cache: Mutex::new(Cache::default()),
        busy: AtomicBool::new(false),
        refresh_due: AtomicBool::new(false),
    })
}
struct FixtureTransport {
    reply: Mutex<Result<Value>>,
    calls: std::sync::atomic::AtomicUsize,
}
impl Transport for FixtureTransport {
    fn call(&self, _: &EndpointConfig, _: &Request, _: Instant) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.reply.lock().unwrap().clone()
    }
}
fn observer(endpoints: Vec<Arc<Endpoint>>, reply: Result<Value>) -> Observer {
    Observer {
        events: Arc::default(),
        attention: Mutex::new(attention::State::default()),
        cardlinks: Mutex::new(cardlinks::State::default()),
        conversations: conversations::Conversations::default(),
        endpoints,
        allow_output: true,
        transport: Arc::new(FixtureTransport {
            reply: Mutex::new(reply),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }),
        shutdown: Arc::new((Mutex::new(false), Condvar::new())),
        pollers: Vec::new(),
    }
}

#[test]
fn accepted_inventory_inventory_failure_and_output_failure_immediately_signal_fleet() {
    use events::{Events, StreamingBody};
    use http_body_util::BodyExt;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let events = Arc::new(Events::default());
        let e = endpoint("one");
        e.cache.lock().unwrap().events = Some(events.clone());
        let o = observer(vec![e.clone()], Err(Error("timeout")));
        let mut body = StreamingBody::Sse(events.admit("owner").unwrap());
        body.frame().await.unwrap().unwrap();
        e.accept("epoch", inventory("terminal"));
        assert!(e.cache.try_lock().is_ok());
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(
            std::str::from_utf8(&frame)
                .unwrap()
                .starts_with("event: fleet\n")
        );
        let id = e.cache.lock().unwrap().ids[0].clone();
        assert_eq!(o.output(&id).unwrap_err().code(), "timeout");
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(
            std::str::from_utf8(&frame)
                .unwrap()
                .starts_with("event: fleet\n")
        );
        assert!(e.cache.try_lock().is_ok());
        e.fail("private upstream text");
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(!std::str::from_utf8(&frame).unwrap().contains("private"));
        assert_eq!(e.cache.lock().unwrap().error, Some("upstream_unavailable"));
    });
}

#[test]
fn helper_titles_are_whole_bounded_strings_without_widening_other_fields() {
    let old = serde_json::to_value(inventory("terminal")).unwrap();
    for title in [
        "x".repeat(40),
        "x".repeat(41),
        "x".repeat(512),
        "x".repeat(513),
        "x".repeat(4096),
        "\u{1f600}".repeat(1024),
        "literal...".into(),
    ] {
        let mut value = old.clone();
        value["agents"][0]["title"] = json!(title);
        let decoded = transport::decode_helper(
            &serde_json::to_vec(&json!({"result":value})).unwrap(),
            &Request::Inventory {},
        )
        .unwrap();
        assert_eq!(parse_inventory(decoded).unwrap().agents[0].title, title);
    }
    for title in [
        json!("x".repeat(4097)),
        json!(format!("{}x", "\u{1f600}".repeat(1024))),
        json!("bad\ntext"),
        json!(42),
    ] {
        let mut value = old.clone();
        value["agents"][0]["title"] = title;
        assert!(parse_inventory(value).is_err());
    }
    for field in ["provider", "project"] {
        let mut value = old.clone();
        value["agents"][0][field] = json!("x".repeat(513));
        assert!(parse_inventory(value).is_err());
    }
}

#[test]
fn helper_pane_name_is_optional_safe_and_other_fields_remain_closed() {
    let old = serde_json::to_value(inventory("terminal")).unwrap();
    assert!(old["agents"][0].get("pane_name").is_none());
    assert_eq!(
        parse_inventory(old.clone()).unwrap().agents[0].pane_name,
        None
    );
    for name in [
        Value::Null,
        json!("Label15"),
        json!("<b>literal</b>"),
        json!(" x "),
        json!("\u{00e9}".repeat(80)),
    ] {
        let mut value = old.clone();
        value["agents"][0]["pane_name"] = name.clone();
        let parsed = parse_inventory(value).unwrap();
        assert_eq!(parsed.agents[0].pane_name.as_deref(), name.as_str());
    }
    let mut invalid = vec![
        json!(true),
        json!(42),
        json!({}),
        json!([]),
        json!(""),
        json!("  "),
        json!("x".repeat(161)),
        json!("\u{00e9}".repeat(81)),
    ];
    for c in [
        '\0', '\n', '\r', '\t', '\u{001b}', '\u{007f}', '\u{0085}', '\u{061c}', '\u{200e}',
        '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}',
        '\u{2067}', '\u{2068}', '\u{2069}', '\u{2028}', '\u{2029}',
    ] {
        invalid.push(json!(format!("left{c}right")));
    }
    for name in invalid {
        let mut value = old.clone();
        value["agents"][0]["pane_name"] = name;
        assert_eq!(parse_inventory(value).unwrap_err().code(), "schema_error");
    }
    let mut extra = old.clone();
    extra["agents"][0]["manual_label"] = json!("unsupported");
    assert!(parse_inventory(extra).is_err());
    let mut extra = old;
    extra["unexpected"] = json!(true);
    assert!(parse_inventory(extra).is_err());
}

#[test]
fn presentation_rename_and_clear_preserve_terminal_identity_and_fleet_ids() {
    let e = endpoint("a");
    let original = inventory("terminal");
    e.accept("epoch", original.clone());
    let ids = terminal_references(&e);
    let o = observer(vec![e.clone()], Err(Error("unused")));
    for name in [Some("Label15"), Some("Renamed"), None] {
        let mut changed = original.clone();
        changed.agents[0].pane_name = name.map(str::to_owned);
        changed.agents[0].title = format!("{}...", "T".repeat(4093));
        changed.agents[0].provider = Some("renamed-provider".into());
        assert!(original.agents[0].same_terminal(&changed.agents[0]));
        e.accept("epoch", changed);
        assert_eq!(terminal_references(&e), ids);
        let fleet = o.fleet();
        let card = &fleet["endpoints"][0]["agents"][0];
        assert_eq!(card["pane_name"], json!(name));
        assert_eq!(card["title"], format!("{}...", "T".repeat(4093)));
    }
}

#[test]
fn config_is_closed_and_has_safe_defaults() {
    let config = config();
    config.validate().unwrap();
    assert!(!config.allow_best_effort_output);
    assert!(config.conversations.is_empty());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        br#"{"listen":"127.0.0.1:1","listen":"127.0.0.1:2"}"#,
    )
    .unwrap();
    assert_eq!(
        Config::load(file.path()).unwrap_err().code(),
        "invalid_config"
    );
    for v in [
        json!({"listen":"127.0.0.1:1","public_origin":"https://h","allowed_logins":["a"],"endpoints":[],"extra":true}),
        json!({"listen":"127.0.0.1:1","public_origin":"https://h","allowed_logins":["a"],"endpoints":[{"id":"a","label":"A","socket":"/a","command":"evil"}]}),
    ] {
        assert!(serde_json::from_value::<Config>(v).is_err());
    }
}

#[test]
fn config_rejects_network_listeners_url_rewriting_paths_and_duplicate_ids() {
    let mut c = config();
    for listen in ["0.0.0.0:7341", "192.168.0.1:7341", "[::]:7341"] {
        c.listen = listen.parse().unwrap();
        assert_eq!(c.validate().unwrap_err().code(), "invalid_listen");
    }
    c = config();
    for origin in [
        "http://host",
        "https://a@host",
        "https://host/",
        "https://host?x",
        "https://host#x",
        "https://host:0",
        "https://host:+443",
        "https://host:443/path",
        "https://host\n",
    ] {
        c.public_origin = origin.into();
        assert_eq!(
            c.validate().unwrap_err().code(),
            "invalid_origin",
            "{origin}"
        );
    }
    c = config();
    for socket in ["relative", "/tmp/../socket", "/tmp/socket\0", "/tmp/"] {
        c.endpoints[0].socket = socket.into();
        assert!(c.validate().is_err());
    }
    c = config();
    c.endpoints.push(c.endpoints[0].clone());
    assert_eq!(c.validate().unwrap_err().code(), "invalid_endpoint_id");
    c = config();
    c.endpoints = (0..9)
        .map(|n| {
            let mut e = c.endpoints[0].clone();
            e.id = n.to_string();
            e
        })
        .collect();
    assert_eq!(c.validate().unwrap_err().code(), "endpoint_limit");
}

#[test]
fn ssh_configuration_cannot_inject_options_shell_or_destinations() {
    let mut c = config();
    c.endpoints[0].ssh = Some(config::SshConfig {
        target: "laptop-alias".into(),
        helper: "/home/example/bin/coven".into(),
    });
    c.validate().unwrap();
    for target in [
        "-oProxyCommand=bad",
        "user@host",
        "host:22",
        "host;true",
        "host\n",
        "host/thing",
        "host..alias",
        "",
    ] {
        c.endpoints[0].ssh.as_mut().unwrap().target = target.into();
        assert_eq!(c.validate().unwrap_err().code(), "invalid_ssh_config");
    }
    c.endpoints[0].ssh.as_mut().unwrap().target = "laptop".into();
    for helper in [
        "coven",
        "/a/../coven",
        "/a/coven;id",
        "/a/$(id)",
        "/a/coven with-space",
        "/a/coven'",
    ] {
        c.endpoints[0].ssh.as_mut().unwrap().helper = helper.into();
        assert!(c.validate().is_err());
    }
}

#[test]
fn endpoint_scoped_ids_replacement_and_failure_invalidate_output() {
    let a = endpoint("a");
    let b = endpoint("b");
    a.accept("epoch", inventory("same-terminal"));
    b.accept("epoch", inventory("same-terminal"));
    let old = a.cache.lock().unwrap().ids[0].clone();
    assert!(valid_id(&old));
    assert_ne!(old, b.cache.lock().unwrap().ids[0]);
    a.accept("epoch", inventory("same-terminal"));
    assert_eq!(old, a.cache.lock().unwrap().ids[0]);
    a.accept("epoch", inventory("replacement"));
    assert_ne!(old, a.cache.lock().unwrap().ids[0]);
    let o = observer(vec![a.clone(), b], Err(Error("timeout")));
    assert_eq!(o.output(&old).unwrap_err().code(), "invalid_reference");
    let current = a.cache.lock().unwrap().ids[0].clone();
    assert_eq!(o.output(&current).unwrap_err().code(), "timeout");
    assert_eq!(o.output(&current).unwrap_err().code(), "stale_reference");
    a.accept("epoch", inventory("replacement"));
    assert_ne!(current, a.cache.lock().unwrap().ids[0]);
}

fn terminal_references(endpoint: &Endpoint) -> std::collections::BTreeMap<String, String> {
    let cache = endpoint.cache.lock().unwrap();
    cache
        .inventory
        .iter()
        .flat_map(|i| &i.agents)
        .zip(&cache.ids)
        .map(|(agent, id)| (agent.terminal_id.clone(), id.clone()))
        .collect()
}

fn multiple_inventory(terminals: &[&str]) -> Inventory {
    Inventory {
        generation: "a".repeat(64),
        discovery_status: "unsupported".into(),
        unclassified_panes: 0,
        agents: terminals
            .iter()
            .map(|terminal| {
                let mut agent = inventory(terminal).agents.remove(0);
                agent.pane_id = format!("pane-{terminal}");
                agent
            })
            .collect(),
    }
}

#[test]
fn terminal_references_survive_reorder_other_births_removals_and_layout_moves() {
    let e = endpoint("a");
    e.accept("epoch", multiple_inventory(&["one", "two"]));
    let original = terminal_references(&e);
    e.accept("epoch", multiple_inventory(&["two", "one"]));
    assert_eq!(terminal_references(&e), original);
    e.accept("epoch", multiple_inventory(&["three", "one", "two"]));
    let added = terminal_references(&e);
    assert_eq!(added["one"], original["one"]);
    assert_eq!(added["two"], original["two"]);
    assert!(!original.values().any(|id| *id == added["three"]));
    e.accept("epoch", multiple_inventory(&["three", "one"]));
    let removed = terminal_references(&e);
    assert_eq!(removed["one"], original["one"]);
    assert_eq!(removed["three"], added["three"]);
    assert!(!removed.contains_key("two"));
    let mut moved = multiple_inventory(&["one", "three"]);
    moved.agents[0].pane_id = "moved-pane".into();
    moved.agents[0].tab_id = "moved-tab".into();
    moved.agents[0].workspace_id = "moved-workspace".into();
    e.accept("epoch", moved);
    assert_eq!(terminal_references(&e), removed);
    e.accept("epoch", multiple_inventory(&["two", "one", "three"]));
    let reappeared = terminal_references(&e);
    assert_ne!(reappeared["two"], original["two"]);
    assert_eq!(reappeared["one"], original["one"]);
    assert_eq!(reappeared["three"], added["three"]);
}

#[test]
fn source_generation_failure_and_expired_lease_invalidate_every_reference() {
    for reason in ["source_generation", "error", "expiry"] {
        let e = endpoint("a");
        e.accept("epoch", multiple_inventory(&["one", "two"]));
        let old = terminal_references(&e);
        let o = observer(vec![e.clone()], Err(Error("must_not_query")));
        let mut next = multiple_inventory(&["two", "one"]);
        match reason {
            "source_generation" => next.generation = "b".repeat(64),
            "error" => e.fail("timeout"),
            "expiry" => e.cache.lock().unwrap().fresh = Some(Instant::now() - STALE),
            _ => unreachable!(),
        }
        if reason != "source_generation" {
            for id in old.values() {
                assert_eq!(o.output(id).unwrap_err().code(), "stale_reference");
            }
        }
        e.accept("epoch", next);
        let current = terminal_references(&e);
        for (terminal, id) in old {
            assert_ne!(current[&terminal], id, "{reason}");
            assert_eq!(o.output(&id).unwrap_err().code(), "invalid_reference");
        }
    }
}

#[test]
fn stale_retains_agents_but_successful_empty_clears_them() {
    let e = endpoint("a");
    e.accept("epoch", inventory("t"));
    let o = observer(vec![e.clone()], Err(Error("timeout")));
    e.fail("timeout");
    let fleet = o.fleet();
    assert_eq!(fleet["endpoints"][0]["connection"], "unavailable");
    assert_eq!(fleet["endpoints"][0]["agents"].as_array().unwrap().len(), 1);
    assert_eq!(
        fleet["endpoints"][0]["agents"][0]["output_available"],
        false
    );
    e.accept(
        "epoch",
        Inventory {
            generation: "b".repeat(64),
            agents: Vec::new(),
            discovery_status: "unsupported".into(),
            unclassified_panes: 0,
        },
    );
    let fleet = o.fleet();
    assert_eq!(fleet["endpoints"][0]["connection"], "reachable");
    assert_eq!(fleet["endpoints"][0]["agents"], json!([]));
}

#[test]
fn fleet_dto_uses_browser_types_before_and_after_observation() {
    let e = endpoint("a");
    let o = observer(vec![e.clone()], Err(Error("unused")));
    let initial = o.fleet();
    assert_eq!(initial["endpoints"][0]["last_attempt_at_ms"], 0);
    assert!(initial["endpoints"][0]["observed_at_ms"].is_null());
    assert_eq!(initial["endpoints"][0]["discovery_status"], "unavailable");
    assert!(initial["endpoints"][0]["unclassified_panes"].is_null());
    let mut i = inventory("t");
    i.agents[0].provider = None;
    e.accept("epoch", i);
    let fleet = o.fleet();
    let endpoint = &fleet["endpoints"][0];
    let agent = &endpoint["agents"][0];
    assert!(endpoint["last_attempt_at_ms"].is_u64());
    assert!(endpoint["observed_at_ms"].is_u64());
    assert!(agent["observed_at_ms"].is_u64());
    assert_eq!(agent["observed_at_ms"], endpoint["observed_at_ms"]);
    assert_eq!(agent["provider"], "");
    assert_eq!(agent["project"], "");
}

#[test]
fn due_inventory_has_priority_over_repeated_output_and_retries_within_lease() {
    use std::sync::{Barrier, atomic::AtomicUsize};
    struct SlowOutput {
        entered: Barrier,
        release: Barrier,
        active: AtomicUsize,
        maximum: AtomicUsize,
        outputs: AtomicUsize,
        inventories: AtomicUsize,
    }
    impl Transport for SlowOutput {
        fn call(&self, _: &EndpointConfig, request: &Request, _: Instant) -> Result<Value> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let reply = match request {
                Request::Inventory {} => {
                    self.inventories.fetch_add(1, Ordering::SeqCst);
                    serde_json::to_value(inventory("t")).unwrap()
                }
                Request::Output { .. } => {
                    if self.outputs.fetch_add(1, Ordering::SeqCst) == 0 {
                        self.entered.wait();
                        self.release.wait();
                    }
                    json!({"generation":"a".repeat(64),"terminal_id":"t","text":"bounded","truncated":false})
                }
            };
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(reply)
        }
    }
    let e = endpoint("a");
    e.accept("epoch", inventory("t"));
    e.cache.lock().unwrap().fresh = Some(Instant::now() - (INTERVAL - Duration::from_millis(100)));
    let id = e.cache.lock().unwrap().ids[0].clone();
    let transport = Arc::new(SlowOutput {
        entered: Barrier::new(2),
        release: Barrier::new(2),
        active: AtomicUsize::new(0),
        maximum: AtomicUsize::new(0),
        outputs: AtomicUsize::new(0),
        inventories: AtomicUsize::new(0),
    });
    let mut o = observer(vec![e.clone()], Err(Error("unused")));
    o.transport = transport.clone();
    let o = Arc::new(o);
    let worker_o = o.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || worker_o.output(&worker_id));
    transport.entered.wait();
    e.cache.lock().unwrap().fresh = Some(Instant::now() - INTERVAL);
    let observed = e.cache.lock().unwrap().observed;
    let mut failures = 0;
    let mut attempts = Vec::new();
    for _ in 0..3 {
        let delay = e.poll_once("epoch", transport.as_ref(), &mut failures);
        attempts.push((delay, o.output(&id).unwrap_err().code()));
        std::thread::sleep(delay);
    }
    transport.release.wait();
    worker.join().unwrap().unwrap();
    assert!(
        attempts
            .iter()
            .all(|(delay, code)| *delay == BUSY_RETRY && *code == "endpoint_busy")
    );
    assert_eq!(failures, 0);
    assert_eq!(e.cache.lock().unwrap().observed, observed);
    assert_eq!(o.fleet()["endpoints"][0]["connection"], "reachable");
    assert_eq!(o.output(&id).unwrap_err().code(), "endpoint_busy");
    assert!(e.cache.lock().unwrap().fresh.unwrap().elapsed() < STALE);
    assert_eq!(
        e.poll_once("epoch", transport.as_ref(), &mut failures),
        INTERVAL
    );
    assert!(e.cache.lock().unwrap().fresh.unwrap().elapsed() < Duration::from_secs(1));
    o.output(&id).unwrap();
    assert_eq!(transport.inventories.load(Ordering::SeqCst), 1);
    assert_eq!(transport.outputs.load(Ordering::SeqCst), 2);
    assert_eq!(transport.maximum.load(Ordering::SeqCst), 1);
}

#[test]
fn helper_envelopes_are_closed_bounded_and_never_forward_diagnostics() {
    let request = Request::Inventory {};
    let result = serde_json::to_value(inventory("t")).unwrap();
    assert_eq!(
        transport::decode_helper(
            &serde_json::to_vec(&json!({"result":result})).unwrap(),
            &request
        )
        .unwrap(),
        result
    );
    for code in [
        "unsupported_protocol",
        "timeout",
        "response_limit",
        "terminal_replaced",
        "schema_error",
        "upstream_unavailable",
    ] {
        let payload = serde_json::to_vec(&json!({"error":{"code":code}})).unwrap();
        assert_eq!(
            transport::decode_helper(&payload, &request)
                .unwrap_err()
                .code(),
            code
        );
    }
    for payload in [
        br#"{"error":{"code":"PRIVATE /secret"}}"#.as_slice(),
        br#"{"error":{"code":"unsupported_protocol","message":"PRIVATE"}}"#,
        br#"{"error":{"code":"timeout","code":"PRIVATE"}}"#,
        br#"{"error":{"code":"timeout"},"extra":"PRIVATE"}"#,
        br#"{"error":{"code":"timeout"},"result":{}}"#,
        br#"{"error":{"code":"timeout"},"error":{"code":"timeout"}}"#,
    ] {
        let error = transport::decode_helper(payload, &request).unwrap_err();
        assert_eq!(error.code(), "schema_error");
        assert!(!error.to_string().contains("PRIVATE"));
    }
    assert_eq!(
        transport::decode_helper(&vec![b' '; transport::HELPER_LIMIT + 1], &request)
            .unwrap_err()
            .code(),
        "response_limit"
    );
    assert_eq!(
        probe_response("/unused", br#"{"operation":"bad","secret":"PRIVATE"}"#),
        json!({"error":{"code":"invalid_probe_request"}})
    );
}

#[test]
fn output_is_opt_in_fresh_and_has_no_queue() {
    let e = endpoint("a");
    e.accept("epoch", inventory("t"));
    let id = e.cache.lock().unwrap().ids[0].clone();
    let mut o = observer(
        vec![e.clone()],
        Ok(
            json!({"generation":"a".repeat(64),"terminal_id":"t","text":"literal <script>","truncated":false}),
        ),
    );
    o.allow_output = false;
    assert_eq!(o.output(&id).unwrap_err().code(), "output_disabled");
    o.allow_output = true;
    let busy = e.acquire().unwrap();
    assert_eq!(o.output(&id).unwrap_err().code(), "endpoint_busy");
    assert_eq!(
        e.refresh("epoch", o.transport.as_ref()).unwrap_err().code(),
        "endpoint_busy"
    );
    assert_eq!(o.fleet()["endpoints"][0]["agents"][0]["id"], id);
    drop(busy);
    assert_eq!(o.output(&id).unwrap_err().code(), "endpoint_busy");
    let inventory_transport = FixtureTransport {
        reply: Mutex::new(Ok(serde_json::to_value(inventory("t")).unwrap())),
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    e.refresh("epoch", &inventory_transport).unwrap();
    let output = o.output(&id).unwrap();
    assert_eq!(output["text"], "literal <script>");
    assert_eq!(
        output["attribution"],
        "matching_snapshot_brackets_best_effort"
    );
    e.cache.lock().unwrap().fresh = Some(Instant::now() - STALE);
    assert_eq!(o.output(&id).unwrap_err().code(), "stale_reference");
    e.accept("epoch", inventory("t"));
    assert_ne!(id, e.cache.lock().unwrap().ids[0]);
    assert_eq!(
        o.output("../../sock").unwrap_err().code(),
        "invalid_reference"
    );
}

#[test]
fn output_wrong_generation_discards_text_and_invalidates_lease() {
    let e = endpoint("a");
    e.accept("epoch", inventory("t"));
    let id = e.cache.lock().unwrap().ids[0].clone();
    let o = observer(
        vec![e.clone()],
        Ok(
            json!({"generation":"b".repeat(64),"terminal_id":"t","text":"PRIVATE","truncated":false}),
        ),
    );
    assert_eq!(o.output(&id).unwrap_err().code(), "terminal_replaced");
    assert!(e.cache.lock().unwrap().fresh.is_none());
    assert!(!o.fleet().to_string().contains("PRIVATE"));
    assert_eq!(o.output(&id).unwrap_err().code(), "stale_reference");
    e.accept("epoch", inventory("t"));
    assert_ne!(e.cache.lock().unwrap().ids[0], id);
    assert_eq!(o.output(&id).unwrap_err().code(), "invalid_reference");
}

#[test]
fn utf8_truncation_never_splits_a_codepoint() {
    let mut text = format!("{}é", "a".repeat(model::OUTPUT_LIMIT - 1));
    assert!(model::truncate(&mut text));
    assert_eq!(text.len(), model::OUTPUT_LIMIT - 1);
    assert!(!model::truncate(&mut text));
}

#[test]
fn helper_projection_rejects_oversized_duplicates_and_unknown_fields() {
    let mut i = inventory("t");
    i.agents.push(i.agents[0].clone());
    assert!(parse_inventory(serde_json::to_value(i).unwrap()).is_err());
    let mut i = inventory("t");
    i.agents[0].title = "x".repeat(4097);
    assert!(parse_inventory(serde_json::to_value(i).unwrap()).is_err());
    let mut value = serde_json::to_value(inventory("t")).unwrap();
    value["command"] = json!("bad");
    assert!(parse_inventory(value).is_err());
    assert_eq!(
        probe("/unused", br#"{"operation":"output","pane_id":"p"}"#)
            .unwrap_err()
            .code(),
        "invalid_probe_request"
    );
    assert_eq!(
        probe(
            "/unused",
            br#"{"operation":"inventory","operation":"inventory"}"#
        )
        .unwrap_err()
        .code(),
        "invalid_probe_request"
    );
}

#[test]
fn remote_discovery_fields_are_closed_bounded_and_cannot_grant_activity_or_output() {
    let mut inventory = inventory("t");
    inventory.discovery_status = "observed".into();
    inventory.agents[0].evidence = "coven_process_discovery".into();
    inventory.agents[0].activity = "unknown".into();
    let result = serde_json::to_value(inventory).unwrap();
    let wire = serde_json::to_vec(&json!({"result":result})).unwrap();
    assert_eq!(
        transport::decode_helper(&wire, &Request::Inventory {}).unwrap(),
        result
    );
    for (path, value) in [
        ("evidence", json!("native_authority")),
        ("activity", json!("working")),
        ("provider_candidates", json!([])),
        ("provider_candidates", json!(["/PRIVATE/bin/djinn"])),
        ("provider_candidates", json!(vec!["djinn"; 23])),
        ("provider_candidates", json!(["djinn", "djinn"])),
    ] {
        let mut malicious = result.clone();
        malicious["agents"][0][path] = value;
        assert_eq!(
            transport::decode_helper(
                &serde_json::to_vec(&json!({"result":malicious})).unwrap(),
                &Request::Inventory {}
            )
            .unwrap_err()
            .code(),
            "schema_error"
        );
    }
    for (key, value) in [
        ("discovery_status", json!("complete")),
        ("unclassified_panes", json!(1025)),
    ] {
        let mut malicious = result.clone();
        malicious[key] = value;
        assert!(
            transport::decode_helper(
                &serde_json::to_vec(&json!({"result":malicious})).unwrap(),
                &Request::Inventory {}
            )
            .is_err()
        );
    }
    let mut malicious = result;
    malicious["agents"][0]["argv"] = json!(["PRIVATE"]);
    assert_eq!(
        transport::decode_helper(
            &serde_json::to_vec(&json!({"result":malicious})).unwrap(),
            &Request::Inventory {}
        )
        .unwrap_err()
        .code(),
        "schema_error"
    );
}

#[test]
fn terminal_decoder_does_not_widen_ordinary_wire_strings() {
    let large = "x".repeat(8192);
    let wire =
        serde_json::to_vec(&json!({"id":"r","result":{"type":"pane_read","read":{"text":large}}}))
            .unwrap();
    assert!(crate::observation_wire::decode_native(&wire).is_err());
    assert!(crate::observation_wire::decode_terminal_output(&wire).is_ok());
    let wire = serde_json::to_vec(&json!({"id":"r","result":{"read":{"text":"ok","other":large}}}))
        .unwrap();
    assert!(crate::observation_wire::decode_terminal_output(&wire).is_err());
    let wire =
        serde_json::to_vec(&json!({"result":{"read":{"text":"x".repeat(256*1024+1)}}})).unwrap();
    assert!(crate::observation_wire::decode_terminal_output(&wire).is_err());
    assert!(
        crate::observation_wire::decode_terminal_output(
            br#"{"result":{"read":{"text":"a","text":"b"}}}"#
        )
        .is_err()
    );
}

#[test]
fn busy_endpoint_never_queues_a_second_remote_operation_or_blocks_fleet() {
    struct BlockingTransport {
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }
    impl Transport for BlockingTransport {
        fn call(&self, _: &EndpointConfig, _: &Request, _: Instant) -> Result<Value> {
            self.entered.wait();
            self.release.wait();
            Err(Error("timeout"))
        }
    }
    let e = endpoint("a");
    e.accept("epoch", inventory("t"));
    let id = e.cache.lock().unwrap().ids[0].clone();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let mut o = observer(vec![e], Err(Error("unused")));
    o.transport = Arc::new(BlockingTransport {
        entered: entered.clone(),
        release: release.clone(),
    });
    let o = Arc::new(o);
    let worker_o = o.clone();
    let worker_id = id.clone();
    let worker = std::thread::spawn(move || worker_o.output(&worker_id));
    entered.wait();
    let start = Instant::now();
    let error = o.output(&id).unwrap_err();
    let fleet = o.fleet();
    release.wait();
    worker.join().unwrap().unwrap_err();
    assert_eq!(error.code(), "endpoint_busy");
    assert_eq!(fleet["endpoints"][0]["agents"].as_array().unwrap().len(), 1);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };

    #[test]
    fn rejected_probe_fields_never_construct_or_connect_an_upstream_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("untouched.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let sockets = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        for input in [
            br#"{"operation":"inventory","method":"pane.send_keys"}"#.as_slice(),
            br#"{"operation":"inventory","terminal_id":"t"}"#,
            br#"{"operation":"inventory","operation":"inventory"}"#,
            br#"{"operation":"output","terminal_id":"t","method":"pane.send_keys"}"#,
            br#"{"operation":"output","terminal_id":"t","extra":true}"#,
        ] {
            assert_eq!(
                probe(path.to_str().unwrap(), input).unwrap_err().code(),
                "invalid_probe_request"
            );
            assert_eq!(
                probe_response(path.to_str().unwrap(), input),
                json!({"error":{"code":"invalid_probe_request"}})
            );
        }
        assert_eq!(sockets.count(), 0);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(
            serde_json::to_value(Request::Inventory {}).unwrap(),
            json!({"operation":"inventory"})
        );
    }

    #[test]
    fn helper_supervisor_closes_stdin_caps_both_pipes_and_kills_on_deadline() {
        use std::process::Command;
        let input = br#"{"operation":"inventory"}"#;
        let output = transport::run_helper(
            &mut Command::new("/bin/cat"),
            input,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(output, input);
        let error = transport::run_helper(
            Command::new("/bin/sleep").arg("10"),
            input,
            Instant::now() + Duration::from_millis(40),
        )
        .unwrap_err();
        assert_eq!(error.code(), "timeout");
        let error = transport::run_helper(
            &mut Command::new("/usr/bin/yes"),
            input,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(error.code(), "response_limit");
        let error = transport::run_helper(
            Command::new("/bin/sh").args(["-c", "exec /usr/bin/yes >&2"]),
            input,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(error.code(), "response_limit");
    }

    fn snapshot(terminal: &str, status: &str) -> Value {
        let mut a = json!({"terminal_id":terminal,"pane_id":"p","workspace_id":"w","tab_id":"tab","agent_status":status,"agent":"djinn","name":"Example"});
        a["native_secret"] = json!("must-not-project");
        json!({"type":"session_snapshot","snapshot":{"protocol":18,"version":"test","panes":[a.clone()],"agents":[a]}})
    }
    struct Server {
        _dir: tempfile::TempDir,
        path: String,
        worker: Option<JoinHandle<()>>,
    }
    impl Server {
        fn new(replies: Vec<(&'static str, Value)>, delay: Duration) -> Self {
            Self::timed(
                replies
                    .into_iter()
                    .map(|(method, result)| (method, result, delay))
                    .collect(),
            )
        }
        fn timed(replies: Vec<(&'static str, Value, Duration)>) -> Self {
            Self::configured(replies, false)
        }
        fn configured(
            replies: Vec<(&'static str, Value, Duration)>,
            replace_on_process: bool,
        ) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("fixture.sock");
            let listener = UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let replace_path = path.clone();
            let worker = std::thread::spawn(move || {
                let mut _replacement = None;
                for (method, result, delay) in replies {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((s, _)) => break s,
                            Err(e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    && Instant::now() < deadline =>
                            {
                                std::thread::sleep(Duration::from_millis(2))
                            }
                            _ => return,
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(request["method"], method);
                    if replace_on_process && method == "pane.process_info" {
                        std::fs::remove_file(&replace_path).unwrap();
                        _replacement = Some(UnixListener::bind(&replace_path).unwrap());
                        std::fs::set_permissions(
                            &replace_path,
                            std::fs::Permissions::from_mode(0o600),
                        )
                        .unwrap();
                    }
                    if method == "pane.read" {
                        assert_eq!(request["params"]["pane_id"], "p");
                        assert_eq!(request["params"]["source"], "recent_unwrapped");
                        assert_eq!(request["params"]["format"], "text");
                        assert_eq!(request["params"]["lines"], 256);
                    }
                    std::thread::sleep(delay);
                    let reply = if result.is_null() {
                        json!({"id":request["id"],"error":{"code":"failed","message":"PRIVATE"}})
                    } else {
                        json!({"id":request["id"],"result":result})
                    };
                    let mut bytes = serde_json::to_vec(&reply).unwrap();
                    bytes.push(b'\n');
                    let _ = stream.write_all(&bytes);
                }
            });
            Self {
                _dir: dir,
                path: path.to_str().unwrap().into(),
                worker: Some(worker),
            }
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }
    fn read_result(text: &str) -> Value {
        json!({"type":"pane_read","read":{"pane_id":"p","workspace_id":"w","tab_id":"tab","source":"recent_unwrapped","format":"text","text":text,"revision":0,"truncated":false}})
    }

    fn discovery_snapshot(native: &[usize], count: usize) -> Value {
        let panes: Vec<Value> = (0..count).map(|n| json!({
            "terminal_id":format!("t{n}"),"pane_id":format!("w1:p{}",n+1),"workspace_id":"w1","tab_id":"tab1",
            "agent_status":"working","agent":"djinn","name":format!("Project {n}"),"cwd":format!("/project-{n}")
        })).collect();
        let agents: Vec<_> = native.iter().map(|n| panes[*n].clone()).collect();
        json!({"type":"session_snapshot","snapshot":{"protocol":18,"version":"test",
            "process_observation":{"version":1,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"},
            "panes":panes,"agents":agents}})
    }

    fn discovery_replies(snapshot: &Value, processes: &[Vec<&str>]) -> Vec<(&'static str, Value)> {
        let mut process_replies = Vec::new();
        for (index, executables) in processes.iter().enumerate() {
            let rows: Vec<_> = executables.iter().enumerate().map(|(pid, executable)| json!({
                "pid":pid+1,"name":"PRIVATE_NAME","argv":[executable,"PRIVATE_ARGUMENT"],"cwd":"/PRIVATE_PROCESS_CWD"
            })).collect();
            process_replies.push((
                "pane.process_info",
                json!({"type":"pane_process_info","process_info":{
                    "pane_id":format!("w1:p{}",index+1),"foreground_processes":rows
                }}),
            ));
        }
        process_replies.sort_by(|(_, a), (_, b)| {
            a["process_info"]["pane_id"]
                .as_str()
                .cmp(&b["process_info"]["pane_id"].as_str())
        });
        let mut replies = vec![("session.snapshot", snapshot.clone())];
        replies.extend(process_replies);
        replies.push(("session.snapshot", snapshot.clone()));
        replies
    }

    #[test]
    fn whole_titles_for_native_and_process_agents_preserve_priority_and_metadata() {
        let mut cases: Vec<(Value, String)> = [40, 41, 512, 513, 4096]
            .into_iter()
            .map(|n| (json!("x".repeat(n)), "x".repeat(n)))
            .collect();
        for n in [10, 128, 129, 1024] {
            let text = "\u{1f600}".repeat(n);
            cases.push((json!(text), text));
        }
        cases.extend([
            (json!("literal..."), "literal...".into()),
            (
                json!(" \t left\n middle\r right\u{007f}  "),
                "left middle right".into(),
            ),
            (
                json!(format!("{}x", "\u{1f600}".repeat(1024))),
                "Title unavailable (too long)".into(),
            ),
            (
                json!("\u{00e9}".repeat(2049)),
                "Title unavailable (too long)".into(),
            ),
            (json!(42), "Title unavailable (invalid)".into()),
        ]);
        // Exercise every candidate, including the native agent's matched-pane fallbacks.
        for candidate in 0..6 {
            for (input, expected) in &cases {
                let mut snapshot = discovery_snapshot(&[0], 2);
                for pane in snapshot["snapshot"]["panes"].as_array_mut().unwrap() {
                    for key in ["name", "terminal_title", "title"] {
                        pane.as_object_mut().unwrap().remove(key);
                    }
                    pane["label"] = json!("Stable pane name");
                }
                for key in ["name", "terminal_title", "title"] {
                    snapshot["snapshot"]["agents"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove(key);
                }
                let key = ["name", "terminal_title", "title"][candidate % 3];
                if candidate < 3 {
                    snapshot["snapshot"]["agents"][0][key] = input.clone();
                } else {
                    snapshot["snapshot"]["panes"][0][key] = input.clone();
                }
                snapshot["snapshot"]["panes"][1][key] = input.clone();
                // Lower-priority text must not conceal an unavailable preferred title.
                if key != "title" {
                    snapshot["snapshot"]["panes"][0]["title"] = json!("lower priority");
                    snapshot["snapshot"]["panes"][1]["title"] = json!("lower priority");
                }
                let server = Server::new(
                    discovery_replies(&snapshot, &[vec!["codex"], vec!["djinn"]]),
                    Duration::ZERO,
                );
                let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
                let inventory = parse_inventory(value).unwrap();
                for agent in &inventory.agents {
                    assert_eq!(&agent.title, expected, "candidate {candidate}");
                    assert!(!agent.title.contains('\u{fffd}'));
                    assert_eq!(agent.pane_name.as_deref(), Some("Stable pane name"));
                    assert_eq!(agent.provider.as_deref(), Some("djinn"));
                    assert!(agent.project.is_some());
                }
            }
        }
    }

    #[test]
    fn titles_above_global_character_cap_remain_wire_errors() {
        // This guard precedes title projection, unlike multibyte byte-limit overflow.
        let mut snapshot = snapshot("t", "working");
        snapshot["snapshot"]["agents"][0]["name"] = json!("x".repeat(4097));
        let wire = serde_json::to_vec(&json!({"id":"r","result":snapshot})).unwrap();
        assert!(crate::observation_wire::decode_native(&wire).is_err());
    }

    #[test]
    fn matched_pane_label_is_the_only_name_source_for_both_ingress_paths() {
        let mut previous = None;
        // Each inventory is freshly projected, including after rename and clear.
        for label in [
            Some(json!("Label15")),
            Some(json!("Renamed")),
            None,
            Some(Value::Null),
            Some(json!(" ")),
            Some(json!(42)),
            Some(json!({})),
            Some(json!(true)),
            Some(json!([])),
            Some(json!("bad\nname")),
            Some(json!("bad\u{202e}name")),
            Some(json!("x".repeat(161))),
            Some(json!("\u{00e9}".repeat(81))),
            Some(json!("\u{00e9}".repeat(80))),
            Some(json!("<b>literal</b>")),
        ] {
            let mut snapshot = discovery_snapshot(&[0], 2);
            for pane in snapshot["snapshot"]["panes"].as_array_mut().unwrap() {
                pane["terminal_title"] = json!("OSC title is not a name");
                pane["title"] = json!("Tab title is not a name");
                pane["manual_label"] = json!("Not a snapshot field");
                if let Some(label) = &label {
                    pane["label"] = label.clone();
                }
            }
            snapshot["snapshot"]["agents"][0]["label"] = json!("Wrong source");
            let expected = label
                .as_ref()
                .and_then(Value::as_str)
                .filter(|s| crate::agent_management_presentation::safe_display_text(s, 160));
            let server = Server::new(
                discovery_replies(&snapshot, &[vec!["codex"], vec!["djinn"]]),
                Duration::ZERO,
            );
            let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
            for agent in value["agents"].as_array().unwrap() {
                assert_eq!(agent.get("pane_name").and_then(Value::as_str), expected);
                assert_eq!(agent.get("pane_name").is_some(), expected.is_some());
            }
            let inventory = parse_inventory(value).unwrap();
            assert_eq!(inventory.agents[0].title, "Project 0");
            assert_eq!(inventory.agents[1].title, "Project 1");
            assert_eq!(inventory.agents[0].evidence, "kitsune_snapshot");
            assert_eq!(inventory.agents[1].evidence, "coven_process_discovery");
            if let Some(old) = &previous {
                assert!(inventory.agents[0].same_terminal(old));
            }
            previous = Some(inventory.agents[0].clone());
        }
    }

    #[test]
    fn empty_native_catalog_discovers_two_process_agents_but_not_shells_or_editors() {
        let snapshot = discovery_snapshot(&[], 6);
        let server = Server::new(
            discovery_replies(
                &snapshot,
                &[
                    vec!["/private/bin/djinn"],
                    vec!["/private/bin/djinn-ui"],
                    vec!["zsh"],
                    vec!["nvim"],
                    vec!["bash"],
                    vec!["rustc"],
                ],
            ),
            Duration::ZERO,
        );
        let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        assert_eq!(value["agents"].as_array().unwrap().len(), 2);
        assert_eq!(value["discovery_status"], "observed");
        assert_eq!(value["unclassified_panes"], 4);
        let inventory = parse_inventory(value).unwrap();
        for agent in &inventory.agents {
            assert_eq!(agent.activity, "unknown");
            assert_eq!(agent.evidence, "coven_process_discovery");
            assert_eq!(agent.provider_candidates, vec!["djinn"]);
        }
        let e = endpoint("a");
        e.accept("epoch", inventory);
        let transport = Arc::new(FixtureTransport {
            reply: Mutex::new(Err(Error("must_not_read"))),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut o = observer(vec![e.clone()], Err(Error("unused")));
        o.transport = transport.clone();
        let fleet = o.fleet();
        let public = fleet.to_string();
        for secret in [
            "PRIVATE",
            "/private/bin",
            "argv",
            "pid",
            "scan_id",
            "observation_id",
        ] {
            assert!(!public.contains(secret), "{secret}");
        }
        let ids = e.cache.lock().unwrap().ids.clone();
        for id in ids {
            assert_eq!(o.output(&id).unwrap_err().code(), "output_unsupported");
        }
        assert_eq!(transport.calls.load(Ordering::Relaxed), 0);
        assert!(
            fleet["endpoints"][0]["agents"]
                .as_array()
                .unwrap()
                .iter()
                .all(|agent| agent["output_available"] == false)
        );
        assert_eq!(fleet["endpoints"][0]["discovery_status"], "observed");
        assert_eq!(fleet["endpoints"][0]["unclassified_panes"], 4);
    }

    #[test]
    fn native_metadata_wins_and_ambiguous_process_families_stay_one_unknown_card() {
        let snapshot = discovery_snapshot(&[0], 3);
        let server = Server::new(
            discovery_replies(
                &snapshot,
                &[vec!["codex"], vec!["djinn", "codex"], vec!["nvim"]],
            ),
            Duration::ZERO,
        );
        let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        let inventory = parse_inventory(value).unwrap();
        assert_eq!(inventory.agents.len(), 2);
        assert_eq!(inventory.unclassified_panes, 1);
        let native = &inventory.agents[0];
        assert_eq!(native.evidence, "kitsune_snapshot");
        assert_eq!(native.provider.as_deref(), Some("djinn"));
        assert_eq!(native.activity, "working");
        assert_eq!(native.title, "Project 0");
        let process = &inventory.agents[1];
        assert_eq!(process.evidence, "coven_process_discovery");
        assert_eq!(process.provider, None);
        assert_eq!(process.provider_candidates, vec!["codex", "djinn"]);
        assert_eq!(process.activity, "unknown");
        assert_eq!(process.project.as_deref(), Some("/project-1"));
        let e = endpoint("a");
        e.accept("epoch", inventory.clone());
        let ids = terminal_references(&e);
        let mut upgraded = inventory;
        upgraded.agents[1].evidence = "kitsune_snapshot".into();
        upgraded.agents[1].activity = "idle".into();
        e.accept("epoch", upgraded);
        assert_eq!(terminal_references(&e), ids);
    }

    #[test]
    fn unsupported_discovery_and_observed_unrecognized_panes_are_distinct() {
        let mut unsupported = discovery_snapshot(&[0], 2);
        unsupported["snapshot"]["process_observation"] = json!({"version":1,"platform":"windows","scope":"pane_foreground_cohort","selection":"unclassified"});
        let server = Server::new(vec![("session.snapshot", unsupported)], Duration::ZERO);
        let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        assert_eq!(value["discovery_status"], "unsupported");
        assert_eq!(value["unclassified_panes"], 1);
        assert_eq!(value["agents"].as_array().unwrap().len(), 1);
        let snapshot = discovery_snapshot(&[], 2);
        let server = Server::new(
            discovery_replies(&snapshot, &[vec!["bash"], vec!["nvim"]]),
            Duration::ZERO,
        );
        let value = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        assert_eq!(value["discovery_status"], "observed");
        assert_eq!(value["unclassified_panes"], 2);
        assert_eq!(value["agents"], json!([]));
    }

    #[test]
    fn malformed_recognized_descriptors_retain_two_stale_agents_and_cli_classification() {
        for descriptor in [
            json!({"version":"broken","platform":"linux"}),
            json!({"version":1,"platform":"linux","scope":false,"selection":"unclassified"}),
            json!({"version":1,"platform":"linux"}),
            json!({"version":2,"platform":"macos","scope":"pane_foreground_cohort","selection":"unclassified"}),
        ] {
            let snapshot = discovery_snapshot(&[], 2);
            let mut malformed = snapshot.clone();
            malformed["snapshot"]["process_observation"] = descriptor;
            let mut replies = discovery_replies(&snapshot, &[vec!["djinn"], vec!["djinn"]]);
            replies.extend([
                ("session.snapshot", malformed.clone()),
                ("session.snapshot", malformed),
            ]);
            let server = Server::new(replies, Duration::ZERO);
            let mut e = endpoint("a");
            Arc::get_mut(&mut e).unwrap().config.socket = server.path.clone();
            e.refresh("epoch", &NativeTransport).unwrap();
            let ids = terminal_references(&e);
            assert_eq!(ids.len(), 2);
            let observed = e.cache.lock().unwrap().observed;
            assert_eq!(
                e.refresh("epoch", &NativeTransport).unwrap_err().code(),
                "schema_error"
            );
            assert_eq!(terminal_references(&e), ids);
            let fleet = observer(vec![e], Err(Error("unused"))).fleet();
            assert_eq!(fleet["endpoints"][0]["connection"], "unavailable");
            assert_eq!(fleet["endpoints"][0]["discovery_status"], "unavailable");
            assert_eq!(fleet["endpoints"][0]["observed_at_ms"], json!(observed));
            assert_eq!(fleet["endpoints"][0]["agents"].as_array().unwrap().len(), 2);
            let report = crate::agent_discovery::inspect_cancel(
                &crate::agent_discovery::Request {
                    socket: server.path.clone(),
                    pane: None,
                    expected_terminal: None,
                    timeout: 5.0,
                    max_response_bytes: 4 * 1024 * 1024,
                },
                &crate::observation_socket::NoCancel,
            )
            .unwrap();
            assert_eq!(report["assessment"], "incompatible");
            assert_eq!(
                report["diagnostics"],
                json!(["process_observation_unsupported"])
            );
        }
    }

    #[test]
    fn absent_null_and_well_typed_unsupported_descriptors_remain_metadata_only() {
        for descriptor in [
            None,
            Some(Value::Null),
            Some(
                json!({"version":99,"platform":"linux","scope":"pane_foreground_cohort","selection":"unclassified"}),
            ),
            Some(
                json!({"version":1,"platform":"unknown","scope":"pane_foreground_cohort","selection":"unclassified"}),
            ),
            Some(
                json!({"version":1,"platform":"linux","scope":"unsupported_cohort","selection":"unclassified"}),
            ),
        ] {
            let mut snapshot = discovery_snapshot(&[0], 2);
            if let Some(descriptor) = descriptor {
                snapshot["snapshot"]["process_observation"] = descriptor;
            } else {
                snapshot["snapshot"]
                    .as_object_mut()
                    .unwrap()
                    .remove("process_observation");
            }
            let server = Server::new(vec![("session.snapshot", snapshot)], Duration::ZERO);
            let inventory = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
            assert_eq!(inventory["discovery_status"], "unsupported");
            assert_eq!(inventory["agents"].as_array().unwrap().len(), 1);
            assert_eq!(inventory["agents"][0]["evidence"], "kitsune_snapshot");
            assert_eq!(inventory["unclassified_panes"], 1);
        }
    }

    #[test]
    fn observer_scans_65_panes_with_two_agents_while_cli_keeps_64_pane_cap() {
        let snapshot = discovery_snapshot(&[0], 65);
        let mut processes = vec![vec!["bash"]; 65];
        processes[0] = vec!["djinn"];
        processes[64] = vec!["djinn"];
        let mut replies = discovery_replies(&snapshot, &processes);
        assert!(serde_json::to_vec(&replies).unwrap().len() < 4 * 1024 * 1024);
        replies.push(("session.snapshot", snapshot));
        let server = Server::new(replies, Duration::ZERO);
        let started = Instant::now();
        let inventory = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(inventory["discovery_status"], "observed");
        assert_eq!(inventory["unclassified_panes"], 63);
        let agents = inventory["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(
            agents
                .iter()
                .filter(|a| a["evidence"] == "kitsune_snapshot")
                .count(),
            1
        );
        assert_eq!(
            agents
                .iter()
                .filter(|a| a["evidence"] == "coven_process_discovery")
                .count(),
            1
        );
        let report = crate::agent_discovery::inspect_cancel(
            &crate::agent_discovery::Request {
                socket: server.path.clone(),
                pane: None,
                expected_terminal: None,
                timeout: 5.0,
                max_response_bytes: 4 * 1024 * 1024,
            },
            &crate::observation_socket::NoCancel,
        )
        .unwrap();
        assert_eq!(report["assessment"], "unavailable");
        assert_eq!(report["diagnostics"], json!(["pane_limit"]));
        assert_eq!(report["targets"], json!([]));
    }

    #[test]
    fn internal_acquisition_rejects_unbounded_pane_limits_before_socket_access() {
        let sockets = crate::platform::observation_socket::SocketConstructionGuard::new(true);
        for pane_limit in [0, 1025, usize::MAX] {
            let result = crate::agent_discovery::acquire(
                crate::agent_discovery::AcquisitionRequest {
                    socket: "/unused",
                    pane: None,
                    expected_terminal: None,
                    scan_id: "",
                    allow_unsupported: true,
                    pane_limit,
                },
                Instant::now() + Duration::from_secs(5),
                &crate::observation_socket::NoCancel,
                &mut crate::observation_socket::ReadBudget::new(4 * 1024 * 1024),
                &mut json!({"endpoint":{}}),
            );
            assert_eq!(result.err().unwrap().code, "invalid_pane_limit");
        }
        assert_eq!(sockets.count(), 0);
    }

    #[test]
    fn malformed_process_and_locator_drift_keep_last_known_agents_stale() {
        let snapshot = discovery_snapshot(&[], 1);
        let mut malformed = discovery_replies(&snapshot, &[vec!["djinn"]]);
        malformed[1].1["process_info"]["foreground_processes"] = json!("PRIVATE");
        malformed.pop();
        let mut drift = discovery_replies(&snapshot, &[vec!["djinn"]]);
        drift[2].1["snapshot"]["panes"][0]["tab_id"] = json!("moved");
        let mut generation = discovery_replies(&snapshot, &[vec!["djinn"]]);
        generation[2].1["snapshot"]["version"] = json!("new-server");
        for replies in [malformed, drift, generation] {
            let server = Server::new(replies, Duration::ZERO);
            let e = endpoint("a");
            let mut old = inventory("t0");
            old.discovery_status = "observed".into();
            old.agents[0].evidence = "coven_process_discovery".into();
            old.agents[0].activity = "unknown".into();
            e.accept("epoch", old);
            let ids = terminal_references(&e);
            let mut config = e.config.clone();
            config.socket = server.path.clone();
            struct SourceFixture(EndpointConfig);
            impl Transport for SourceFixture {
                fn call(
                    &self,
                    _: &EndpointConfig,
                    request: &Request,
                    deadline: Instant,
                ) -> Result<Value> {
                    source::execute(&self.0.socket, request, deadline)
                }
            }
            assert!(e.refresh("epoch", &SourceFixture(config)).is_err());
            assert_eq!(terminal_references(&e), ids);
            let o = observer(vec![e], Err(Error("unused")));
            let fleet = o.fleet();
            assert_eq!(fleet["endpoints"][0]["connection"], "unavailable");
            assert_eq!(fleet["endpoints"][0]["discovery_status"], "unavailable");
            assert_eq!(fleet["endpoints"][0]["agents"].as_array().unwrap().len(), 1);
            assert!(!fleet.to_string().contains("PRIVATE"));
        }
    }

    #[test]
    fn process_timeout_and_socket_replacement_never_publish_fresh_empty_inventory() {
        let snapshot = discovery_snapshot(&[], 1);
        let replies = discovery_replies(&snapshot, &[vec!["djinn"]]);
        for replacement in [false, true] {
            let server = Server::configured(
                vec![
                    (replies[0].0, replies[0].1.clone(), Duration::ZERO),
                    (
                        replies[1].0,
                        replies[1].1.clone(),
                        if replacement {
                            Duration::ZERO
                        } else {
                            Duration::from_millis(200)
                        },
                    ),
                ],
                replacement,
            );
            struct BoundedSource(String);
            impl Transport for BoundedSource {
                fn call(
                    &self,
                    _: &EndpointConfig,
                    request: &Request,
                    deadline: Instant,
                ) -> Result<Value> {
                    source::execute(
                        &self.0,
                        request,
                        deadline.min(Instant::now() + Duration::from_millis(100)),
                    )
                }
            }
            let e = endpoint("a");
            let mut old = inventory("t0");
            old.discovery_status = "observed".into();
            old.agents[0].evidence = "coven_process_discovery".into();
            old.agents[0].activity = "unknown".into();
            e.accept("epoch", old);
            let ids = terminal_references(&e);
            let error = e
                .refresh("epoch", &BoundedSource(server.path.clone()))
                .unwrap_err();
            assert_eq!(
                error.code(),
                if replacement {
                    "terminal_replaced"
                } else {
                    "timeout"
                }
            );
            assert_eq!(terminal_references(&e), ids);
            let fleet = observer(vec![e], Err(Error("unused"))).fleet();
            assert_eq!(fleet["endpoints"][0]["discovery_status"], "unavailable");
            assert_eq!(fleet["endpoints"][0]["agents"].as_array().unwrap().len(), 1);
            assert_eq!(
                fleet["endpoints"][0]["agents"][0]["output_available"],
                false
            );
        }
    }

    #[test]
    fn inventory_uses_raw_agents_without_inventing_completion_or_native_authority() {
        let server = Server::new(
            vec![("session.snapshot", snapshot("t", "done"))],
            Duration::ZERO,
        );
        let reply = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
        assert_eq!(reply["agents"][0]["activity"], "unknown");
        assert_eq!(reply["agents"][0]["provider"], "djinn");
        assert!(!reply.to_string().contains("must-not-project"));
    }

    #[test]
    fn titles_follow_snapshot_precedence_and_cwd_is_bounded_without_output_reads() {
        for (name, terminal_title, title, provider, expected) in [
            (
                Some("Named"),
                Some("Terminal"),
                Some("Title"),
                Some("djinn"),
                "Named",
            ),
            (
                Some(" "),
                Some("Terminal"),
                Some("Title"),
                Some("djinn"),
                "Terminal",
            ),
            (None, None, Some("Title"), Some("djinn"), "Title"),
            (None, None, None, Some("djinn"), "djinn"),
            (None, None, None, None, "Agent"),
        ] {
            let mut result = snapshot("t", "done");
            let agent = &mut result["snapshot"]["agents"][0];
            agent["name"] = json!(name);
            agent["terminal_title"] = json!(terminal_title);
            agent["title"] = json!(title);
            agent["agent"] = json!(provider);
            agent["cwd"] = json!(format!("/{}\n", "é".repeat(600)));
            result["snapshot"]["panes"][0] = agent.clone();
            let server = Server::new(vec![("session.snapshot", result)], Duration::ZERO);
            let reply = probe(&server.path, br#"{"operation":"inventory"}"#).unwrap();
            assert_eq!(reply["agents"][0]["title"], expected);
            assert_eq!(reply["agents"][0]["activity"], "unknown");
            let project = reply["agents"][0]["project"].as_str().unwrap();
            assert!(project.starts_with('/'));
            assert!(project.len() <= 512);
            assert!(!project.chars().any(char::is_control));
        }
    }

    #[test]
    fn ambiguous_pane_mapping_and_unsupported_protocol_are_rejected() {
        let mut ambiguous = snapshot("t", "working");
        let mut other = ambiguous["snapshot"]["panes"][0].clone();
        other["terminal_id"] = json!("other");
        ambiguous["snapshot"]["panes"]
            .as_array_mut()
            .unwrap()
            .push(other);
        let server = Server::new(vec![("session.snapshot", ambiguous)], Duration::ZERO);
        assert_eq!(
            probe(&server.path, br#"{"operation":"inventory"}"#)
                .unwrap_err()
                .code(),
            "schema_error"
        );
        let mut unsupported = snapshot("t", "working");
        unsupported["snapshot"]["protocol"] = json!(19);
        let server = Server::new(vec![("session.snapshot", unsupported)], Duration::ZERO);
        assert_eq!(
            probe(&server.path, br#"{"operation":"inventory"}"#)
                .unwrap_err()
                .code(),
            "unsupported_protocol"
        );
    }

    #[test]
    fn remote_helper_preserves_protocol_rejection_and_nonzero_remains_transport_failure() {
        use std::process::Command;
        let mut unsupported = snapshot("t", "working");
        unsupported["snapshot"]["protocol"] = json!(19);
        let server = Server::new(
            vec![
                ("session.snapshot", unsupported.clone()),
                ("session.snapshot", unsupported),
            ],
            Duration::ZERO,
        );
        let input = br#"{"operation":"inventory"}"#;
        let local_error = probe(&server.path, input).unwrap_err();
        let envelope = probe_response(&server.path, input);
        assert_eq!(envelope, json!({"error":{"code":"unsupported_protocol"}}));
        let stdout = transport::run_helper(
            &mut Command::new("/bin/cat"),
            &serde_json::to_vec(&envelope).unwrap(),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        let remote_error = transport::decode_helper(&stdout, &Request::Inventory {}).unwrap_err();
        assert_eq!(local_error, remote_error);
        let endpoint = endpoint("remote");
        let fixture = FixtureTransport {
            reply: Mutex::new(Err(remote_error)),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        assert_eq!(
            endpoint.refresh("epoch", &fixture).unwrap_err().code(),
            "unsupported_protocol"
        );
        let o = observer(vec![endpoint], Err(Error("unused")));
        assert_eq!(o.fleet()["endpoints"][0]["connection"], "unsupported");
        let error = transport::run_helper(Command::new("/bin/sh").args(["-c", "printf '%s' '{\"error\":{\"code\":\"unsupported_protocol\"}}'; printf PRIVATE >&2; exit 1"]),input,Instant::now()+Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.code(), "upstream_unavailable");
        assert!(!error.to_string().contains("PRIVATE"));
    }

    #[test]
    fn output_brackets_match_and_utf8_is_bounded() {
        let text = format!("{}é", "a".repeat(model::OUTPUT_LIMIT - 1));
        let server = Server::new(
            vec![
                ("session.snapshot", snapshot("t", "working")),
                ("pane.read", read_result(&text)),
                ("session.snapshot", snapshot("t", "idle")),
            ],
            Duration::ZERO,
        );
        let reply = probe(&server.path, br#"{"operation":"output","terminal_id":"t"}"#).unwrap();
        assert_eq!(
            reply["text"].as_str().unwrap().len(),
            model::OUTPUT_LIMIT - 1
        );
        assert_eq!(reply["truncated"], true);
    }

    #[test]
    fn second_snapshot_failure_or_terminal_replacement_discards_output() {
        let mut moved = snapshot("t", "working");
        for rows in ["panes", "agents"] {
            moved["snapshot"][rows][0]["pane_id"] = json!("moved-pane");
            moved["snapshot"][rows][0]["tab_id"] = json!("moved-tab");
            moved["snapshot"][rows][0]["workspace_id"] = json!("moved-workspace");
        }
        for after in [Value::Null, snapshot("replacement", "working"), moved] {
            let server = Server::new(
                vec![
                    ("session.snapshot", snapshot("t", "working")),
                    ("pane.read", read_result("PRIVATE")),
                    ("session.snapshot", after),
                ],
                Duration::ZERO,
            );
            let error =
                probe(&server.path, br#"{"operation":"output","terminal_id":"t"}"#).unwrap_err();
            assert!(matches!(
                error.code(),
                "terminal_replaced" | "upstream_rejected"
            ));
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }

    #[test]
    fn socket_timeout_and_response_caps_are_enforced() {
        let server = Server::new(
            vec![("session.snapshot", snapshot("t", "working"))],
            Duration::from_millis(150),
        );
        let error = source::execute(
            &server.path,
            &Request::Inventory {},
            Instant::now() + Duration::from_millis(30),
        )
        .unwrap_err();
        assert_eq!(error.code(), "timeout");
        let mut big = snapshot("t", "working");
        big["snapshot"]["agents"][0]["name"] = json!("x".repeat(4097));
        let server = Server::new(vec![("session.snapshot", big)], Duration::ZERO);
        assert!(probe(&server.path, br#"{"operation":"inventory"}"#).is_err());
    }

    #[test]
    fn background_collector_is_explicit_cached_and_drop_joins_it() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Observer>();
        let server = Server::new(
            vec![("session.snapshot", snapshot("t", "working"))],
            Duration::from_millis(100),
        );
        let mut config = config();
        config.endpoints[0].socket = server.path.clone();
        let observer = Observer::start(&config).unwrap();
        let start = Instant::now();
        for _ in 0..10 {
            observer.fleet();
        }
        assert!(start.elapsed() < Duration::from_millis(100));
        let deadline = Instant::now() + Duration::from_secs(2);
        while observer.fleet()["endpoints"][0]["agents"]
            .as_array()
            .unwrap()
            .is_empty()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            observer.fleet()["endpoints"][0]["agents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let start = Instant::now();
        drop(observer);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
