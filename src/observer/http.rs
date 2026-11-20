//! Same-origin loopback HTTP facade. Serve is trusted; local processes are trusted too.
use super::push::PushService;
use super::{Config, Observer};
use crate::{Error, Result};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};

const MAX_JSON: usize = 2 * 1024 * 1024;
const MAX_BODY: usize = 400 * 1024;
const CSP: &str = "default-src 'none'; script-src 'self'; worker-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'";

trait Api: Send + Sync {
    fn attention(&self, _login: &str) -> Value {
        json!({"contract":"coven.attention.v1","state":"unavailable","items":[],"omitted_count":null,"unavailable_count":1,"unlinked_count":0})
    }
    fn decision_details(&self, _login: &str, _id: &str) -> Result<Value> {
        Err(Error("decision_unavailable"))
    }
    fn decision_response_allowed(&self, _login: &str, _id: &str) -> bool {
        false
    }
    fn respond_decision(
        &self,
        _login: &str,
        _id: &str,
        _attempt: &str,
        _action: &Value,
    ) -> Result<Value> {
        Err(Error("decision_unavailable"))
    }
    fn push_status(&self, _login: &str) -> Value {
        json!({"contract":"coven.web-push.v1","enabled":false,"public_key":null,"subscriptions":[]})
    }
    fn push_allowed(&self, _login: &str) -> bool {
        false
    }
    fn push_subscribe(&self, _login: &str, _subscription: &Value) -> Result<Value> {
        Err(Error("push_unavailable"))
    }
    fn push_unsubscribe(&self, _login: &str, _id: &str) -> Result<Value> {
        Err(Error("push_unavailable"))
    }
    fn conversations(&self, _login: &str) -> Value {
        json!({"contract":"coven.conversations.v1","conversations":[]})
    }
    fn conversation(&self, _login: &str, _id: &str) -> Result<Value> {
        Err(Error("invalid_reference"))
    }
    fn linked_conversation(
        &self,
        _login: &str,
        _agent: &str,
        _conversation: &str,
        _binding: &str,
    ) -> Result<Value> {
        Err(Error("invalid_reference"))
    }
    fn submission_allowed(&self, _login: &str, _id: &str) -> bool {
        false
    }
    fn submit(&self, _login: &str, _id: &str, _request_id: &str, _text: &str) -> Result<Value> {
        Err(Error("submission_disabled"))
    }
    fn fleet(&self) -> Value;
    fn output(&self, id: &str) -> Result<Value>;
}
impl Api for Observer {
    fn attention(&self, login: &str) -> Value {
        Observer::attention(self, login)
    }
    fn decision_details(&self, login: &str, id: &str) -> Result<Value> {
        Observer::decision_details(self, login, id)
    }
    fn decision_response_allowed(&self, login: &str, id: &str) -> bool {
        Observer::decision_response_allowed(self, login, id)
    }
    fn respond_decision(
        &self,
        login: &str,
        id: &str,
        attempt: &str,
        action: &Value,
    ) -> Result<Value> {
        Observer::respond_decision(self, login, id, attempt, action)
    }
    fn conversations(&self, login: &str) -> Value {
        self.conversation_list(login)
    }
    fn conversation(&self, login: &str, id: &str) -> Result<Value> {
        self.conversations.read(login, id)
    }
    fn linked_conversation(
        &self,
        login: &str,
        agent: &str,
        conversation: &str,
        binding: &str,
    ) -> Result<Value> {
        Observer::linked_conversation(self, login, agent, conversation, binding)
    }
    fn submission_allowed(&self, login: &str, id: &str) -> bool {
        self.conversations.submission_allowed(login, id)
    }
    fn submit(&self, login: &str, id: &str, request_id: &str, text: &str) -> Result<Value> {
        self.conversations.submit(login, id, request_id, text)
    }
    fn fleet(&self) -> Value {
        Observer::fleet(self)
    }
    fn output(&self, id: &str) -> Result<Value> {
        Observer::output(self, id)
    }
}

