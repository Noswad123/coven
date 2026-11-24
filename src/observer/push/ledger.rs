//! Lifecycle history, not a lifetime event quota. Unknown scopes retain history.
//! Seven-day retention permits re-alerting an occurrence after that interval.
//! The unpublished v2 private schema uses compact notice keys so all 4096
//! admission tokens fit the 1 MiB bound. Tokens never enter public API/payloads.
use super::*;
pub(super) const SUB_LIMIT: usize = 256;
pub(super) const GLOBAL_LIMIT: usize = 4096;
pub(super) const RETENTION: u64 = 7 * 24 * 3600;
pub(super) const RECOVERY: u64 = 3600;
// Longer than the 300s queue TTL plus bounded metadata/transport I/O. This
// recovers dropped jobs (including failed bookkeeping), not sent dedup entries.
pub(super) const PENDING_LEASE: u64 = 600;
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Ready,
    Pending,
    Sent,
    Failed,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Notice {
    #[serde(rename = "s")]
    pub stream: Option<String>,
    #[serde(rename = "t")]
    pub at: u64,
    #[serde(rename = "p")]
    pub phase: Phase,
    #[serde(rename = "j")]
    pub token: String,
}
pub(super) struct Snapshot {
    pub alerts: BTreeSet<String>,
    streams: BTreeMap<String, (bool, BTreeSet<String>)>,
    pub(super) expires: Option<Instant>,
}
impl Snapshot {
    pub fn read(doc: &Value) -> Option<Self> {
        if doc["contract"] != "coven.attention.v1"
            || !matches!(doc["state"].as_str(), Some("observed" | "partial"))
        {
            return None;
        }
        let items = doc["items"].as_array().filter(|a| a.len() <= 256)?;
        // Observer already fences reader ACL and verified card linkage. Writer
        // permission and full details are deliberately NOT notification gates.
        let alerts = items
            .iter()
            .filter(|i| matches!(i["kind"].as_str(), Some("question" | "permission")))
            .filter_map(|i| {
                i["alert_id"]
                    .as_str()
                    .filter(|id| super::super::valid_id(id))
                    .map(str::to_owned)
            })
            .collect();
        let streams = (|| -> Option<BTreeMap<String, (bool, BTreeSet<String>)>> {
            let rows = doc["streams"].as_array().filter(|a| a.len() <= 64)?;
            let mut streams = BTreeMap::new();
            let mut all = BTreeSet::new();
            for row in rows {
                let id = row["id"].as_str().filter(|id| super::super::valid_id(id))?;
                let complete = row["complete"].as_bool()?;
                let mut ids = BTreeSet::new();
                for alert in row["alert_ids"].as_array().filter(|a| a.len() <= 32)? {
                    let alert = alert.as_str().filter(|id| super::super::valid_id(id))?;
                    if !all.insert(alert.to_owned()) || !ids.insert(alert.to_owned()) {
                        return None;
                    }
                }
                if streams.insert(id.to_owned(), (complete, ids)).is_some() {
                    return None;
                }
            }
            Some(streams)
        })()
        .unwrap_or_default();
        Some(Self {
            alerts,
            streams,
            expires: None,
        })
    }
    fn stream(&self, alert: &str) -> Option<String> {
        self.streams
            .iter()
            .find(|(_, (_, ids))| ids.contains(alert))
            .map(|(id, _)| id.clone())
    }
}
impl Inner {
    pub(super) fn reconcile(&self, owner: &str, sample: Option<&Snapshot>, now: u64) -> Result<()> {
        self.mutate(|state| {
            // The write transaction can wait behind filesystem work. An expired
            // complete stream is unknown, not evidence that a notice resolved.
            let sample = sample.filter(|s| s.expires.is_none_or(|at| Instant::now() < at));
            // Retention bounds unknown/removed contexts without inventing resolution.
            for r in state.records.values_mut() {
                r.seen.retain(|_, n| now.saturating_sub(n.at) < RETENTION);
                for n in r.seen.values_mut() {
                    if n.phase == Phase::Pending && now.saturating_sub(n.at) >= PENDING_LEASE {
                        n.phase = Phase::Ready;
                    }
                }
                if r.owner != owner {
                    continue;
                }
                if let Some(sample) = sample {
                    r.seen.retain(|alert, n| {
                        !n.stream
                            .as_ref()
                            .and_then(|id| sample.streams.get(id))
                            .is_some_and(|(complete, ids)| *complete && !ids.contains(alert))
                    });
                    for (alert, n) in &mut r.seen {
                        if n.stream.is_none() {
                            n.stream = sample.stream(alert);
                        }
                    }
                }
            }
            Ok(())
        })
    }
    pub(super) fn poll(&self, owner: &str) {
        if self.stopped() || self.degraded.load(Ordering::Acquire) {
            return;
        }
        let sample = self.sample(owner);
        if self.stopped() {
            return;
        }
        if self
            .reconcile(owner, sample.as_ref().map(|(s, _)| s), now())
            .is_err()
        {
            self.diagnostic("state_failed");
            return;
        }
        let Some((sample, expires)) = sample.filter(|(_, expires)| Instant::now() < *expires)
        else {
            self.diagnostic("unavailable");
            return;
        };
        for alert in &sample.alerts {
            if Instant::now() >= expires {
                return;
            }
            let records: Vec<_> = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .records
                .iter()
                .filter(|(_, r)| {
                    r.owner == owner
                        && r.seen.get(alert).is_none_or(|n| {
                            n.phase == Phase::Ready
                                || (n.phase == Phase::Failed
                                    && now().saturating_sub(n.at) >= RECOVERY)
                        })
                })
                .map(|(id, r)| (id.clone(), r.clone()))
                .collect();
            for (id, record) in records {
                if self
                    .work
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .queue
                    .len()
                    >= 128
                {
                    self.diagnostic("queue_limit");
                    return;
                }
                let mut token = String::new();
                let admitted = self.mutate(|state| {
                    let count: usize = state.records.values().map(|r| r.seen.len()).sum();
                    let r = state
                        .records
                        .get_mut(&id)
                        .ok_or(Error("push_subscription_changed"))?;
                    if r.owner != owner || r.generation != record.generation {
                        return Err(Error("push_subscription_changed"));
                    }
                    if let Some(n) = r.seen.get(alert) {
                        if n.phase != Phase::Ready
                            && (n.phase != Phase::Failed || now().saturating_sub(n.at) < RECOVERY)
                        {
                            return Err(Error("push_already_seen"));
                        }
                    } else if r.seen.len() >= SUB_LIMIT || count >= GLOBAL_LIMIT {
                        return Err(Error("push_history_limit"));
                    }
                    token = crate::platform::random_uuid_hex()
                        .map_err(|_| Error("push_state_unavailable"))?;
                    r.seen.insert(
                        alert.clone(),
                        Notice {
                            stream: sample.stream(alert),
                            at: now(),
                            phase: Phase::Pending,
                            token: token.clone(),
                        },
                    );
                    Ok(())
                });
                if admitted.is_err() {
                    self.diagnostic("history_or_state_limit");
                    continue;
                }
                let at = Instant::now();
                self.work
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .queue
                    .push_back(Job {
                        id,
                        generation: record.generation,
                        token,
                        subscription: record.subscription,
                        owner: owner.into(),
                        alert: alert.clone(),
                        created: at,
                        due: at,
                        attempts: 0,
                    });
                self.wake.notify_all();
            }
        }
    }
    pub(super) fn finish(&self, job: &Job, phase: Phase) {
        if self
            .mutate(|state| {
                if let Some(r) = state
                    .records
                    .get_mut(&job.id)
                    .filter(|r| r.pending_job(job))
                {
                    if let Some(n) = r.seen.get_mut(&job.alert) {
                        n.phase = phase;
                        n.at = now();
                    }
                }
                Ok(())
            })
            .is_err()
        {
            self.diagnostic("state_failed");
        }
    }
    pub(super) fn requeue(&self, mut job: Job, delay: Duration) {
        if !self.eligible(&job) {
            return;
        }
        job.due = Instant::now() + delay;
        let mut work = self.work.lock().unwrap_or_else(|p| p.into_inner());
        if work.queue.len() < 128 && !work.stop {
            work.queue.push_back(job);
            return;
        }
        drop(work);
        self.finish(&job, Phase::Failed);
    }
}
