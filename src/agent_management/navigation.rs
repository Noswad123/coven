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
            current.map(|e| json!(e.revision)).unwrap_or(Value::Null) == *expected,
            "agent_stale_revision",
        )?;
        let mut revision = current.map(|e| e.revision.clone());
        let mut target = current.map(|e| e.target.clone());
        let mut valid = current.is_some_and(|e| e.valid);
        let apply = if method == "focus.prepare" {
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
                method == "focus.prepare" && valid,
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
