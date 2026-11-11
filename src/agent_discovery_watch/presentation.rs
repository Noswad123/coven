use super::*;
use crate::{
    group_presenter::wire,
    observation_socket::{Delivery, Endpoint, Failure, WireResult},
};
use sha2::{Digest, Sha256};

pub(super) struct Presenter {
    source: Option<String>,
    endpoint: WireResult<Endpoint>,
    owner: Option<Value>,
    nonce: String,
    stopped: bool,
    revision: u64,
    expires: Option<Instant>,
    displayed_at: Option<Instant>,
    displayed: Vec<String>,
    selected: Option<String>,
    pub(super) report: Value,
}

impl Presenter {
    pub(super) fn new(source: Option<&str>, endpoint: WireResult<Endpoint>) -> Self {
        let nonce = if source.is_some() {
            crate::platform::random_uuid_hex()
        } else {
            Ok(String::new())
        };
        let outcome = if source.is_none() {
            "disabled"
        } else if nonce.is_err() {
            "failed"
        } else {
            "pending"
        };
        Self {
            source: source.map(str::to_owned),
            endpoint,
            owner: None,
            stopped: source.is_none() || nonce.is_err(),
            nonce: nonce.unwrap_or_default(),
            revision: 0,
            expires: None,
            displayed_at: None,
            displayed: Vec::new(),
            selected: None,
            report: json!({"outcome":outcome,
                "authorization":"none","selection":"read_only_observation_details",
                "delivery":"ephemeral_not_lossless","frames":0,"polls":0,"selections":0,
                "ttl_ms":5000,"max_rows":128,"max_frame_bytes":98304,
                "ignored_intents":0,"omitted_matches":0,"cleanup":"not_opened","diagnostics":[]}),
        }
    }

    fn deadline(end: Instant) -> Instant {
        end.min(Instant::now() + Duration::from_secs(1))
    }

    pub(super) fn sample(
        &mut self,
        sample: &Value,
        sampled: Instant,
        end: Instant,
        cancel: &dyn Cancel,
    ) {
        if self.stopped {
            return;
        }
        // Poll and replace renew TTL. Neither may renew a stale observation or expired lease.
        if Instant::now() + Duration::from_secs(1) >= sampled + Duration::from_secs(5)
            || self
                .expires
                .is_some_and(|expiry| Instant::now() + Duration::from_secs(1) >= expiry)
        {
            self.stopped = true;
            self.report["outcome"] = json!("expired");
            self.report["cleanup"] = json!("ttl_only_expired");
            return;
        }
        if let Err(error) = self.publish(sample, sampled, end, cancel) {
            self.stopped = true;
            self.report["outcome"] = json!(if error.delivery == Delivery::Uncertain {
                "uncertain"
            } else {
                "failed"
            });
            self.report["diagnostics"] = json!([error.code]);
            if error.code == "presentation_sample_expired" {
                self.report["outcome"] = json!("expired");
                self.report["cleanup"] = json!("ttl_only_expired");
                self.owner = None;
            } else if error.delivery == Delivery::Uncertain {
                self.report["cleanup"] = json!("ttl_only_uncertain");
                self.owner = None;
            } else {
                self.close();
            }
        }
    }