struct WebApi {
    observer: Arc<Observer>,
    push: Option<Arc<PushService>>,
}
impl WebApi {
    fn start(config: &Config, observer: Arc<Observer>) -> Result<Self> {
        let weak = Arc::downgrade(&observer);
        let push = PushService::start(
            config,
            Arc::new(move |login| {
                weak.upgrade().map(|o| o.attention(login)).unwrap_or_else(|| json!({"contract":"coven.attention.v1","state":"unavailable","items":[],"omitted_count":null,"unavailable_count":1,"unlinked_count":0}))
            }),
        )?;
        Ok(Self { observer, push })
    }
}
impl Api for WebApi {
    fn attention(&self, login: &str) -> Value {
        self.observer.attention(login)
    }
    fn decision_details(&self, login: &str, id: &str) -> Result<Value> {
        self.observer.decision_details(login, id)
    }
    fn decision_response_allowed(&self, login: &str, id: &str) -> bool {
        self.observer.decision_response_allowed(login, id)
    }
    fn respond_decision(
        &self,
        login: &str,
        id: &str,
        attempt: &str,
        action: &Value,
    ) -> Result<Value> {
        self.observer.respond_decision(login, id, attempt, action)
    }
    fn push_status(&self, login: &str) -> Value {
        self.push
            .as_ref()
            .map(|p| p.status(login))
            .unwrap_or_else(|| self.observer.push_status(login))
    }
    fn push_allowed(&self, login: &str) -> bool {
        self.push
            .as_ref()
            .is_some_and(|p| p.registration_allowed(login))
    }
    fn push_subscribe(&self, login: &str, subscription: &Value) -> Result<Value> {
        self.push
            .as_ref()
            .ok_or(Error("push_unavailable"))?
            .subscribe(login, subscription)
    }
    fn push_unsubscribe(&self, login: &str, id: &str) -> Result<Value> {
        self.push
            .as_ref()
            .ok_or(Error("push_unavailable"))?
            .unsubscribe(login, id)
    }
    fn conversations(&self, login: &str) -> Value {
        self.observer.conversation_list(login)
    }
    fn conversation(&self, login: &str, id: &str) -> Result<Value> {
        self.observer.conversation(login, id)
    }
    fn linked_conversation(
        &self,
        login: &str,
        agent: &str,
        conversation: &str,
        binding: &str,
    ) -> Result<Value> {
        self.observer
            .linked_conversation(login, agent, conversation, binding)
    }
    fn submission_allowed(&self, login: &str, id: &str) -> bool {
        self.observer.submission_allowed(login, id)
    }
    fn submit(&self, login: &str, id: &str, request: &str, text: &str) -> Result<Value> {
        self.observer.submit(login, id, request, text)
    }
    fn fleet(&self) -> Value {
        self.observer.fleet()
    }
    fn output(&self, id: &str) -> Result<Value> {
        self.observer.output(id)
    }
}

struct Policy {
    authority: String,
    origin: String,
    logins: Vec<String>,
    csrf: Csrf,
}

