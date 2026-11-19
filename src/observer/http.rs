//! Same-origin loopback HTTP facade. Serve is trusted; local processes are trusted too.
use super::{Config, Observer};
use crate::{Error, Result};
use http_body_util::Full;
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};

const MAX_JSON: usize = 2 * 1024 * 1024;
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'";

trait Api: Send + Sync {
    fn fleet(&self) -> Value;
    fn output(&self, id: &str) -> Result<Value>;
}
impl Api for Observer {
    fn fleet(&self) -> Value {
        Observer::fleet(self)
    }
    fn output(&self, id: &str) -> Result<Value> {
        Observer::output(self, id)
    }
}

struct Policy {
    authority: String,
    origin: String,
    logins: Vec<String>,
}
impl Policy {
    fn from_config(config: &Config) -> Self {
        Self {
            authority: config.public_origin["https://".len()..].to_owned(),
            origin: config.public_origin.clone(),
            logins: config.allowed_logins.clone(),
        }
    }

    fn authorize(&self, meta: &Metadata) -> std::result::Result<(), u16> {
        if !meta.loopback {
            return Err(403);
        }
        if meta.url.len() > 2048
            || meta.headers.len() > 64
            || meta
                .headers
                .iter()
                .map(|(a, b)| a.len() + b.len())
                .sum::<usize>()
                > 8192
        {
            return Err(431);
        }
        let header = |name| meta.header(name).map_err(|()| 400u16);
        if header("host")? != Some(self.authority.as_str()) {
            return Err(403);
        }
        let login = header("tailscale-user-login")?.ok_or(403u16)?;
        if !self.logins.iter().any(|allowed| allowed == login) {
            return Err(403);
        }
        if let Some(origin) = header("origin")? {
            if origin != self.origin {
                return Err(403);
            }
        }
        if let Some(site) = header("sec-fetch-site")? {
            if !matches!(site, "same-origin" | "none") {
                return Err(403);
            }
        }
        if header("transfer-encoding")?.is_some()
            || header("content-length")?.is_some_and(|length| length != "0")
        {
            return Err(400);
        }
        Ok(())
    }
}

struct Metadata {
    loopback: bool,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}
impl Metadata {
    fn from_request(request: &Request<Incoming>, loopback: bool) -> Self {
        Self {
            loopback,
            method: request.method().as_str().into(),
            url: request.uri().to_string(),
            headers: request
                .headers()
                .iter()
                .map(|(name, value)| (name.as_str().into(), value.to_str().unwrap_or("\0").into()))
                .collect(),
        }
    }
    fn header(&self, name: &str) -> std::result::Result<Option<&str>, ()> {
        let mut matching = self
            .headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name));
        let value = matching.next().map(|(_, value)| value.as_str());
        if matching.next().is_some() || value.is_some_and(|v| v.chars().any(char::is_control)) {
            return Err(());
        }
        Ok(value)
    }
}

struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}
impl Reply {
    fn json(status: u16, value: &Value) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) if body.len() <= MAX_JSON => Self {
                status,
                content_type: "application/json; charset=utf-8",
                body,
            },
            _ => Self::error(503, "response_limit"),
        }
    }
    fn error(status: u16, code: &'static str) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            // Codes are closed static identifiers, never peer-provided text.
            body: format!("{{\"error\":{{\"code\":\"{code}\"}}}}").into_bytes(),
        }
    }
    fn asset(content_type: &'static str, body: &'static [u8]) -> Self {
        Self {
            status: 200,
            content_type,
            body: body.to_vec(),
        }
    }
    fn response(self) -> Result<Response<Full<Bytes>>> {
        let mut response = Response::builder().status(self.status);
        for (name, value) in [
            ("Content-Type", self.content_type),
            ("Cache-Control", "no-store"),
            ("Content-Security-Policy", CSP),
            ("X-Content-Type-Options", "nosniff"),
            ("Referrer-Policy", "no-referrer"),
            ("Cross-Origin-Resource-Policy", "same-origin"),
            ("X-Frame-Options", "DENY"),
            ("Connection", "close"),
        ] {
            response = response.header(name, value);
        }
        response
            .body(Full::new(Bytes::from(self.body)))
            .map_err(|_| Error("http_header_error"))
    }
}

struct OutputSlot<'a>(&'a AtomicUsize);
impl<'a> OutputSlot<'a> {
    fn acquire(slots: &'a AtomicUsize) -> Option<Self> {
        let mut count = slots.load(Ordering::Relaxed);
        loop {
            if count >= 2 {
                return None;
            }
            match slots.compare_exchange_weak(
                count,
                count + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(Self(slots)),
                Err(current) => count = current,
            }
        }
    }
}
impl Drop for OutputSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