    fn publish(
        &mut self,
        sample: &Value,
        sampled: Instant,
        end: Instant,
        cancel: &dyn Cancel,
    ) -> WireResult<()> {
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let source = self.source.as_deref().unwrap();
        if self.owner.is_none() {
            if socket::select(&endpoint.selected)? != *endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            let reply = socket::read(
                &endpoint.selected,
                socket::ReadMethod::Snapshot,
                &json!({}),
                Self::deadline(end),
                &mut socket::ReadBudget::new(1024 * 1024),
                cancel,
            )?;
            if reply.endpoint != *endpoint {
                return Err(Failure::new("endpoint_replaced"));
            }
            wire::snapshot(&reply, source)?;
            let sent = Instant::now();
            let opened = socket::view_call(
                endpoint,
                "integration.view.open",
                &json!({"source":source,"version":1}),
                Self::deadline(end),
                cancel,
                &wire::opened,
            )?;
            self.owner =
                Some(json!({"source":source,"lease":opened["lease"],"epoch":opened["epoch"]}));
            self.expires = Some(sent + Duration::from_secs(5));
            self.report["cleanup"] = json!("ttl_only");
        }
        if Instant::now() + Duration::from_secs(2) >= sampled + Duration::from_secs(5) {
            return Err(Failure::new("presentation_sample_expired"));
        }
        let (fresh, omitted) = rows(sample, &self.nonce, None);
        let poll_before = sample["assessment"] == "observed" && self.revision > 0;
        if poll_before {
            // Kit replace clears queued clicks. Drain the displayed revision first,
            // but only while both that frame and the new observation are fresh.
            let poll_end = (sampled + Duration::from_secs(5))
                .min(self.displayed_at.unwrap() + Duration::from_secs(5))
                .min(self.expires.unwrap());
            if Instant::now() + Duration::from_secs(1) >= poll_end {
                return Err(Failure::new("presentation_sample_expired"));
            }
            self.poll(&fresh, end.min(poll_end), cancel)?;
        }
        if !fresh
            .iter()
            .skip(1)
            .any(|row| row["id"].as_str() == self.selected.as_deref())
        {
            self.selected = None;
        }
        let (rows, _) = rows(sample, &self.nonce, self.selected.as_deref());
        self.report["omitted_matches"] = json!(omitted);
        self.revision += 1;
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let owner = self.owner.as_ref().unwrap();
        let epoch = owner["epoch"].as_str().unwrap();
        let mut frame = owner.clone();
        frame["revision"] = json!(self.revision);
        frame["items"] = json!(rows);
        if serde_json::to_vec(&frame)
            .map_err(|_| Failure::new("integration_view_invalid_frame"))?
            .len()
            > 96 * 1024
        {
            return Err(Failure::new("integration_view_invalid_frame"));
        }
        if Instant::now() + Duration::from_secs(2) >= sampled + Duration::from_secs(5) {
            return Err(Failure::new("presentation_sample_expired"));
        }
        let sent = Instant::now();
        socket::view_call(
            endpoint,
            "integration.view.replace",
            &frame,
            Self::deadline(end),
            cancel,
            &|bytes, id| wire::replaced(bytes, id, epoch, self.revision),
        )?;
        self.expires = Some(sent + Duration::from_secs(5));
        self.displayed_at = Some(sampled);
        self.displayed = rows
            .iter()
            .skip(1)
            .map(|row| row["id"].as_str().unwrap().to_owned())
            .collect();
        count(&mut self.report, "frames");
        // Failed/incompatible samples clear the old positive frame before any
        // lease-renewing poll. Its discarded intents cannot select stale candidates.
        if !poll_before {
            self.poll(&rows, end, cancel)?;
        }
        self.report["outcome"] = json!("active");
        Ok(())
    }

    fn poll(&mut self, fresh: &[Value], end: Instant, cancel: &dyn Cancel) -> WireResult<()> {
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let owner = self.owner.as_ref().unwrap();
        let epoch = owner["epoch"].as_str().unwrap();
        let polled = socket::view_call(
            endpoint,
            "integration.view.poll",
            owner,
            Self::deadline(end),
            cancel,
            &|bytes, id| wire::polled(bytes, id, epoch, self.revision),
        )?;
        count(&mut self.report, "polls");
        for intent in polled["intents"].as_array().unwrap() {
            if intent["epoch"] == epoch
                && intent["revision"] == self.revision
                && self.displayed.iter().any(|id| intent["item_id"] == *id)
                && fresh
                    .iter()
                    .skip(1)
                    .any(|row| row["id"] == intent["item_id"])
            {
                self.selected = intent["item_id"].as_str().map(str::to_owned);
                count(&mut self.report, "selections");
            } else {
                count(&mut self.report, "ignored_intents");
            }
        }
        Ok(())
    }