struct Csrf {
    key: hmac::Key,
    started: Instant,
}
impl Csrf {
    fn new() -> Result<Self> {
        Ok(Self {
            key: hmac::Key::generate(hmac::HMAC_SHA256, &SystemRandom::new())
                .map_err(|_| Error("csrf_unavailable"))?,
            started: Instant::now(),
        })
    }
    fn issue(&self, login: &str) -> Result<String> {
        let mut nonce = [0u8; 32];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error("csrf_unavailable"))?;
        let payload = format!("{}-{}", self.started.elapsed().as_secs(), hex(&nonce));
        let tag = hmac::sign(&self.key, format!("{login}\0{payload}").as_bytes());
        Ok(format!("{payload}-{}", hex(tag.as_ref())))
    }
    fn verify(&self, login: &str, token: &str) -> bool {
        let Some((payload, tag)) = token.rsplit_once('-') else {
            return false;
        };
        let Some((seconds, nonce)) = payload.split_once('-') else {
            return false;
        };
        let Ok(seconds) = seconds.parse::<u64>() else {
            return false;
        };
        let now = self.started.elapsed().as_secs();
        if seconds > now
            || now - seconds >= 60
            || nonce.len() != 64
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return false;
        }
        let Some(tag) = unhex(tag) else { return false };
        hmac::verify(&self.key, format!("{login}\0{payload}").as_bytes(), &tag).is_ok()
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |b| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
impl Policy {
    fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            authority: config.public_origin["https://".len()..].to_owned(),
            origin: config.public_origin.clone(),
            logins: config.allowed_logins.clone(),
            csrf: Csrf::new()?,
        })
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
    if meta.header("transfer-encoding") != Ok(None)
        || meta.header("content-encoding") != Ok(None)
        || !matches!(meta.header("content-length"), Ok(None | Some("0")))
    {
        return Reply::error(400, "invalid_request");
    }
    if meta.method != "GET" {
        return Reply::error(405, "method_not_allowed");
    }
    // authorize() established exactly one trusted Serve principal above.
    let login = meta
        .header("tailscale-user-login")
        .ok()
        .flatten()
        .unwrap_or("");
    if meta.url == "/api/v1/csrf" {
        return match policy.csrf.issue(login) {
            Ok(token) => Reply::json(200, &json!({"contract":"coven.csrf.v1","token":token})),
            Err(_) => Reply::error(503, "csrf_unavailable"),
        };
    }
    if meta.url == "/api/v1/conversations" {
        return Reply::json(200, &api.conversations(login));
    }
    if meta.url == "/api/v1/attention" {
        return Reply::json(200, &api.attention(login));
    }
    if meta.url == "/api/v1/push/config" {
        return Reply::json(200, &api.push_status(login));
    }
    if let Some(id) = meta.url.strip_prefix("/api/v1/decisions/") {
        if !super::valid_id(id) {
            return Reply::error(400, "invalid_reference");
        }
        let Some(_slot) = OutputSlot::acquire(slots) else {
            return Reply::error(429, "observer_busy");
        };
        return match api.decision_details(login, id) {
            Ok(value) => Reply::json(200, &value),
            Err(_) => Reply::error(409, "decision_unavailable"),
        };
    }
    if let Some((agent, conversation, binding)) = linked_route(&meta.url) {
        let Some(_slot) = OutputSlot::acquire(slots) else {
            return Reply::error(429, "observer_busy");
        };
        return match api.linked_conversation(login, agent, conversation, binding) {
            Ok(value) => Reply::json(200, &value),
            Err(e) if e.code() == "invalid_reference" => Reply::error(404, "invalid_reference"),
            Err(_) => Reply::error(409, "stale_reference"),
        };
    }
    if let Some(id) = meta
        .url
        .strip_prefix("/api/v1/conversations/")
        .and_then(|s| s.strip_suffix("/messages"))
    {
        if !super::valid_id(id) {
            return Reply::error(400, "invalid_reference");
        }
        let Some(_slot) = OutputSlot::acquire(slots) else {
            return Reply::error(429, "observer_busy");
        };
        return match api.conversation(login, id) {
            Ok(value) => Reply::json(200, &value),
            Err(e) => Reply::error(
                if e.code() == "invalid_reference" {
                    404
                } else {
                    503
                },
                match e.code() {
                    "invalid_reference" => "invalid_reference",
                    "runtime_mismatch" => "runtime_mismatch",
                    "session_unavailable" => "session_unavailable",
                    "conversation_unsupported" => "conversation_unsupported",
                    "request_unavailable" => "request_unavailable",
                    "timeout" => "timeout",
                    "response_limit" => "response_limit",
                    _ => "upstream_unavailable",
                },
            ),
        };
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
        "/service-worker.js" => Reply::asset(
            "text/javascript; charset=utf-8",
            include_bytes!("../../clients/web/service-worker.js"),
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
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .map_err(|_| Error("observer_start_failed"))?;
    let policy = Arc::new(Policy::from_config(&config)?);
    let observer = Arc::new(Observer::start(&config)?);
    let web_api = Arc::new(WebApi::start(&config, observer.clone())?);
    let slots = Arc::new(AtomicUsize::new(0));
    let outputs = Arc::new(Semaphore::new(2));
    let mutations = Arc::new(Semaphore::new(2));
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
            let (api, policy, slots, outputs, mutations) = (
                web_api.clone(),
                policy.clone(),
                slots.clone(),
                outputs.clone(),
                mutations.clone(),
            );
            tasks.spawn(async move {
                let _permit = permit;
                connection(
                    stream,
                    peer.ip().is_loopback(),
                    api,
                    policy,
                    slots,
                    outputs,
                    mutations,
                )
                .await;
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
    drop(web_api);
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
    mutations: Arc<Semaphore>,
) {
    let started = Instant::now();
    // Hyper coalesces identical Content-Length fields. Preserve the closed
    // framing rule before its parser normalizes them, without consuming bytes.
    let invalid_length = match timeout(Duration::from_secs(5), async {
        let mut headers = vec![0u8; 32768];
        loop {
            let length = stream.peek(&mut headers).await.ok()?;
            if length == 0 {
                return None;
            }
            if let Some(end) = headers[..length].windows(4).position(|w| w == b"\r\n\r\n") {
                let lengths: Vec<_> = headers[..end]
                    .split(|b| *b == b'\n')
                    .skip(1)
                    .filter(|line| {
                        line.split(|b| *b == b':')
                            .next()
                            .is_some_and(|name| name.eq_ignore_ascii_case(b"content-length"))
                    })
                    .collect();
                let invalid = lengths.iter().any(|line| {
                    let value = line.splitn(2, |b| *b == b':').nth(1).unwrap_or_default();
                    let value = value.trim_ascii();
                    value.is_empty() || !value.iter().all(u8::is_ascii_digit)
                });
                return Some(lengths.len() > 1 || invalid);
            }
            if length == headers.len() {
                return None;
            }
            // A peek of an incomplete header remains readable; avoid spinning.
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    {
        Ok(Some(value)) => value,
        _ => return,
    };
    let service = service_fn(move |request: Request<Incoming>| {
        let meta = Metadata::from_request(&request, loopback);
        let (api, policy, slots, outputs, mutations) = (
            api.clone(),
            policy.clone(),
            slots.clone(),
            outputs.clone(),
            mutations.clone(),
        );
        async move {
            let reply = if invalid_length {
                drop(request);
                Reply::error(400, "invalid_request")
            } else if meta.method == "POST" {
                submit_request(api, policy, mutations, meta, request.into_body()).await
            } else {
                // Refused GET bodies are never polled or drained.
                drop(request);
                dispatch(api, policy, slots, outputs, meta).await
            };
            reply
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
        Duration::from_secs(20).saturating_sub(started.elapsed()),
        builder.serve_connection(TokioIo::new(stream), service),
    )
    .await;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptBody {
    contract: String,
    request_id: String,
    execute: bool,
    text: String,
}
fn parse_prompt(body: &[u8]) -> Option<PromptBody> {
    let prompt: PromptBody = serde_json::from_str(std::str::from_utf8(body).ok()?).ok()?;
    if prompt.contract != "coven.conversation-submit.v1"
        || !prompt.execute
        || !crate::session_prompts::protocol::uuid(&prompt.request_id)
        || prompt.text.len() > 65536
        || prompt.text.trim().is_empty()
        || prompt
            .text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return None;
    }
    Some(prompt)
}

#[derive(Deserialize, serde::Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum DecisionAction {
    #[serde(rename = "question_answer")]
    Answer { answers: Vec<Vec<String>> },
    #[serde(rename = "question_reject")]
    Reject {},
    #[serde(rename = "permission_once")]
    Once {},
    #[serde(rename = "permission_deny")]
    Deny { scope: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionBody {
    contract: String,
    attempt_id: String,
    execute: bool,
    action: DecisionAction,
}
fn parse_decision(bytes: &[u8]) -> Option<DecisionBody> {
    let body: DecisionBody = serde_json::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    if body.contract != "coven.decision-response.v1"
        || !body.execute
        || !crate::session_prompts::protocol::uuid(&body.attempt_id)
    {
        return None;
    }
    match &body.action {
        DecisionAction::Deny { scope } if scope != "session_pending" => return None,
        DecisionAction::Answer { answers } => {
            if answers.is_empty() || answers.len() > 8 {
                return None;
            }
            let mut total = 0;
            for row in answers {
                if row.len() > 17 {
                    return None;
                }
                let mut seen = std::collections::HashSet::new();
                for answer in row {
                    total += answer.len();
                    if answer.trim().is_empty()
                        || answer.len() > 4096
                        || !seen.insert(answer)
                        || answer
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
                    {
                        return None;
                    }
                }
            }
            if total > 8192 {
                return None;
            }
        }
        _ => {}
    }
    Some(body)
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct PushKeys {
    p256dh: String,
    auth: String,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Subscription {
    endpoint: String,
    keys: PushKeys,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscribeBody {
    contract: String,
    execute: bool,
    subscription: Subscription,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnsubscribeBody {
    contract: String,
    execute: bool,
    subscription_id: String,
}

async fn extended_mutation(
    api: Arc<dyn Api>,
    permit: tokio::sync::OwnedSemaphorePermit,
    login: String,
    id: String,
    path: &str,
    bytes: &[u8],
) -> Reply {
    if path.starts_with("/api/v1/decisions/") {
        let Some(body) = parse_decision(bytes) else {
            return Reply::error(400, "invalid_decision");
        };
        let Ok(action) = serde_json::to_value(body.action) else {
            return Reply::error(400, "invalid_decision");
        };
        let uncertain = json!({"contract":"coven.decision-response.v1","id":id,"attempt_id":body.attempt_id,
            "outcome":"uncertain","delivery":"uncertain","action":action["type"],"diagnostics":["decision_uncertain"]});
        let fallback = uncertain.clone();
        return tokio::task::spawn_blocking(move || {
            let _permit = permit;
            match api.respond_decision(&login, &id, &body.attempt_id, &action) {
                Ok(value) => Reply::json(200, &value),
                Err(_) => Reply::json(200, &uncertain),
            }
        })
        .await
        .unwrap_or_else(|_| Reply::json(200, &fallback));
    }
    let Some(text) = std::str::from_utf8(bytes).ok() else {
        return Reply::error(400, "invalid_request");
    };
    let subscription = if path == "/api/v1/push/subscriptions" {
        let Ok(body) = serde_json::from_str::<SubscribeBody>(text) else {
            return Reply::error(400, "invalid_request");
        };
        if body.contract != "coven.web-push.v1" || !body.execute {
            return Reply::error(400, "invalid_request");
        }
        let Ok(value) = serde_json::to_value(body.subscription) else {
            return Reply::error(400, "invalid_request");
        };
        Ok(value)
    } else {
        let Ok(body) = serde_json::from_str::<UnsubscribeBody>(text) else {
            return Reply::error(400, "invalid_request");
        };
        if body.contract != "coven.web-push.v1"
            || !body.execute
            || !super::valid_id(&body.subscription_id)
        {
            return Reply::error(400, "invalid_request");
        }
        Err(body.subscription_id)
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let result = match subscription {
            Ok(value) => api.push_subscribe(&login, &value),
            Err(id) => api.push_unsubscribe(&login, &id),
        };
        match result {
            Ok(value) => Reply::json(200, &value),
            Err(_) => Reply::error(409, "push_unavailable"),
        }
    })
    .await
    .unwrap_or_else(|_| Reply::error(503, "push_unavailable"))
}

// Cache/metadata only: no native I/O and no body polling on refusal.
fn submit_preflight<'a>(
    api: &dyn Api,
    policy: &Policy,
    meta: &'a Metadata,
) -> std::result::Result<(&'a str, &'a str, usize), Reply> {
    policy
        .authorize(meta)
        .map_err(|status| Reply::error(status, "access_denied"))?;
    let (id, max_body) = if let Some(id) = meta
        .url
        .strip_prefix("/api/v1/decisions/")
        .and_then(|s| s.strip_suffix("/responses"))
    {
        (id, 65536)
    } else if matches!(
        meta.url.as_str(),
        "/api/v1/push/subscriptions" | "/api/v1/push/unsubscribe"
    ) {
        ("", 8192)
    } else {
        (
            meta.url
                .strip_prefix("/api/v1/conversations/")
                .and_then(|s| s.strip_suffix("/prompts"))
                .ok_or_else(|| Reply::error(405, "method_not_allowed"))?,
            MAX_BODY,
        )
    };
    if !meta.url.starts_with("/api/v1/push/") && !super::valid_id(id) {
        return Err(Reply::error(400, "invalid_reference"));
    }
    let login = meta
        .header("tailscale-user-login")
        .ok()
        .flatten()
        .unwrap_or("");
    let allowed = if meta.url.starts_with("/api/v1/decisions/") {
        api.decision_response_allowed(login, id)
    } else if meta.url.starts_with("/api/v1/push/") {
        api.push_allowed(login)
    } else {
        api.submission_allowed(login, id)
    };
    if !allowed {
        return Err(Reply::error(403, "submission_disabled"));
    }
    if meta.header("origin") != Ok(Some(policy.origin.as_str()))
        || !matches!(
            meta.header("sec-fetch-site"),
            Ok(None | Some("same-origin"))
        )
        || !matches!(
            meta.header("sec-fetch-mode"),
            Ok(None | Some("cors" | "same-origin"))
        )
        || !matches!(meta.header("sec-fetch-dest"), Ok(None | Some("empty")))
    {
        return Err(Reply::error(403, "access_denied"));
    }
    if !meta
        .header("x-coven-csrf")
        .ok()
        .flatten()
        .is_some_and(|token| policy.csrf.verify(login, token))
    {
        return Err(Reply::error(403, "invalid_csrf"));
    }
    if !matches!(
        meta.header("content-type"),
        Ok(Some("application/json" | "application/json; charset=utf-8"))
    ) || meta.header("transfer-encoding") != Ok(None)
        || meta.header("content-encoding") != Ok(None)
    {
        return Err(Reply::error(400, "invalid_request"));
    }
    let length = meta
        .header("content-length")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0 && *n <= max_body)
        .ok_or_else(|| Reply::error(400, "invalid_request"))?;
    Ok((login, id, length))
}

async fn submit_request<B>(
    api: Arc<dyn Api>,
    policy: Arc<Policy>,
    mutations: Arc<Semaphore>,
    meta: Metadata,
    mut body: B,
) -> Reply
where
    B: hyper::body::Body<Data = Bytes> + Unpin,
{
    let (login, id, length) = match submit_preflight(api.as_ref(), &policy, &meta) {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let Ok(permit) = mutations.try_acquire_owned() else {
        return Reply::error(429, "observer_busy");
    };
    let collected = timeout(Duration::from_secs(2), async {
        let mut bytes = Vec::with_capacity(length);
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| ())?;
            let data = frame.into_data().map_err(|_| ())?;
            if data.len() > length - bytes.len() {
                return Err(());
            }
            bytes.extend_from_slice(&data);
        }
        if bytes.len() != length {
            return Err(());
        }
        Ok(bytes)
    })
    .await;
    let bytes = match collected {
        Ok(Ok(bytes)) => bytes,
        Err(_) => return Reply::error(408, "body_timeout"),
        _ => return Reply::error(400, "invalid_request"),
    };
    if meta.url.starts_with("/api/v1/decisions/") || meta.url.starts_with("/api/v1/push/") {
        return extended_mutation(
            api,
            permit,
            login.to_owned(),
            id.to_owned(),
            &meta.url,
            &bytes,
        )
        .await;
    }
    let Some(prompt) = parse_prompt(&bytes) else {
        return Reply::error(400, "invalid_prompt");
    };
    let (login, id) = (login.to_owned(), id.to_owned());
    tokio::task::spawn_blocking(move || {
        // The blocking job, not the HTTP waiter, owns mutation capacity.
        let _permit = permit;
        match api.submit(&login, &id, &prompt.request_id, &prompt.text) {
            Ok(report) => Reply::json(200, &report),
            // Never describe a missing receipt as proof of non-delivery.
            Err(_) => Reply::error(503, "receipt_unavailable"),
        }
    })
    .await
    .unwrap_or_else(|_| Reply::error(503, "receipt_unavailable"))
}

fn linked_route(path: &str) -> Option<(&str, &str, &str)> {
    let mut parts = path.strip_prefix("/api/v1/agents/")?.split('/');
    let agent = parts.next()?;
    if parts.next()? != "conversations" {
        return None;
    }
    let conversation = parts.next()?;
    if parts.next()? != "bindings" {
        return None;
    }
    let binding = parts.next()?;
    if parts.next()? != "messages"
        || parts.next().is_some()
        || ![agent, conversation, binding]
            .iter()
            .all(|id| super::valid_id(id))
    {
        return None;
    }
    Some((agent, conversation, binding))
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
        && (matches!(
            meta.url.as_str(),
            "/api/v1/conversations" | "/api/v1/attention" | "/api/v1/push/config"
        ) || meta
            .url
            .strip_prefix("/api/v1/decisions/")
            .is_some_and(super::valid_id)
            || linked_route(&meta.url).is_some()
            || meta
                .url
                .strip_prefix("/api/v1/agents/")
                .and_then(|s| s.strip_suffix("/output"))
                .is_some_and(|id| {
                    id.len() == 64
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            || meta
                .url
                .strip_prefix("/api/v1/conversations/")
                .and_then(|s| s.strip_suffix("/messages"))
                .is_some_and(super::valid_id));
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
#[path = "http/prompt_tests.rs"]
mod prompt_tests;

#[cfg(test)]
#[path = "http/attention_tests.rs"]
mod attention_tests;

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Real HTTP framing/dispatch with a synthetic observer and no daemon/pollers.
    pub(in crate::observer) fn card_request(
        api: Arc<Observer>,
        path: &str,
        login: Option<&str>,
    ) -> (u16, Value) {
        use std::io::{Read, Write};
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let auth = login.map(|l| format!("Tailscale-User-Login: {l}\r\n")).unwrap_or_default();
            let request = format!("GET {path} HTTP/1.1\r\nHost: host.example.ts.net\r\n{auth}Connection: close\r\n\r\n");
            let client = tokio::task::spawn_blocking(move || {
                let mut stream = std::net::TcpStream::connect(addr).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                stream.write_all(request.as_bytes()).unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                response
            });
            let (stream, _) = listener.accept().await.unwrap();
            let mut policy = policy();
            policy.logins = vec!["alice".into(), "bob".into()];
            connection(stream, true, api, Arc::new(policy), Arc::new(AtomicUsize::new(0)),
                Arc::new(Semaphore::new(2)), Arc::new(Semaphore::new(2))).await;
            let response = client.await.unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(headers.to_ascii_lowercase().contains("cache-control: no-store"));
            let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
            (status, serde_json::from_str(body).unwrap())
        })
    }
    pub(super) struct Fake(pub(super) AtomicUsize);
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
    pub(super) fn policy() -> Policy {
        Policy {
            authority: "host.example.ts.net".into(),
            origin: "https://host.example.ts.net".into(),
            logins: vec!["owner@example.invalid".into()],
            csrf: Csrf::new().expect("fixture entropy"),
        }
    }
    pub(super) fn meta(path: &str) -> Metadata {
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
    fn exact_card_route_is_closed_authenticated_and_read_admitted() {
        struct Bound(AtomicUsize);
        impl Api for Bound {
            fn fleet(&self) -> Value {
                json!({})
            }
            fn output(&self, _: &str) -> Result<Value> {
                panic!("not output")
            }
            fn linked_conversation(
                &self,
                login: &str,
                agent: &str,
                conversation: &str,
                binding: &str,
            ) -> Result<Value> {
                assert_eq!(login, "owner@example.invalid");
                assert_eq!(
                    (agent, conversation, binding),
                    (
                        "a".repeat(64).as_str(),
                        "b".repeat(64).as_str(),
                        "c".repeat(64).as_str()
                    )
                );
                self.0.fetch_add(1, Ordering::Relaxed);
                Err(Error("stale_reference"))
            }
        }
        let api = Arc::new(Bound(AtomicUsize::new(0)));
        let path = format!(
            "/api/v1/agents/{}/conversations/{}/bindings/{}/messages",
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64)
        );
        let slots = AtomicUsize::new(0);
        let policy = policy();
        let mut request = meta(&path);
        request.headers.clear();
        assert_eq!(route(api.as_ref(), &policy, &slots, &request).status, 403);
        assert_eq!(api.0.load(Ordering::Relaxed), 0);
        for path in [
            format!("{path}?socket=/private"),
            path.replace("/bindings/", "/binding/"),
            path.replace(&"c".repeat(64), "current"),
        ] {
            assert!(linked_route(&path).is_none());
            assert_eq!(
                route(api.as_ref(), &policy, &slots, &meta(&path)).status,
                404
            );
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let reply = runtime.block_on(dispatch(
            api.clone(),
            Arc::new(policy),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(Semaphore::new(0)),
            meta(&path),
        ));
        assert_eq!(reply.status, 429);
        assert_eq!(api.0.load(Ordering::Relaxed), 0);
        assert_eq!(
            route(api.as_ref(), &self::policy(), &slots, &meta(&path)).status,
            409
        );
        assert_eq!(api.0.load(Ordering::Relaxed), 1);
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
    fn canceled_read_futures_keep_shared_job_permits_and_fleet_stays_responsive() {
        struct Blocking {
            entered: std::sync::mpsc::Sender<()>,
            release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        }
        impl Api for Blocking {
            fn conversation(&self, _: &str, id: &str) -> Result<Value> {
                self.output(id)
            }
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
            for route in [
                &path,
                &format!("/api/v1/conversations/{}/messages", "a".repeat(64)),
            ] {
                let task = tokio::spawn(dispatch(
                    api.clone(),
                    policy.clone(),
                    slots.clone(),
                    outputs.clone(),
                    meta(route),
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
