use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

struct Fake;
impl Api for Fake {
    fn fleet(&self) -> Value {
        json!({})
    }
    fn output(&self, _: &str) -> Result<Value> {
        panic!("not terminal output")
    }
    fn attention(&self, _: &str) -> Value {
        json!({"contract":"coven.attention.v1","state":"partial","items":[],"omitted_count":null,"unavailable_count":1,"unlinked_count":0})
    }
    fn decision_response_allowed(&self, _: &str, _: &str) -> bool {
        true
    }
    fn push_allowed(&self, _: &str) -> bool {
        true
    }
    fn respond_decision(&self, _: &str, _: &str, _: &str, _: &Value) -> Result<Value> {
        Err(Error("private/native/path/answer"))
    }
    fn push_subscribe(&self, _: &str, _: &Value) -> Result<Value> {
        Ok(
            json!({"contract":"coven.web-push.v1","subscription_id":"a".repeat(64),"subscribed":true}),
        )
    }
    fn push_unsubscribe(&self, _: &str, _: &str) -> Result<Value> {
        Ok(
            json!({"contract":"coven.web-push.v1","subscription_id":"a".repeat(64),"subscribed":false}),
        )
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .unwrap()
}
fn isolated_observer(config: &Config) -> Observer {
    struct Poison;
    impl super::super::transport::Transport for Poison {
        fn call(
            &self,
            _: &super::super::config::EndpointConfig,
            _: &super::super::source::Request,
            _: Instant,
        ) -> Result<Value> {
            panic!("HTTP fixture must not contact native transport")
        }
    }
    Observer {
        events: Arc::default(),
        attention: std::sync::Mutex::new(Default::default()),
        cardlinks: std::sync::Mutex::new(Default::default()),
        conversations: super::super::conversations::Conversations::new(config, "http-fixture")
            .unwrap(),
        endpoints: Vec::new(),
        allow_output: false,
        transport: Arc::new(Poison),
        shutdown: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
        pollers: Vec::new(),
    }
}
fn metadata(policy: &Policy, path: &str, length: usize) -> Metadata {
    let mut meta = tests::meta(path);
    meta.method = "POST".into();
    meta.headers.extend([
        ("Origin".into(), policy.origin.clone()),
        (
            "X-Coven-CSRF".into(),
            policy.csrf.issue("owner@example.invalid").unwrap(),
        ),
        ("Content-Type".into(), "application/json".into()),
        ("Content-Length".into(), length.to_string()),
    ]);
    meta
}
fn payload() -> Vec<u8> {
    serde_json::to_vec(&json!({"contract":"coven.decision-response.v1","attempt_id":"12345678-1234-4234-8234-123456789abc","execute":true,"action":{"type":"question_answer","answers":[["literal\t\r\n"]]}})).unwrap()
}
struct Never(Arc<AtomicUsize>);
impl hyper::body::Body for Never {
    type Data = Bytes;
    type Error = std::convert::Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<hyper::body::Frame<Bytes>, Self::Error>>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("refused body must never be polled")
    }
}
#[test]
fn closed_decision_schema_duplicate_unknown_utf8_and_answer_limits() {
    let valid = String::from_utf8(payload()).unwrap();
    assert!(parse_decision(valid.as_bytes()).is_some());
    for body in [
        valid.replace("\"execute\":true", "\"execute\":true,\"execute\":true"),
        valid.replace(
            "\"execute\":true",
            "\"execute\":true,\"native_id\":\"secret\"",
        ),
        valid.replace("\"answers\":", "\"answers\":[],\"answers\":"),
        valid.replace("\"type\":", "\"unexpected\":true,\"type\":"),
        valid.replace("question_answer", "permission_always"),
        valid.replace("literal\\t\\r\\n", &"\u{00e9}".repeat(2049)),
        valid.replace("literal\\t\\r\\n", "bad\\u0000"),
    ] {
        assert!(parse_decision(body.as_bytes()).is_none());
    }
    assert!(parse_decision(b"\xff").is_none());
    for action in [
        json!({"type":"question_reject"}),
        json!({"type":"permission_once"}),
        json!({"type":"permission_deny","scope":"session_pending"}),
    ] {
        let mut value: Value = serde_json::from_slice(&payload()).unwrap();
        value["action"] = action;
        assert!(parse_decision(&serde_json::to_vec(&value).unwrap()).is_some());
        value["action"]["extra"] = json!(true);
        assert!(parse_decision(&serde_json::to_vec(&value).unwrap()).is_none());
    }
}
#[test]
fn decision_and_push_preflight_refuse_before_polling_any_body() {
    let runtime = runtime();
    let policy = Arc::new(tests::policy());
    for path in [
        format!("/api/v1/decisions/{}/responses", "a".repeat(64)),
        "/api/v1/push/subscriptions".into(),
        "/api/v1/push/unsubscribe".into(),
    ] {
        for case in 0..12 {
            let mut meta = metadata(&policy, &path, 100);
            match case {
                0 => meta.headers.retain(|(k, _)| k != "X-Coven-CSRF"),
                1 => meta
                    .headers
                    .push(("Origin".into(), "https://evil.invalid".into())),
                2 => meta.headers.push(("Content-Length".into(), "100".into())),
                3 => meta
                    .headers
                    .push(("Transfer-Encoding".into(), "chunked".into())),
                4 => meta
                    .headers
                    .push(("Content-Encoding".into(), "gzip".into())),
                5 => meta
                    .headers
                    .push(("Sec-Fetch-Mode".into(), "navigate".into())),
                6 => meta
                    .headers
                    .push(("Sec-Fetch-Dest".into(), "document".into())),
                7 => meta.headers.push(("Host".into(), "evil.invalid".into())),
                8 => meta.loopback = false,
                9 => {
                    meta.headers.retain(|(k, _)| k != "Content-Length");
                    meta.headers.push(("Content-Length".into(), "65537".into()));
                }
                10 => {
                    meta.headers.retain(|(k, _)| k != "Tailscale-User-Login");
                }
                _ => {}
            }
            let polls = Arc::new(AtomicUsize::new(0));
            let api: Arc<dyn Api> = if case == 11 {
                Arc::new(tests::Fake(AtomicUsize::new(0)))
            } else {
                Arc::new(Fake)
            };
            let reply = runtime.block_on(submit_request(
                api,
                policy.clone(),
                Arc::new(Semaphore::new(2)),
                meta,
                Never(polls.clone()),
            ));
            assert_ne!(reply.status, 200);
            assert_eq!(polls.load(Ordering::Relaxed), 0);
        }
    }
}
#[test]
fn real_dispatch_uses_bounded_reads_and_sanitizes_lost_decision_receipt() {
    let runtime = runtime();
    let policy = Arc::new(tests::policy());
    let path = format!("/api/v1/decisions/{}/responses", "a".repeat(64));
    let bytes = payload();
    let reply = runtime.block_on(submit_request(
        Arc::new(Fake),
        policy.clone(),
        Arc::new(Semaphore::new(2)),
        metadata(&policy, &path, bytes.len()),
        Full::new(Bytes::from(bytes)),
    ));
    assert_eq!(reply.status, 200);
    let report: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(report["outcome"], "uncertain");
    assert!(!String::from_utf8(reply.body).unwrap().contains("private"));
    let read = runtime.block_on(dispatch(
        Arc::new(Fake),
        policy.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(Semaphore::new(0)),
        tests::meta("/api/v1/attention"),
    ));
    assert_eq!(read.status, 429);
    let default = route(
        &tests::Fake(AtomicUsize::new(0)),
        &policy,
        &AtomicUsize::new(0),
        &tests::meta("/api/v1/push/config"),
    );
    let config: Value = serde_json::from_slice(&default.body).unwrap();
    assert_eq!(config["enabled"], false);
    assert_eq!(config["public_key"], Value::Null);
}