fn route(api: &dyn Api, policy: &Policy, slots: &AtomicUsize, meta: &Metadata) -> Reply {
    if let Err(status) = policy.authorize(meta) {
        return Reply::error(
            status,
            if status == 403 {
                "access_denied"
            } else {
                "invalid_request"
            },
        );
    }
    if meta.method != "GET" {
        return Reply::error(405, "method_not_allowed");
    }
    match meta.url.as_str() {
        "/" => Reply::asset(
            "text/html; charset=utf-8",
            include_bytes!("../../clients/web/index.html"),
        ),
        "/app.js" => Reply::asset(
            "text/javascript; charset=utf-8",
            include_bytes!("../../clients/web/app.js"),
        ),
        "/styles.css" => Reply::asset(
            "text/css; charset=utf-8",
            include_bytes!("../../clients/web/styles.css"),
        ),
        "/api/v1/health" => Reply::json(
            200,
            &json!({"contract":"coven.fleet-observer.v1","status":"ready"}),
        ),
        "/api/v1/fleet" => Reply::json(200, &api.fleet()),
        path => {
            let Some(id) = path
                .strip_prefix("/api/v1/agents/")
                .and_then(|s| s.strip_suffix("/output"))
            else {
                return Reply::error(404, "not_found");
            };
            if id.len() != 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Reply::error(400, "invalid_reference");
            }
            let Some(_slot) = OutputSlot::acquire(slots) else {
                return Reply::error(429, "observer_busy");
            };
            match api.output(id) {
                Ok(value) => Reply::json(200, &value),
                Err(error) => Reply::error(
                    match error.code() {
                        "invalid_reference" => 404,
                        "output_disabled" | "output_unsupported" => 403,
                        "stale_reference" | "terminal_replaced" => 409,
                        "endpoint_busy" => 429,
                        _ => 503,
                    },
                    match error.code() {
                        "invalid_reference" => "invalid_reference",
                        "output_disabled" => "output_disabled",
                        "output_unsupported" => "output_unsupported",
                        "stale_reference" => "stale_reference",
                        "terminal_replaced" => "terminal_replaced",
                        "endpoint_busy" => "endpoint_busy",
                        "timeout" => "timeout",
                        "response_limit" => "response_limit",
                        _ => "upstream_unavailable",
                    },
                ),
            }
        }
    }
}

/// Serve identity headers are trusted only from loopback. Other local processes
/// remain within the explicitly trusted host boundary; this is not public hosting.
pub fn serve(config: Config) -> Result<()> {
    config.validate()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Error("observer_start_failed"))?;
    let policy = Arc::new(Policy::from_config(&config));
    let observer = Arc::new(Observer::start(&config)?);
    let slots = Arc::new(AtomicUsize::new(0));
    let outputs = Arc::new(Semaphore::new(2));
    let connections = Arc::new(Semaphore::new(64));
    let interrupt = crate::observation_socket::InterruptGuard::install()?;
    let result = runtime.block_on(async {
        let listener = TcpListener::bind(config.listen)
            .await
            .map_err(|_| Error("observer_bind_failed"))?;
        let mut tasks = JoinSet::new();
        let mut result = Ok(());
        // The !Send signal guard is sampled only on the owning root thread.
        while !interrupt.interrupted() {
            while tasks.try_join_next().is_some() {}
            let (stream, peer) = match timeout(Duration::from_millis(100), listener.accept()).await
            {
                Ok(Ok(connection)) => connection,
                Err(_) => continue,
                Ok(Err(_)) => {
                    result = Err(Error("observer_accept_failed"));
                    break;
                }
            };
            let Ok(permit) = connections.clone().try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let (api, policy, slots, outputs) = (
                observer.clone(),
                policy.clone(),
                slots.clone(),
                outputs.clone(),
            );
            tasks.spawn(async move {
                let _permit = permit;
                connection(stream, peer.ip().is_loopback(), api, policy, slots, outputs).await;
            });
        }
        drop(listener);
        let _ = timeout(Duration::from_secs(10), async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        result
    });
    runtime.shutdown_timeout(Duration::from_secs(6));
    drop(observer);
    result
}

