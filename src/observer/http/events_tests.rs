use super::tests::{meta, policy};
use super::*;

struct EventApi(Arc<Events>);
impl Api for EventApi {
    fn cached_metadata(&self) -> bool {
        true
    }
    fn events(&self) -> Option<Arc<Events>> {
        Some(self.0.clone())
    }
    fn fleet(&self) -> Value {
        panic!("no source reads")
    }
    fn output(&self, _: &str) -> Result<Value> {
        panic!("no source reads")
    }
}

#[test]
fn events_require_exact_authorized_empty_get_before_admission() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let api = EventApi(Arc::default());
        let policy = policy();
        let check = |m: &Metadata| event_stream(&api, &policy, m).err().unwrap().status;
        let mut request = meta("/api/v1/events");
        request.loopback = false;
        assert_eq!(check(&request), 403);
        for (name, value, status) in [
            ("Host", "evil.invalid", 403),
            ("Origin", "https://evil.invalid", 403),
            ("Sec-Fetch-Site", "cross-site", 403),
            ("Tailscale-User-Login", "other", 403),
            ("Content-Length", "1", 400),
            ("Transfer-Encoding", "chunked", 400),
            ("Content-Encoding", "gzip", 400),
        ] {
            let mut request = meta("/api/v1/events");
            request
                .headers
                .retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            request.headers.push((name.into(), value.into()));
            assert_eq!(check(&request), status);
        }
        for path in ["/api/v1/events?", "/api/v1/events?x=1", "/api/v1/events/"] {
            assert_eq!(check(&meta(path)), 404);
        }
        for method in ["POST", "HEAD", "PUT", "OPTIONS"] {
            let mut request = meta("/api/v1/events");
            request.method = method.into();
            assert_eq!(check(&request), 405);
        }
        let a = event_stream(&api, &policy, &meta("/api/v1/events"))
            .ok()
            .unwrap();
        let b = event_stream(&api, &policy, &meta("/api/v1/events"))
            .ok()
            .unwrap();
        assert_eq!(check(&meta("/api/v1/events")), 429);
        drop(a);
        assert!(event_stream(&api, &policy, &meta("/api/v1/events")).is_ok());
        drop(b);
        // Cached metadata reads do not consume either native read permit.
        for path in ["/api/v1/conversations", "/api/v1/attention"] {
            let reply = dispatch(
                Arc::new(EventApi(Arc::default())),
                Arc::new(self::policy()),
                Arc::new(AtomicUsize::new(2)),
                Arc::new(Semaphore::new(0)),
                meta(path),
            )
            .await;
            assert_eq!(reply.status, 200);
        }
    });
}

#[test]
fn real_sse_connection_streams_ready_then_immediate_scoped_notifications() {
    use std::io::{Read, Write};
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let events = Arc::new(Events::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let notify = events.clone();
        let client = tokio::task::spawn_blocking(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(12))).unwrap();
            let started = Instant::now();
            stream.write_all(b"GET /api/v1/events HTTP/1.1\r\nHost: host.example.ts.net\r\nTailscale-User-Login: owner@example.invalid\r\n\r\n").unwrap();
            let mut text = String::new();
            let mut bytes = [0; 2048];
            while !text.contains("event: ready") {
                let n = stream.read(&mut bytes).unwrap(); assert!(n > 0);
                text.push_str(std::str::from_utf8(&bytes[..n]).unwrap());
            }
            notify.notify(None);
            notify.notify(Some("owner@example.invalid"));
            while !text.contains("event: metadata") || !text.contains("event: fleet") {
                let n = stream.read(&mut bytes).unwrap(); assert!(n > 0);
                text.push_str(std::str::from_utf8(&bytes[..n]).unwrap());
            }
            assert!(text.contains("content-type: text/event-stream"));
            assert!(text.find("event: ready").unwrap() < text.find("event: fleet").unwrap());
            loop {
                let n = stream.read(&mut bytes).unwrap();
                if n == 0 { break; }
                text.push_str(std::str::from_utf8(&bytes[..n]).unwrap());
                assert!(text.len() < 4096);
            }
            assert!(started.elapsed() >= Duration::from_secs(59));
            assert!(started.elapsed() < Duration::from_secs(65));
            assert!(text.matches(": heartbeat\n\n").count() >= 5);
        });
        let (stream, _) = listener.accept().await.unwrap();
        let server = tokio::spawn(connection(stream, true, Arc::new(EventApi(events)), Arc::new(policy()),
            Arc::new(AtomicUsize::new(2)), Arc::new(Semaphore::new(0)), Arc::new(Semaphore::new(0))));
        client.await.unwrap();
        timeout(Duration::from_secs(3), server).await.unwrap().unwrap();
    });
}

#[test]
fn ordinary_connection_keeps_twenty_second_deadline_and_job_owns_read_permit() {
    use std::io::{Read, Write};
    struct Slow(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
    impl Api for Slow {
        fn events(&self) -> Option<Arc<Events>> {
            Some(Arc::default())
        }
        fn fleet(&self) -> Value {
            json!({})
        }
        fn output(&self, _: &str) -> Result<Value> {
            let (lock, wake) = &*self.0;
            let _ = wake
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(23), |release| {
                    !*release
                })
                .unwrap();
            Err(Error("timeout"))
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let outputs = Arc::new(Semaphore::new(1));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let done = release.clone();
        let permits = outputs.clone();
        let client = tokio::task::spawn_blocking(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(22))).unwrap();
            write!(stream, "GET /api/v1/agents/{}/output HTTP/1.1\r\nHost: host.example.ts.net\r\nTailscale-User-Login: owner@example.invalid\r\n\r\n", "a".repeat(64)).unwrap();
            let start = Instant::now();
            let mut bytes = Vec::new();
            let result = stream.read_to_end(&mut bytes);
            let still_owned = permits.available_permits() == 0;
            *done.0.lock().unwrap() = true;
            done.1.notify_all();
            result.unwrap();
            assert!(bytes.is_empty());
            assert!(still_owned);
            assert!(start.elapsed() >= Duration::from_secs(19));
            assert!(start.elapsed() < Duration::from_secs(22));
        });
        let (stream, _) = listener.accept().await.unwrap();
        let server = tokio::spawn(connection(stream, true, Arc::new(Slow(release)), Arc::new(policy()),
            Arc::new(AtomicUsize::new(0)), outputs.clone(), Arc::new(Semaphore::new(0))));
        client.await.unwrap();
        server.await.unwrap();
        let until = Instant::now() + Duration::from_secs(1);
        while outputs.available_permits() == 0 {
            assert!(Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
}