#[test]
fn push_schema_is_closed_at_every_level_and_uses_eight_kib_framing_cap() {
    let runtime = runtime();
    let policy = Arc::new(tests::policy());
    let valid = r#"{"contract":"coven.web-push.v1","execute":true,"subscription":{"endpoint":"https://push.services.mozilla.com/fixture","keys":{"p256dh":"browser","auth":"browser"}}}"#;
    for body in [
        valid.replace("\"execute\":true", "\"execute\":true,\"execute\":true"),
        valid.replace("\"execute\":true", "\"execute\":true,\"extra\":true"),
        valid.replace("\"endpoint\":", "\"extra\":true,\"endpoint\":"),
        valid.replace("\"p256dh\":", "\"extra\":true,\"p256dh\":"),
        valid.replace("\"auth\":", "\"auth\":\"duplicate\",\"auth\":"),
    ] {
        let reply = runtime.block_on(submit_request(
            Arc::new(Fake),
            policy.clone(),
            Arc::new(Semaphore::new(1)),
            metadata(&policy, "/api/v1/push/subscriptions", body.len()),
            Full::new(Bytes::from(body)),
        ));
        assert_eq!(reply.status, 400);
    }
    let reply = runtime.block_on(submit_request(
        Arc::new(Fake),
        policy.clone(),
        Arc::new(Semaphore::new(1)),
        metadata(&policy, "/api/v1/push/subscriptions", valid.len()),
        Full::new(Bytes::from(valid)),
    ));
    assert_eq!(reply.status, 200);
    let polls = Arc::new(AtomicUsize::new(0));
    let reply = runtime.block_on(submit_request(
        Arc::new(Fake),
        policy.clone(),
        Arc::new(Semaphore::new(1)),
        metadata(&policy, "/api/v1/push/subscriptions", 8193),
        Never(polls.clone()),
    ));
    assert_eq!(reply.status, 400);
    assert_eq!(polls.load(Ordering::Relaxed), 0);
}

