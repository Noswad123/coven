use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Associations(BTreeMap<String, Entry>);
struct Entry {
    revision: String,
    target: Value,
    asserted: Instant,
    fact: u64,
    valid: bool,
}
impl Associations {
    pub(super) fn current_deadlines(
        &self,
        native: &native::NativeStore,
        now: Instant,
    ) -> BTreeMap<String, Instant> {
        self.0
            .keys()
            .filter_map(|id| {
                self.current_deadline(id, native, now)
                    .map(|end| (id.clone(), end))
            })
            .collect()
    }

    pub(super) fn current_deadline(
        &self,
        id: &str,
        native: &native::NativeStore,
        now: Instant,
    ) -> Option<Instant> {
        let entry = self.0.get(id).filter(|e| {
            e.valid
                && now.saturating_duration_since(e.asserted) < Duration::from_secs(30)
                && native.navigation_fact(id, now) == Some(e.fact)
        })?;
        native
            .naming_deadline(id, now, false)
            .map(|end| end.min(entry.asserted + Duration::from_secs(30)))
    }

    // Private publisher projection only, never part of observations/control results.
    pub(super) fn naming_hints(
        &self,
        view: &Value,
        native: &native::NativeStore,
        now: Instant,
    ) -> Value {
        let mut hints = Vec::new();
        for row in view["observations"]
            .as_array()
            .into_iter()
            .flatten()
            .take(128)
        {
            let Some(id) = row["observation_id"].as_str() else {
                continue;
            };
            let hint = if let Some(entry) = self.0.get(id).filter(|e| {
                e.valid
                    && now.saturating_duration_since(e.asserted) < Duration::from_secs(30)
                    && native.navigation_fact(id, now) == Some(e.fact)
            }) {
                native.naming_deadline(id, now, false).map(|end| {
                    (
                        entry.revision.clone(),
                        entry.target.clone(),
                        end.min(entry.asserted + Duration::from_secs(30)),
                    )
                })
            } else {
                native
                    .producer_association(id, now)
                    .and_then(|(revision, target)| {
                        native
                            .naming_deadline(id, now, true)
                            .map(|end| (revision, target, end))
                    })
            };
            if let Some((revision, target, end)) = hint {
                hints.push(json!({"id":format!("observation:{id}"),"revision":revision,
                    "target":target,"until":crate::agent_management_presentation::naming_tick(end)}));
            }
        }
        json!(hints)
    }

    pub(super) fn refresh(&mut self, native: &native::NativeStore, now: Instant) {
        for (id, entry) in &mut self.0 {
            entry.valid &= now.saturating_duration_since(entry.asserted) < Duration::from_secs(30)
                && native.navigation_fact(id, now) == Some(entry.fact);
        }
    }

    pub(super) fn control(
        &mut self,
        method: &str,
        p: &Value,
        epoch: &str,
        native: &native::NativeStore,
        now: Instant,
    ) -> Result<Value> {
        self.refresh(native, now);
        if matches!(method, "focus.prepare" | "association.inspect")
            && !self
                .0
                .get(text(p, "observation_id")?)
                .is_some_and(|e| e.valid)
        {
            require(
                p.as_object().is_some_and(|o| {
                    o.len() == 3
                        && ["observation_id", "expected_epoch", "expected_revision"]
                            .iter()
                            .all(|k| o.contains_key(*k))
                }),
                "agent_invalid_schema",
            )?;
            let id = text(p, "observation_id")?;
            crate::agent_navigation::hex(id, 64)?;
            crate::agent_navigation::hex(text(p, "expected_epoch")?, 32)?;
            require(p["expected_epoch"] == epoch, "agent_stale_epoch")?;
            let (revision, target) = native
                .producer_association(id, now)
                .ok_or(Error("agent_association_unknown"))?;
            require(
                (method == "association.inspect" && p["expected_revision"].is_null())
                    || p["expected_revision"] == revision,
                "agent_stale_revision",
            )?;
            return Ok(
                json!({"contract":"coven.agent-navigation.v1","server_epoch":epoch,
                "observation_id":id,"association_revision":revision,"provenance":"producer_reported",
                "native_hosting":"local_full_tui_root","valid":true,"applied":false,"target":target}),
            );
        }
        let keys = if method == "association.assert" {
            6
        } else if method == "association.remove" {
            5
        } else {
            3
        };
        require(
            p.as_object().is_some_and(|o| o.len() == keys),
            "agent_invalid_schema",
        )?;
        let id = text(p, "observation_id")?;
        crate::agent_navigation::hex(id, 64)?;
        crate::agent_navigation::hex(text(p, "expected_epoch")?, 32)?;
        require(p["expected_epoch"] == epoch, "agent_stale_epoch")?;
        let expected = p
            .get("expected_revision")
            .ok_or(Error("agent_invalid_schema"))?;
        require(
            expected.is_null()
                || expected
                    .as_str()
                    .is_some_and(|s| crate::agent_navigation::hex(s, 32).is_ok()),
            "agent_invalid_schema",
        )?;
        let current = self.0.get(id);
        require(
            (method == "association.inspect" && expected.is_null())
                || current.map(|e| json!(e.revision)).unwrap_or(Value::Null) == *expected,
            "agent_stale_revision",
        )?;
        let mut revision = current.map(|e| e.revision.clone());
        let mut target = current.map(|e| e.target.clone());
        let mut valid = current.is_some_and(|e| e.valid);
        let apply = if matches!(method, "focus.prepare" | "association.inspect") {
            false
        } else {
            p["apply"].as_bool().ok_or(Error("agent_invalid_schema"))?
        };
        if method == "association.assert" {
            require(p["provenance"] == "user_asserted", "agent_invalid_schema")?;
            crate::agent_navigation::validate_target(&p["target"])?;
            let fact = native
                .navigation_fact(id, now)
                .ok_or(Error("agent_observation_not_fresh"))?;
            require(
                current.is_some() || self.0.len() < 128,
                "agent_association_limit",
            )?;
            target = Some(p["target"].clone());
            valid = true;
            if apply {
                let next = crate::platform::random_uuid_hex()?;
                self.0.insert(
                    id.to_owned(),
                    Entry {
                        revision: next.clone(),
                        target: p["target"].clone(),
                        asserted: now,
                        fact,
                        valid: true,
                    },
                );
                revision = Some(next);
            }
        } else if method == "association.remove" {
            require(
                p["provenance"] == "user_asserted" && current.is_some(),
                "agent_unknown_association",
            )?;
            if apply {
                // Keep a bounded tombstone so stale create/remove requests cannot revive it.
                let entry = self.0.get_mut(id).unwrap();
                entry.revision = crate::platform::random_uuid_hex()?;
                entry.valid = false;
                revision = Some(entry.revision.clone());
                valid = false;
            }
        } else {
            require(
                matches!(method, "focus.prepare" | "association.inspect") && valid,
                "agent_association_unknown",
            )?;
        }
        Ok(
            json!({"contract":"coven.agent-navigation.v1","server_epoch":epoch,
            "observation_id":id,"association_revision":revision,"provenance":"user_asserted",
            "native_hosting":"unestablished","valid":valid,"applied":apply,"target":target}),
        )
    }
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
