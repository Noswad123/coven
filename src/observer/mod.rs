//! Memory-only fleet observation and explicitly gated shared conversations.
//! Construction never attaches or starts agents.
mod attention;
mod cardlinks;
pub mod config;
mod conversations;
mod discovery;
pub mod http;
mod model;
pub(super) mod push;
mod source;
mod transport;

use crate::{Error, Result};
pub use config::Config;
use config::EndpointConfig;
use model::{CONTRACT, Inventory, Output, digest, now_ms};
use serde_json::{Value, json};
use source::Request;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use transport::{NativeTransport, Transport};

const INTERVAL: Duration = Duration::from_secs(3);
const STALE: Duration = Duration::from_secs(15);
const OPERATION: Duration = Duration::from_secs(5);
const BUSY_RETRY: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Cache {
    inventory: Option<Inventory>,
    ids: Vec<String>,
    generation: u64,
    fresh: Option<Instant>,
    last_attempt: Option<u64>,
    observed: Option<u64>,
    error: Option<&'static str>,
}

struct Endpoint {
    config: EndpointConfig,
    cache: Mutex<Cache>,
    busy: AtomicBool,
    refresh_due: AtomicBool,
}

struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Endpoint {
    fn acquire(&self) -> Result<Busy<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| Error("endpoint_busy"))?;
        Ok(Busy(&self.busy))
    }
    fn fail(&self, code: &'static str) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        cache.fresh = None;
        cache.generation = cache.generation.wrapping_add(1);
        cache.error = Some(source::safe_code(code));
        cache.last_attempt = Some(now_ms());
    }
    fn refresh(&self, epoch: &str, transport: &dyn Transport) -> Result<()> {
        // Keep priority across busy retries; an admitted output may finish, but
        // new output cannot continually steal the next inventory opportunity.
        self.refresh_due.store(true, Ordering::Release);
        let _busy = self.acquire()?;
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last_attempt = Some(now_ms());
        let deadline = Instant::now() + OPERATION;
        let reply = transport
            .call(&self.config, &Request::Inventory {}, deadline)
            .and_then(parse_inventory)
            .and_then(|inventory| {
                if Instant::now() >= deadline {
                    Err(Error("timeout"))
                } else {
                    Ok(inventory)
                }
            });
        let result = match reply {
            Ok(inventory) => {
                self.accept(epoch, inventory);
                Ok(())
            }
            Err(error) => {
                self.fail(source::safe_code(error.code()));
                Err(error)
            }
        };
        self.refresh_due.store(false, Ordering::Release);
        result
    }
    fn poll_once(&self, epoch: &str, transport: &dyn Transport, failures: &mut u32) -> Duration {
        match self.refresh(epoch, transport) {
            Ok(()) => *failures = 0,
            Err(error) if error.code() == "endpoint_busy" => return BUSY_RETRY,
            Err(_) => *failures = (*failures + 1).min(3),
        }
        INTERVAL * (1 << *failures)
    }
    fn accept(&self, epoch: &str, inventory: Inventory) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let continuous = cache.fresh.is_some_and(|at| at.elapsed() < STALE)
            && cache
                .inventory
                .as_ref()
                .is_some_and(|old| old.generation == inventory.generation);
        // These are gateway references, not native identity. Only terminals in
        // the immediately preceding continuous inventory can retain a reference.
        let previous: std::collections::HashMap<String, String> = if continuous {
            cache
                .inventory
                .iter()
                .flat_map(|old| &old.agents)
                .zip(&cache.ids)
                .map(|(agent, id)| (agent.terminal_id.clone(), id.clone()))
                .collect()
        } else {
            std::collections::HashMap::new()
        };
        cache.ids = inventory
            .agents
            .iter()
            .map(|agent| {
                if let Some(id) = previous.get(&agent.terminal_id) {
                    return id.clone();
                }
                cache.generation = cache.generation.wrapping_add(1);
                digest(&[
                    epoch,
                    &self.config.id,
                    &cache.generation.to_string(),
                    &inventory.generation,
                    &agent.terminal_id,
                ])
            })
            .collect();
        cache.inventory = Some(inventory);
        cache.fresh = Some(Instant::now());
        let now = now_ms();
        cache.last_attempt = Some(now);
        cache.observed = Some(now);
        cache.error = None;
    }
}