    fn close(&mut self) {
        let Some(owner) = self.owner.take() else {
            return;
        };
        let Ok(endpoint) = &self.endpoint else {
            return;
        };
        let result = socket::view_call(
            endpoint,
            "integration.view.close",
            &owner,
            Instant::now() + Duration::from_millis(200),
            &socket::NoCancel,
            &wire::closed,
        );
        match result {
            Ok(_) => self.report["cleanup"] = json!("closed"),
            Err(error) => {
                self.stopped = true;
                let uncertain = error.delivery == Delivery::Uncertain;
                self.report["cleanup"] = json!(if uncertain {
                    "ttl_only_uncertain"
                } else {
                    "ttl_only"
                });
                self.report["outcome"] = json!(if uncertain { "uncertain" } else { "failed" });
                self.report["diagnostics"] = json!([error.code]);
            }
        }
    }

    pub(super) fn finish(&mut self) {
        if !self.stopped && self.expires.is_some_and(|expiry| Instant::now() >= expiry) {
            self.stopped = true;
            self.report["outcome"] = json!("expired");
            self.report["cleanup"] = json!("ttl_only_expired");
        }
        if !self.stopped {
            self.close();
        }
        if self.report["outcome"] == "active" {
            self.report["outcome"] = json!("completed");
        } else if self.report["outcome"] == "pending" {
            self.report["outcome"] = json!("not_started");
        }
    }
}

fn provider(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| {
        matches!(
            *s,
            "claude"
                | "codex"
                | "copilot"
                | "pi"
                | "opencode"
                | "gemini"
                | "cursor"
                | "devin"
                | "agy"
                | "cline"
                | "omp"
                | "mastracode"
                | "kimi"
                | "kiro"
                | "droid"
                | "amp"
                | "grok"
                | "hermes"
                | "kilo"
                | "qodercli"
                | "maki"
                | "djinn"
        )
    })
}

pub(super) fn rows(sample: &Value, nonce: &str, selected: Option<&str>) -> (Vec<Value>, usize) {
    let observed = sample["assessment"] == "observed";
    let label = match sample["assessment"].as_str() {
        Some("observed") => "Agent discovery: observed",
        Some("incompatible") => "Agent discovery: incompatible",
        _ => "Agent discovery: unavailable",
    };
    let mut rows = vec![json!({"id":"summary","label":label,"detail":""})];
    let mut total = 0;
    if observed {
        for target in sample["targets"].as_array().into_iter().flatten() {
            for matched in target["matches"].as_array().into_iter().flatten() {
                let Some(provider) = provider(&matched["provider"]) else {
                    continue;
                };
                let Some(pid) = matched["pid"].as_u64() else {
                    continue;
                };
                total += 1;
                if rows.len() == 128 {
                    continue;
                }
                let mut hash = Sha256::new();
                for part in [
                    json!(nonce),
                    target["terminal_id"].clone(),
                    json!(pid),
                    json!(provider),
                ] {
                    let bytes = serde_json::to_vec(&part).unwrap();
                    hash.update((bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
                let id = format!("candidate:{:x}", hash.finalize());
                if rows.iter().any(|row| row["id"] == id) {
                    continue;
                }
                let detail = if selected == Some(id.as_str()) {
                    "Selected (read-only): ephemeral process recognition; activity/attention unknown; native identity, lifecycle and completion unestablished"
                } else {
                    "Ephemeral process recognition; activity/attention unknown; native identity, lifecycle and completion unestablished"
                };
                rows.push(json!({"id":id,"label":format!("{provider} candidate"),"detail":detail}));
            }
        }
    }
    let omitted = total - (rows.len() - 1);
    rows[0]["detail"] = json!(format!(
        "{} displayed matches; {omitted} omitted matches; not a complete agent inventory",
        rows.len() - 1
    ));
    (rows, omitted)
}