#[test]
fn canceled_decision_and_push_waiters_keep_blocking_mutation_permits() {
    struct Blocking {
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }
    impl Blocking {
        fn block(&self) -> Result<Value> {
            self.entered.send(()).unwrap();
            let (lock, wake) = &*self.release;
            let _guard = wake
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(3), |ready| {
                    !*ready
                })
                .unwrap();
            Err(Error("unavailable"))
        }
    }
    impl Api for Blocking {
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            panic!("not output")
        }
        fn decision_response_allowed(&self, _: &str, _: &str) -> bool {
            true
        }
        fn push_allowed(&self, _: &str) -> bool {
            true
        }
        fn respond_decision(&self, _: &str, _: &str, _: &str, _: &Value) -> Result<Value> {
            self.block()
        }
        fn push_subscribe(&self, _: &str, _: &Value) -> Result<Value> {
            self.block()
        }
        fn push_unsubscribe(&self, _: &str, _: &str) -> Result<Value> {
            self.block()
        }
    }
    let runtime = runtime();
    for (path, bytes) in [
        (format!("/api/v1/decisions/{}/responses", "a".repeat(64)), payload()),
        ("/api/v1/push/subscriptions".into(), serde_json::to_vec(&json!({"contract":"coven.web-push.v1","execute":true,"subscription":{"endpoint":"browser","keys":{"p256dh":"browser","auth":"browser"}}})).unwrap()),
        ("/api/v1/push/unsubscribe".into(), serde_json::to_vec(&json!({"contract":"coven.web-push.v1","execute":true,"subscription_id":"a".repeat(64)})).unwrap()),
    ] {
        let (entered, receive) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let api: Arc<dyn Api> = Arc::new(Blocking {entered, release: release.clone()});
        let policy = Arc::new(tests::policy());
        let mutations = Arc::new(Semaphore::new(1));
        runtime.block_on(async {
            let task = tokio::spawn(submit_request(api.clone(), policy.clone(), mutations.clone(), metadata(&policy, &path, bytes.len()), Full::new(Bytes::from(bytes))));
            receive.recv_timeout(Duration::from_secs(1)).unwrap();
            task.abort(); assert!(task.await.is_err());
            assert_eq!(mutations.available_permits(), 0);
            let polls = Arc::new(AtomicUsize::new(0));
            let reply = submit_request(api.clone(), policy.clone(), mutations.clone(), metadata(&policy, &path, 100), Never(polls.clone())).await;
            assert_eq!(reply.status, 429); assert_eq!(polls.load(Ordering::Relaxed), 0);
            *release.0.lock().unwrap() = true; release.1.notify_all();
            timeout(Duration::from_secs(1), async { while mutations.available_permits() != 1 { tokio::time::sleep(Duration::from_millis(5)).await; } }).await.unwrap();
        });
    }
}