async fn connection(
    stream: TcpStream,
    loopback: bool,
    api: Arc<dyn Api>,
    policy: Arc<Policy>,
    slots: Arc<AtomicUsize>,
    outputs: Arc<Semaphore>,
) {
    let service = service_fn(move |request: Request<Incoming>| {
        let meta = Metadata::from_request(&request, loopback);
        // Never poll, collect or drain the peer's body, including on rejection.
        drop(request);
        let (api, policy, slots, outputs) =
            (api.clone(), policy.clone(), slots.clone(), outputs.clone());
        async move {
            dispatch(api, policy, slots, outputs, meta)
                .await
                .response()
                .map_err(|_| std::io::Error::other("http_header_error"))
        }
    });
    let mut builder = http1::Builder::new();
    builder
        .keep_alive(false)
        .max_headers(64)
        .max_buf_size(32768)
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(5));
    // Dropping the connection at the deadline closes its socket without draining.
    let _ = timeout(
        Duration::from_secs(10),
        builder.serve_connection(TokioIo::new(stream), service),
    )
    .await;
}

async fn dispatch(
    api: Arc<dyn Api>,
    policy: Arc<Policy>,
    slots: Arc<AtomicUsize>,
    outputs: Arc<Semaphore>,
    meta: Metadata,
) -> Reply {
    let output = meta.method == "GET"
        && policy.authorize(&meta).is_ok()
        && meta
            .url
            .strip_prefix("/api/v1/agents/")
            .and_then(|s| s.strip_suffix("/output"))
            .is_some_and(|id| {
                id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            });
    if !output {
        return route(api.as_ref(), &policy, &slots, &meta);
    }
    let Ok(permit) = outputs.try_acquire_owned() else {
        return Reply::error(429, "observer_busy");
    };
    tokio::task::spawn_blocking(move || {
        // Cancellation of the HTTP future must not release job capacity.
        let _permit = permit;
        route(api.as_ref(), &policy, &slots, &meta)
    })
    .await
    .unwrap_or_else(|_| Reply::error(503, "upstream_unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(AtomicUsize);
    impl Api for Fake {
        fn fleet(&self) -> Value {
            self.0.fetch_add(1, Ordering::Relaxed);
            json!({"endpoints":[]})
        }
        fn output(&self, _: &str) -> Result<Value> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(Error("output_disabled"))
        }
    }
    fn policy() -> Policy {
        Policy {
            authority: "host.example.ts.net".into(),
            origin: "https://host.example.ts.net".into(),
            logins: vec!["owner@example.invalid".into()],
        }
    }
    fn meta(path: &str) -> Metadata {
        Metadata {
            loopback: true,
            method: "GET".into(),
            url: path.into(),
            headers: vec![
                ("Host".into(), "host.example.ts.net".into()),
                (
                    "Tailscale-User-Login".into(),
                    "owner@example.invalid".into(),
                ),
                ("Sec-Fetch-Site".into(), "same-origin".into()),
            ],
        }
    }
    fn status(request: &Metadata) -> u16 {
        route(
            &Fake(AtomicUsize::new(0)),
            &policy(),
            &AtomicUsize::new(0),
            request,
        )
        .status
    }
    #[test]
    fn requires_proxy_identity_exact_origin_host_and_loopback() {
        let mut request = meta("/api/v1/fleet");
        assert_eq!(status(&request), 200);
        request.loopback = false;
        assert_eq!(status(&request), 403);
        request.loopback = true;
        request.headers.retain(|(k, _)| k != "Tailscale-User-Login");
        assert_eq!(status(&request), 403);
        for (header, value) in [
            ("Origin", "https://evil.invalid"),
            ("Sec-Fetch-Site", "cross-site"),
            ("Host", "evil.invalid"),
            ("Tailscale-User-Login", "other@example.invalid"),
        ] {
            let mut request = meta("/api/v1/fleet");
            request
                .headers
                .retain(|(k, _)| !k.eq_ignore_ascii_case(header));
            request.headers.push((header.into(), value.into()));
            assert_eq!(status(&request), 403);
        }
    }
    #[test]
    fn duplicate_headers_and_bodies_cannot_bypass_checks() {
        for header in ["Host", "Tailscale-User-Login", "Origin", "Sec-Fetch-Site"] {
            let mut request = meta("/");
            request
                .headers
                .push((header.into(), "https://host.example.ts.net".into()));
            request
                .headers
                .push((header.to_lowercase(), "https://evil.invalid".into()));
            assert_ne!(status(&request), 200);
        }
        for (name, value) in [
            ("Content-Length", "1"),
            ("Content-Length", "bad"),
            ("Transfer-Encoding", "chunked"),
        ] {
            let mut request = meta("/");
            request.headers.push((name.into(), value.into()));
            assert_eq!(status(&request), 400);
        }
    }
    #[test]
    fn unauthorized_requests_never_query_sources() {
        let api = Fake(AtomicUsize::new(0));
        let mut request = meta("/api/v1/fleet");
        request.headers.clear();
        assert_eq!(
            route(&api, &policy(), &AtomicUsize::new(0), &request).status,
            403
        );
        assert_eq!(api.0.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn surface_is_closed_read_only_and_bounded() {
        for path in [
            "/api/rpc",
            "/../Cargo.toml",
            "/%2e%2e/Cargo.toml",
            "/api/v1/fleet?socket=/tmp/other",
            "/unknown",
        ] {
            assert_eq!(status(&meta(path)), 404);
        }
        for method in ["POST", "PUT", "DELETE", "OPTIONS", "HEAD"] {
            let mut request = meta("/api/v1/fleet");
            request.method = method.into();
            assert_eq!(status(&request), 405);
        }
        assert_eq!(status(&meta("/api/v1/agents/../output")), 400);
        let request = meta(&format!("/api/v1/agents/{}/output", "a".repeat(64)));
        assert_eq!(status(&request), 403);
        assert_eq!(
            route(
                &Fake(AtomicUsize::new(0)),
                &policy(),
                &AtomicUsize::new(2),
                &request
            )
            .status,
            429
        );
        let mut request = meta("/");
        request.url = "x".repeat(2049);
        assert_eq!(status(&request), 431);
    }
    #[test]
    fn every_response_is_inert_uncached_and_same_origin() {
        for path in ["/", "/app.js", "/styles.css", "/api/v1/fleet", "/unknown"] {
            let reply = route(
                &Fake(AtomicUsize::new(0)),
                &policy(),
                &AtomicUsize::new(0),
                &meta(path),
            );
            let response = reply.response().expect("static headers");
            let header = |name: &str| response.headers().get(name).and_then(|h| h.to_str().ok());
            assert_eq!(header("Cache-Control"), Some("no-store"));
            assert_eq!(header("Cross-Origin-Resource-Policy"), Some("same-origin"));
            assert_eq!(header("Content-Security-Policy"), Some(CSP));
            assert_eq!(header("Access-Control-Allow-Origin"), None);
        }
    }

    #[test]
    fn canceled_output_futures_keep_job_permits_and_fleet_stays_responsive() {
        struct Blocking {
            entered: std::sync::mpsc::Sender<()>,
            release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        }
        impl Api for Blocking {
            fn fleet(&self) -> Value {
                json!({"endpoints":[]})
            }
            fn output(&self, _: &str) -> Result<Value> {
                self.entered.send(()).expect("job entered");
                let (lock, wake) = &*self.release;
                let guard = lock.lock().expect("lock");
                // Bound the fixture even when the assertion path fails.
                let _ = wake
                    .wait_timeout_while(guard, Duration::from_secs(3), |ready| !*ready)
                    .expect("wait");
                Err(Error("output_disabled"))
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let (entered, receive) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let api: Arc<dyn Api> = Arc::new(Blocking {
            entered,
            release: release.clone(),
        });
        let outputs = Arc::new(Semaphore::new(2));
        let slots = Arc::new(AtomicUsize::new(0));
        let policy = Arc::new(policy());
        let path = format!("/api/v1/agents/{}/output", "a".repeat(64));
        runtime.block_on(async {
            for _ in 0..2 {
                let task = tokio::spawn(dispatch(
                    api.clone(),
                    policy.clone(),
                    slots.clone(),
                    outputs.clone(),
                    meta(&path),
                ));
                receive
                    .recv_timeout(Duration::from_secs(1))
                    .expect("blocking job starts");
                task.abort();
                assert!(task.await.is_err_and(|error| error.is_cancelled()));
            }
            assert_eq!(outputs.available_permits(), 0);
            let busy = dispatch(
                api.clone(),
                policy.clone(),
                slots.clone(),
                outputs.clone(),
                meta(&path),
            )
            .await;
            assert_eq!(busy.status, 429);
            assert_eq!(slots.load(Ordering::Relaxed), 2);
            let fleet = timeout(
                Duration::from_millis(250),
                dispatch(
                    api.clone(),
                    policy.clone(),
                    slots.clone(),
                    outputs.clone(),
                    meta("/api/v1/fleet"),
                ),
            )
            .await
            .expect("cached fleet not blocked");
            assert_eq!(fleet.status, 200);
            *release.0.lock().expect("release lock") = true;
            release.1.notify_all();
            timeout(Duration::from_secs(1), async {
                while outputs.available_permits() != 2 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("completed jobs return permits");
            assert_eq!(slots.load(Ordering::Relaxed), 0);
        });
    }
}