fn parse_inventory(value: Value) -> Result<Inventory> {
    let inventory: Inventory = serde_json::from_value(value).map_err(|_| Error("schema_error"))?;
    if !valid_id(&inventory.generation)
        || inventory.agents.len() > 128
        || !matches!(
            inventory.discovery_status.as_str(),
            "observed" | "unsupported"
        )
        || inventory.unclassified_panes > 1024usize.saturating_sub(inventory.agents.len())
    {
        return Err(Error("schema_error"));
    }
    let (mut terminals, mut panes) = (
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    );
    for agent in &inventory.agents {
        if [
            &agent.terminal_id,
            &agent.pane_id,
            &agent.workspace_id,
            &agent.tab_id,
        ]
        .iter()
        .any(|s| {
            s.is_empty() || s.len() > 256 || s.chars().any(|c| c.is_control() || c.is_whitespace())
        }) || agent.title.len() > 512
            || agent.pane_name.as_ref().is_some_and(|name| {
                !crate::agent_management_presentation::safe_display_text(name, 160)
            })
            || agent.provider.as_ref().is_some_and(|s| s.len() > 512)
            || agent.project.as_ref().is_some_and(|s| s.len() > 512)
            || !matches!(
                agent.evidence.as_str(),
                "kitsune_snapshot" | "coven_process_discovery"
            )
            || agent.provider_candidates.len() > 22
            || agent
                .provider_candidates
                .iter()
                .any(|s| !model::provider_token(s))
            || agent
                .provider_candidates
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != agent.provider_candidates.len()
            || (agent.evidence == "coven_process_discovery"
                && (agent.activity != "unknown"
                    || agent.provider_candidates.is_empty()
                    || inventory.discovery_status != "observed"
                    || agent.provider
                        != if agent.provider_candidates.len() == 1 {
                            agent.provider_candidates.first().cloned()
                        } else {
                            None
                        }))
            || !matches!(
                agent.activity.as_str(),
                "working" | "idle" | "blocked" | "unknown"
            )
            || !terminals.insert(&agent.terminal_id)
            || !panes.insert(&agent.pane_id)
        {
            return Err(Error("schema_error"));
        }
    }
    Ok(inventory)
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub struct Observer {
    attention: Mutex<attention::State>,
    cardlinks: Mutex<cardlinks::State>,
    conversations: conversations::Conversations,
    endpoints: Vec<Arc<Endpoint>>,
    allow_output: bool,
    transport: Arc<dyn Transport>,
    shutdown: Arc<(Mutex<bool>, Condvar)>,
    pollers: Vec<JoinHandle<()>>,
}

impl Observer {
    pub fn start(config: &Config) -> Result<Self> {
        config.validate()?;
        let epoch =
            crate::platform::random_uuid_hex().map_err(|_| Error("observer_start_failed"))?;
        let mut observer = Self {
            attention: Mutex::new(attention::State::default()),
            cardlinks: Mutex::new(cardlinks::State::default()),
            conversations: conversations::Conversations::new(config, &epoch)?,
            endpoints: config
                .endpoints
                .iter()
                .map(|config| {
                    Arc::new(Endpoint {
                        config: config.clone(),
                        cache: Mutex::new(Cache::default()),
                        busy: AtomicBool::new(false),
                        refresh_due: AtomicBool::new(true),
                    })
                })
                .collect(),
            allow_output: config.allow_best_effort_output,
            transport: Arc::new(NativeTransport),
            shutdown: Arc::new((Mutex::new(false), Condvar::new())),
            pollers: Vec::new(),
        };
        for endpoint in &observer.endpoints {
            let (endpoint, epoch, transport, shutdown) = (
                endpoint.clone(),
                epoch.clone(),
                observer.transport.clone(),
                observer.shutdown.clone(),
            );
            let handle = std::thread::Builder::new()
                .name("fleet-observer".into())
                .spawn(move || {
                    let mut failures = 0u32;
                    loop {
                        if *shutdown.0.lock().unwrap_or_else(|p| p.into_inner()) {
                            break;
                        }
                        let delay = endpoint.poll_once(&epoch, transport.as_ref(), &mut failures);
                        let guard = shutdown.0.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = shutdown.1.wait_timeout_while(guard, delay, |stop| !*stop);
                    }
                })
                .map_err(|_| Error("observer_start_failed"))?;
            observer.pollers.push(handle);
        }
        Ok(observer)
    }

    pub fn fleet(&self) -> Value {
        let endpoints: Vec<Value> = self.endpoints.iter().map(|endpoint| {
            let cache = endpoint.cache.lock().unwrap_or_else(|p| p.into_inner());
            let fresh = cache.fresh.is_some_and(|at| at.elapsed() < STALE);
            let agents: Vec<Value> = cache.inventory.iter().flat_map(|i| &i.agents).zip(&cache.ids).map(|(agent,id)| json!({
                "id":id,"title":agent.title,"pane_name":agent.pane_name,"provider":agent.provider.as_deref().unwrap_or(""),"project":agent.project.as_deref().unwrap_or(""),
                "activity":agent.activity,"evidence":agent.evidence,"provider_candidates":agent.provider_candidates,"observed_at_ms":cache.observed.unwrap_or(0),
                "output_available":self.allow_output && fresh && agent.evidence == "kitsune_snapshot"
            })).collect();
            json!({"id":endpoint.config.id,"label":endpoint.config.label,
                "connection":if cache.error == Some("unsupported_protocol") {"unsupported"} else if fresh {"reachable"} else {"unavailable"},
                "last_attempt_at_ms":cache.last_attempt.unwrap_or(0),"observed_at_ms":cache.observed,
                "discovery_status":if fresh {cache.inventory.as_ref().map(|i| i.discovery_status.as_str()).unwrap_or("unavailable")} else {"unavailable"},
                "unclassified_panes":cache.inventory.as_ref().map(|i| i.unclassified_panes),
                "error":cache.error.or(if fresh {None} else {Some("upstream_unavailable")}),"agents":agents})
        }).collect();
        json!({"contract":CONTRACT,"generated_at_ms":now_ms(),"refresh_interval_ms":3000,"stale_after_ms":15000,"endpoints":endpoints})
    }

    pub fn output(&self, id: &str) -> Result<Value> {
        if !valid_id(id) {
            return Err(Error("invalid_reference"));
        }
        if !self.allow_output {
            return Err(Error("output_disabled"));
        }
        for endpoint in &self.endpoints {
            let found = endpoint
                .cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .ids
                .iter()
                .any(|known| known == id);
            if !found {
                continue;
            }
            if endpoint.refresh_due.load(Ordering::Acquire) {
                return Err(Error("endpoint_busy"));
            }
            let _busy = endpoint.acquire()?;
            let (terminal_id, generation) = {
                let cache = endpoint.cache.lock().unwrap_or_else(|p| p.into_inner());
                if !cache.fresh.is_some_and(|at| at.elapsed() < STALE) {
                    return Err(Error("stale_reference"));
                }
                if endpoint.refresh_due.load(Ordering::Acquire)
                    || cache.fresh.is_some_and(|at| at.elapsed() >= INTERVAL)
                {
                    return Err(Error("endpoint_busy"));
                }
                let index = cache
                    .ids
                    .iter()
                    .position(|known| known == id)
                    .ok_or(Error("invalid_reference"))?;
                let inventory = cache.inventory.as_ref().ok_or(Error("invalid_reference"))?;
                if inventory.agents[index].evidence != "kitsune_snapshot" {
                    return Err(Error("output_unsupported"));
                }
                (
                    inventory.agents[index].terminal_id.clone(),
                    inventory.generation.clone(),
                )
            };
            let deadline = Instant::now() + OPERATION;
            let result = self
                .transport
                .call(
                    &endpoint.config,
                    &Request::Output {
                        terminal_id: terminal_id.clone(),
                    },
                    deadline,
                )
                .and_then(|value| {
                    if Instant::now() >= deadline {
                        return Err(Error("timeout"));
                    }
                    if !endpoint
                        .cache
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .fresh
                        .is_some_and(|at| at.elapsed() < STALE)
                    {
                        return Err(Error("stale_reference"));
                    }
                    let output: Output =
                        serde_json::from_value(value).map_err(|_| Error("schema_error"))?;
                    if output.generation != generation || output.terminal_id != terminal_id {
                        return Err(Error("terminal_replaced"));
                    }
                    if output.text.len() > model::OUTPUT_LIMIT {
                        return Err(Error("response_limit"));
                    }
                    Ok(output)
                });
            return match result {
                Ok(output) => Ok(
                    json!({"contract":CONTRACT,"agent_id":id,"observed_at_ms":now_ms(),"text":output.text,"truncated":output.truncated,
                    "attribution":"matching_snapshot_brackets_best_effort"}),
                ),
                Err(error) => {
                    let code = source::safe_code(error.code());
                    endpoint.fail(code);
                    Err(Error(code))
                }
            };
        }
        Err(Error("invalid_reference"))
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        *self.shutdown.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.shutdown.1.notify_all();
        for poller in self.pollers.drain(..) {
            let _ = poller.join();
        }
    }
}

pub fn probe(socket: &str, input: &[u8]) -> Result<Value> {
    if input.len() > 4096 {
        return Err(Error("request_limit"));
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|_| Error("invalid_probe_request"))?;
    source::execute(socket, &request, Instant::now() + OPERATION)
}

/// Closed helper stdout envelope. The CLI should serialize this value and exit
/// successfully even for structured errors; transport failures remain nonzero.
/// Neither diagnostics nor arbitrary upstream error strings are forwarded.
pub fn probe_response(socket: &str, input: &[u8]) -> Value {
    match probe(socket, input) {
        Ok(result) => json!({"result":result}),
        Err(error) => {
            json!({"error":{"code":transport::probe_code(error.code()).unwrap_or("upstream_unavailable")}})
        }
    }
}

#[cfg(test)]
mod tests;
