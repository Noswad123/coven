use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

const REQUEST: &str = "12345678-1234-4234-8234-123456789abc";
struct Writable;
impl Api for Writable {
    fn fleet(&self) -> Value {
        json!({})
    }
    fn output(&self, _: &str) -> Result<Value> {
        Err(Error("output_disabled"))
    }
    fn submission_allowed(&self, _: &str, _: &str) -> bool {
        true
    }
    fn submit(&self, _: &str, id: &str, request: &str, text: &str) -> Result<Value> {
        assert_eq!(text, "<script>inert</script>\t\r\n");
        Ok(
            json!({"contract":"coven.conversation-submit.v1","id":id,"request_id":request,"outcome":"accepted","delivery":"accepted"}),
        )
    }
}
fn policy() -> Policy {
    tests::policy()
}
fn payload() -> Vec<u8> {
    serde_json::to_vec(
        &json!({"contract":"coven.conversation-submit.v1","request_id":REQUEST,
        "execute":true,"text":"<script>inert</script>\t\r\n"}),
    )
    .unwrap()
}
fn metadata(policy: &Policy, length: usize) -> Metadata {
    let mut meta = tests::meta(&format!("/api/v1/conversations/{}/prompts", "a".repeat(64)));
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
struct Pending(Arc<AtomicUsize>);
impl hyper::body::Body for Pending {
    type Data = Bytes;
    type Error = std::convert::Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<hyper::body::Frame<Bytes>, Self::Error>>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Poll::Pending
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

#[test]
fn csrf_is_unpredictable_principal_epoch_and_expiry_bound() {
    let mut csrf = Csrf::new().unwrap();
    let token = csrf.issue("alice").unwrap();
    assert_ne!(token, csrf.issue("alice").unwrap());
    assert!(csrf.verify("alice", &token));
    assert!(!csrf.verify("bob", &token));
    assert!(!Csrf::new().unwrap().verify("alice", &token));
    let forged = format!("{}0", &token[..token.len() - 1]);
    if forged != token {
        assert!(!csrf.verify("alice", &forged));
    }
    csrf.started = Instant::now() - Duration::from_secs(61);
    assert!(!csrf.verify("alice", &token));
}

#[test]
fn strict_json_exact_text_uuid_and_utf8_limits() {
    let bytes = payload();
    assert_eq!(
        parse_prompt(&bytes).unwrap().text,
        "<script>inert</script>\t\r\n"
    );
    let valid = String::from_utf8(bytes).unwrap();
    for invalid in [
        valid.replace("\"execute\":true", "\"execute\":true,\"execute\":true"),
        valid.replace(
            "\"execute\":true",
            "\"execute\":true,\"socket\":\"/private\"",
        ),
        valid.replace(REQUEST, &REQUEST.to_uppercase()),
        valid.replace("\"execute\":true", "\"execute\":false"),
    ] {
        assert!(parse_prompt(invalid.as_bytes()).is_none(), "{invalid}");
    }
    assert!(parse_prompt(b"\xff").is_none());
    for text in [
        " \t\r\n".into(),
        "bad\0".into(),
        "bad\u{0085}".into(),
        "\u{e9}".repeat(32769),
    ] {
        let mut value: Value = serde_json::from_str(&valid).unwrap();
        value["text"] = json!(text);
        assert!(parse_prompt(&serde_json::to_vec(&value).unwrap()).is_none());
    }
    let mut value: Value = serde_json::from_str(&valid).unwrap();
    value["text"] = json!("\u{e9}".repeat(32768));
    assert!(parse_prompt(&serde_json::to_vec(&value).unwrap()).is_some());
}

#[test]
fn preflight_rejections_never_poll_bodies() {
    runtime().block_on(async {
        let policy = Arc::new(policy());
        let api: Arc<dyn Api> = Arc::new(Writable);
        let permits = Arc::new(Semaphore::new(2));
        for (name, value) in [
            ("Host", Some("evil.invalid")),
            ("Tailscale-User-Login", None),
            ("Origin", None),
            ("Origin", Some("https://evil.invalid")),
            ("Sec-Fetch-Site", Some("cross-site")),
            ("Sec-Fetch-Mode", Some("navigate")),
            ("X-Coven-CSRF", None),
            ("X-Coven-CSRF", Some("forged")),
            ("Content-Type", Some("text/plain")),
            ("Content-Length", Some("409601")),
            ("Content-Length", None),
            ("Transfer-Encoding", Some("chunked")),
            ("Content-Encoding", Some("gzip")),
        ] {
            let mut meta = metadata(&policy, payload().len());
            meta.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            if let Some(value) = value {
                meta.headers.push((name.into(), value.into()));
            }
            let polls = Arc::new(AtomicUsize::new(0));
            let reply = submit_request(
                api.clone(),
                policy.clone(),
                permits.clone(),
                meta,
                Pending(polls.clone()),
            )
            .await;
            assert_ne!(reply.status, 200, "{name}");
            assert_eq!(polls.load(Ordering::Relaxed), 0);
        }
        for name in [
            "Host",
            "Tailscale-User-Login",
            "Origin",
            "Sec-Fetch-Site",
            "X-Coven-CSRF",
            "Content-Length",
            "Content-Type",
        ] {
            let mut meta = metadata(&policy, payload().len());
            let value = meta.header(name).unwrap().unwrap().to_owned();
            meta.headers.push((name.to_lowercase(), value));
            let polls = Arc::new(AtomicUsize::new(0));
            assert_ne!(
                submit_request(
                    api.clone(),
                    policy.clone(),
                    permits.clone(),
                    meta,
                    Pending(polls.clone())
                )
                .await
                .status,
                200
            );
            assert_eq!(polls.load(Ordering::Relaxed), 0, "duplicate {name}");
        }
        let polls = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            submit_request(
                Arc::new(tests::Fake(AtomicUsize::new(0))),
                policy.clone(),
                permits.clone(),
                metadata(&policy, payload().len()),
                Pending(polls.clone())
            )
            .await
            .status,
            403
        );
        assert_eq!(polls.load(Ordering::Relaxed), 0);
    });
}

#[test]
fn bounded_body_timeout_and_actual_submission() {
    runtime().block_on(async {
        let policy = Arc::new(policy());
        let api: Arc<dyn Api> = Arc::new(Writable);
        let permits = Arc::new(Semaphore::new(2));
        let bytes = payload();
        let response = submit_request(
            api.clone(),
            policy.clone(),
            permits.clone(),
            metadata(&policy, bytes.len()),
            Full::new(Bytes::from(bytes)),
        )
        .await;
        assert_eq!(response.status, 200);
        assert!(!String::from_utf8(response.body).unwrap().contains("script"));
        let polls = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        assert_eq!(
            submit_request(
                api.clone(),
                policy.clone(),
                permits.clone(),
                metadata(&policy, 1),
                Pending(polls.clone())
            )
            .await
            .status,
            408
        );
        assert!(start.elapsed() >= Duration::from_secs(2));
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(polls.load(Ordering::Relaxed) > 0);
        assert_eq!(permits.available_permits(), 2);
        assert_eq!(
            submit_request(
                api,
                policy.clone(),
                permits,
                metadata(&policy, 1),
                Full::new(Bytes::from_static(b"too long"))
            )
            .await
            .status,
            400
        );
    });
}

#[test]
fn canceled_mutations_keep_capacity_until_blocking_jobs_finish() {
    struct Blocking {
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }
    impl Api for Blocking {
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            unreachable!()
        }
        fn submission_allowed(&self, _: &str, _: &str) -> bool {
            true
        }
        fn submit(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value> {
            self.entered.send(()).unwrap();
            let _ = self
                .release
                .1
                .wait_timeout_while(
                    self.release.0.lock().unwrap(),
                    Duration::from_secs(3),
                    |v| !*v,
                )
                .unwrap();
            Err(Error("receipt_unavailable"))
        }
    }
    let (entered, receive) = std::sync::mpsc::channel();
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let api: Arc<dyn Api> = Arc::new(Blocking {
        entered,
        release: release.clone(),
    });
    let permits = Arc::new(Semaphore::new(2));
    let policy = Arc::new(policy());
    runtime().block_on(async {
        for _ in 0..2 {
            let bytes = payload();
            let task = tokio::spawn(submit_request(
                api.clone(),
                policy.clone(),
                permits.clone(),
                metadata(&policy, bytes.len()),
                Full::new(Bytes::from(bytes)),
            ));
            receive.recv_timeout(Duration::from_secs(1)).unwrap();
            task.abort();
            assert!(task.await.is_err_and(|e| e.is_cancelled()));
        }
        assert_eq!(permits.available_permits(), 0);
        let polls = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            submit_request(
                api.clone(),
                policy.clone(),
                permits.clone(),
                metadata(&policy, 1),
                Pending(polls.clone())
            )
            .await
            .status,
            429
        );
        assert_eq!(polls.load(Ordering::Relaxed), 0);
        assert_eq!(
            dispatch(
                api,
                policy,
                Arc::new(AtomicUsize::new(0)),
                Arc::new(Semaphore::new(2)),
                tests::meta("/api/v1/fleet")
            )
            .await
            .status,
            200
        );
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        timeout(Duration::from_secs(1), async {
            while permits.available_permits() != 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn discovery_runs_on_a_bounded_blocking_job_and_keeps_capacity_after_cancel() {
    struct Discovery {
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }
    impl Api for Discovery {
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            unreachable!()
        }
        fn conversations(&self, _: &str) -> Value {
            self.entered.send(()).unwrap();
            let _ = self
                .release
                .1
                .wait_timeout_while(
                    self.release.0.lock().unwrap(),
                    Duration::from_secs(3),
                    |v| !*v,
                )
                .unwrap();
            json!({"contract":"coven.conversations.v1","conversations":[]})
        }
    }
    let (entered, receive) = std::sync::mpsc::channel();
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let api: Arc<dyn Api> = Arc::new(Discovery {
        entered,
        release: release.clone(),
    });
    let outputs = Arc::new(Semaphore::new(1));
    let policy = Arc::new(policy());
    let slots = Arc::new(AtomicUsize::new(0));
    runtime().block_on(async {
        let task = tokio::spawn(dispatch(
            api.clone(),
            policy.clone(),
            slots.clone(),
            outputs.clone(),
            tests::meta("/api/v1/conversations"),
        ));
        receive
            .recv_timeout(Duration::from_secs(1))
            .expect("discovery starts off the async root thread");
        task.abort();
        assert!(task.await.is_err());
        assert_eq!(outputs.available_permits(), 0);
        assert_eq!(
            dispatch(
                api.clone(),
                policy.clone(),
                slots.clone(),
                outputs.clone(),
                tests::meta("/api/v1/conversations")
            )
            .await
            .status,
            429
        );
        assert_eq!(
            dispatch(
                api,
                policy,
                slots,
                outputs.clone(),
                tests::meta("/api/v1/fleet")
            )
            .await
            .status,
            200
        );
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        timeout(Duration::from_secs(1), async {
            while outputs.available_permits() != 1 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    });
}