#[test]
fn real_web_api_push_lifecycle_has_no_observer_arc_cycle_and_disabled_is_inert() {
    let directory = tempfile::tempdir().unwrap();
    let mut config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341",
        "public_origin":"https://host.example.ts.net","allowed_logins":["owner@example.invalid"],
        "endpoints":[{"id":"fixture","label":"Fixture","socket":directory.path().join("absent.sock")}]})).unwrap();
    config.validate().unwrap();
    let observer = Arc::new(isolated_observer(&config));
    assert!(observer.pollers.is_empty());
    let api = WebApi::start(&config, observer.clone()).unwrap();
    assert!(api.push.is_none());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert_eq!(Arc::strong_count(&observer), 2);
    let response = route(
        &api,
        &tests::policy(),
        &AtomicUsize::new(0),
        &tests::meta("/api/v1/push/config"),
    )
    .response()
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    drop(api);
    assert_eq!(Arc::strong_count(&observer), 1);

    config.push = Some(super::super::push::PushConfig {
        state_directory: directory.path().join("push").to_str().unwrap().into(),
    });
    let api = WebApi::start(&config, observer.clone()).unwrap();
    let push = api.push.as_ref().unwrap().clone();
    let weak_observer = Arc::downgrade(&observer);
    let weak_push = Arc::downgrade(&push);
    assert_eq!(
        Arc::strong_count(&observer),
        2,
        "collector must capture only Weak<Observer>"
    );
    assert_eq!(
        push.status("owner@example.invalid")["subscriptions"],
        json!([])
    );
    drop(api);
    drop(observer);
    assert!(
        weak_observer.upgrade().is_none(),
        "live push service must not retain Observer"
    );
    let started = Instant::now();
    drop(push); // Joins its idle collector/sender before fixture teardown.
    assert!(weak_push.upgrade().is_none());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn real_backend_cache_refusals_do_not_poll_body_or_contact_transport() {
    let directory = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341",
        "public_origin":"https://host.example.ts.net","allowed_logins":["owner@example.invalid"],
        "endpoints":[{"id":"fixture","label":"Fixture","socket":directory.path().join("absent.sock")}]})).unwrap();
    config.validate().unwrap();
    let api = Arc::new(WebApi::start(&config, Arc::new(isolated_observer(&config))).unwrap());
    let policy = Arc::new(tests::policy());
    let runtime = runtime();
    for path in [
        format!("/api/v1/decisions/{}/responses", "a".repeat(64)),
        "/api/v1/push/subscriptions".into(),
        "/api/v1/push/unsubscribe".into(),
    ] {
        let polls = Arc::new(AtomicUsize::new(0));
        let reply = runtime.block_on(submit_request(
            api.clone(),
            policy.clone(),
            Arc::new(Semaphore::new(1)),
            metadata(&policy, &path, 100),
            Never(polls.clone()),
        ));
        assert_eq!(reply.status, 403);
        assert_eq!(polls.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn admitted_http_body_can_outlive_its_reference_without_turning_known_unsent_into_uncertainty() {
    // The Api admission bit represents a cached reference valid at preflight;
    // body polling withdraws it before the real backend's initial lookup.
    struct Admitted {
        observer: Observer,
        allowed: std::sync::atomic::AtomicBool,
        preflights: AtomicUsize,
        responses: AtomicUsize,
    }
    impl Api for Admitted {
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            panic!("not output")
        }
        fn decision_response_allowed(&self, _: &str, _: &str) -> bool {
            self.preflights.fetch_add(1, Ordering::Relaxed);
            self.allowed.load(Ordering::Acquire)
        }
        fn respond_decision(
            &self,
            login: &str,
            id: &str,
            attempt: &str,
            action: &Value,
        ) -> Result<Value> {
            assert!(!self.allowed.load(Ordering::Acquire));
            self.responses.fetch_add(1, Ordering::Relaxed);
            self.observer.respond_decision(login, id, attempt, action)
        }
    }
    struct RevokingBody {
        api: Arc<Admitted>,
        body: Full<Bytes>,
    }
    impl hyper::body::Body for RevokingBody {
        type Data = Bytes;
        type Error = std::convert::Infallible;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<std::result::Result<hyper::body::Frame<Bytes>, Self::Error>>> {
            assert_eq!(self.api.preflights.load(Ordering::Relaxed), 1);
            self.api.allowed.store(false, Ordering::Release);
            *self.api.observer.attention.lock().unwrap() = Default::default();
            hyper::body::Body::poll_frame(Pin::new(&mut self.body), cx)
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let config: Config = serde_json::from_value(json!({"listen":"127.0.0.1:7341",
        "public_origin":"https://host.example.ts.net","allowed_logins":["owner@example.invalid"],
        "endpoints":[{"id":"fixture","label":"Fixture","socket":directory.path().join("absent.sock")}]})).unwrap();
    let api = Arc::new(Admitted {
        observer: isolated_observer(&config),
        allowed: std::sync::atomic::AtomicBool::new(true),
        preflights: AtomicUsize::new(0),
        responses: AtomicUsize::new(0),
    });
    let policy = Arc::new(tests::policy());
    let path = format!("/api/v1/decisions/{}/responses", "a".repeat(64));
    let bytes = payload();
    let reply = runtime().block_on(submit_request(
        api.clone(),
        policy.clone(),
        Arc::new(Semaphore::new(1)),
        metadata(&policy, &path, bytes.len()),
        RevokingBody {
            api: api.clone(),
            body: Full::new(Bytes::from(bytes)),
        },
    ));
    assert_eq!(api.responses.load(Ordering::Relaxed), 1);
    assert_eq!(reply.status, 200);
    let report: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(report["contract"], "coven.decision-response.v1");
    assert_eq!(report["outcome"], "unavailable", "{report}");
    assert_eq!(report["delivery"], "unsent", "{report}");
    assert_eq!(report["id"], "a".repeat(64));
}

#[test]
fn unexpected_blocking_decision_failure_still_returns_conservative_uncertainty() {
    struct Panics;
    impl Api for Panics {
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            panic!("not output")
        }
        fn decision_response_allowed(&self, _: &str, _: &str) -> bool {
            true
        }
        fn respond_decision(&self, _: &str, _: &str, _: &str, _: &Value) -> Result<Value> {
            panic!("synthetic lost receipt")
        }
    }
    let policy = Arc::new(tests::policy());
    let bytes = payload();
    let path = format!("/api/v1/decisions/{}/responses", "a".repeat(64));
    let reply = runtime().block_on(submit_request(
        Arc::new(Panics),
        policy.clone(),
        Arc::new(Semaphore::new(1)),
        metadata(&policy, &path, bytes.len()),
        Full::new(Bytes::from(bytes)),
    ));
    let report: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(report["outcome"], "uncertain");
    assert_eq!(report["delivery"], "uncertain");
    assert_eq!(report["diagnostics"], json!(["decision_uncertain"]));
    assert!(!String::from_utf8(reply.body).unwrap().contains("synthetic"));
}
