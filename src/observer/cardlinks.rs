//! Principal-scoped metadata joins. Neither published paths nor stable card IDs are authority.
use super::*;
use crate::shared_native::{Association, Connection};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    revisions: HashMap<String, u64>,
    serial: u64,
    links: HashMap<String, Vec<Link>>,
}

#[derive(Clone)]
struct Link {
    conversation: String,
    agent: String,
    endpoint: String,
    fingerprint: String,
    binding: String,
    expires: Instant,
    cache_generation: u64,
    inventory_generation: String,
    terminal: model::Agent,
    proof: String,
}

struct Card {
    endpoint: usize,
    agent: String,
    generation: String,
    cache_generation: u64,
    terminal: model::Agent,
}

impl Observer {
    pub(super) fn metadata_card_fence(&self, login: &str) -> (u64, Option<Instant>) {
        let state = self.cardlinks.lock().unwrap_or_else(|p| p.into_inner());
        (
            state.revisions.get(login).copied().unwrap_or_default(),
            state
                .links
                .get(login)
                .into_iter()
                .flatten()
                .map(|l| l.expires)
                .min(),
        )
    }

    /// Refresh native grants normally, then acquire bounded, transcript-free metadata.
    pub fn conversation_list(&self, login: &str) -> Value {
        self.conversation_list_with_fence(login, self.attention_revision(login))
            .0
    }

    pub(super) fn conversation_list_with_fence(
        &self,
        login: &str,
        revision: u64,
    ) -> (Value, metadata::Fence) {
        let (mut value, proof) = self.conversations.list_with_proof(login);
        // The list's grant proof is captured with its rows; fleet evidence precedes
        // card acquisition, and the card revision comes from the actual commit.
        let mut fence = metadata::Fence::for_list(self, proof.clone(), revision);
        fence.cards = self.enrich_cards(login, &mut value, proof.card);
        (value, fence)
    }

    fn cards(&self) -> Vec<Card> {
        let mut cards = Vec::new();
        for (index, endpoint) in self.endpoints.iter().enumerate() {
            if endpoint.config.ssh.is_some() {
                continue;
            }
            let cache = endpoint.cache.lock().unwrap_or_else(|p| p.into_inner());
            if !cache.fresh.is_some_and(|at| at.elapsed() < STALE) {
                continue;
            }
            if let Some(inventory) = &cache.inventory {
                cards.extend(
                    inventory
                        .agents
                        .iter()
                        .zip(&cache.ids)
                        .map(|(terminal, id)| Card {
                            endpoint: index,
                            agent: id.clone(),
                            generation: inventory.generation.clone(),
                            cache_generation: cache.generation,
                            terminal: terminal.clone(),
                        }),
                );
            }
        }
        cards
    }

    fn card_current(&self, card: &Card) -> bool {
        let cache = self.endpoints[card.endpoint]
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cache.fresh.is_some_and(|at| at.elapsed() < STALE)
            && cache.generation == card.cache_generation
            && cache.inventory.as_ref().is_some_and(|i| {
                i.generation == card.generation
                    && i.agents
                        .iter()
                        .zip(&cache.ids)
                        .any(|(a, id)| id == &card.agent && a.same_terminal(&card.terminal))
            })
    }

    fn enrich_cards(
        &self,
        login: &str,
        value: &mut Value,
        proof: Option<String>,
    ) -> (u64, Option<Instant>) {
        if !self.conversations.admitted(login) {
            return (0, None);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let revision = self
            .cardlinks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .revisions
            .get(login)
            .copied()
            .unwrap_or_default();
        // Display truncation hides authorized competitors, so uniqueness cannot
        // be proven from these rows. Keep the native handles, but do no join I/O.
        let omitted = value["omitted_conversations"].as_u64().unwrap_or(0) > 0;
        let proof = proof.unwrap_or_default();
        let mut complete = !proof.is_empty()
            && !value["sources"]
                .as_array()
                .is_some_and(|sources| sources.iter().any(|s| s["status"] == "unavailable"));
        let Some(rows) = value["conversations"].as_array_mut() else {
            return (revision, None);
        };
        // Capture fleet mappings before any native or filesystem work. All native
        // candidates below must first pass the conversation source/row ACL.
        let cards = self.cards();
        let mut candidates: Vec<(usize, usize, Link)> = Vec::new();
        for (row_index, row) in rows.iter_mut().enumerate() {
            row["association_status"] = json!("unavailable");
            row["card"] = Value::Null;
            if omitted {
                continue;
            }
            let Some(id) = row["id"].as_str().map(str::to_owned) else {
                continue;
            };
            let Ok(selected) = self.conversations.selected(login, &id) else {
                continue;
            };
            let started = Instant::now();
            let mut connection = Connection::new(&selected.endpoint, &selected.runtime, deadline);
            let association = match connection.association(&selected.grant) {
                Ok(Some(a)) => a,
                Ok(None) => continue,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let mut expires =
                (started + Duration::from_millis(association.lease_ms)).min(selected.expires);
            if expires <= Instant::now() {
                complete = false;
                continue;
            }
            let mut matches = Vec::new();
            for (card_index, card) in cards.iter().enumerate() {
                if card.terminal.terminal_id != association.target.terminal_id {
                    continue;
                }
                let endpoint = &self.endpoints[card.endpoint];
                if source::verify_card(
                    &endpoint.config.socket,
                    &card.generation,
                    &card.terminal,
                    &association.target,
                    deadline,
                )
                .is_ok()
                    && self.card_current(card)
                {
                    matches.push(card_index);
                }
            }
            if matches.len() > 1 {
                row["association_status"] = json!("ambiguous");
            }
            if !matches.is_empty() {
                let stable = |mut a: Association| {
                    a.lease_ms = 1;
                    a
                };
                match connection.association(&selected.grant) {
                    Ok(Some(current))
                        if stable(current.clone()) == stable(association.clone())
                            && expires > Instant::now() => {}
                    _ => {
                        complete = false;
                        matches.clear();
                    }
                }
            }
            if !matches.iter().all(|index| {
                let card = &cards[*index];
                source::verify_card(
                    &self.endpoints[card.endpoint].config.socket,
                    &card.generation,
                    &card.terminal,
                    &association.target,
                    deadline,
                )
                .is_ok()
                    && self.card_current(card)
            }) {
                complete = false;
            }
            match self.conversations.selected(login, &id) {
                Ok(now)
                    if now.endpoint == selected.endpoint
                        && now.runtime == selected.runtime
                        && now.grant.grant_id == selected.grant.grant_id
                        && now.grant.session_id == selected.grant.session_id
                        && now.grant.can_submit == selected.grant.can_submit
                        && now.expires > Instant::now()
                        && selected.expires > Instant::now() =>
                {
                    expires = expires.min(now.expires);
                }
                _ => complete = false,
            }
            // Include every alias in the uniqueness calculation, never pick one.
            for card_index in matches {
                let card = &cards[card_index];
                let endpoint = &self.endpoints[card.endpoint];
                let fingerprint = digest(&[
                    &proof,
                    login,
                    &id,
                    &format!("{:?}", selected.endpoint),
                    &selected.runtime,
                    &selected.grant.grant_id,
                    &selected.grant.session_id,
                    &association.instance_id,
                    &association.session_id,
                    &association.receiver_epoch,
                    &association.scope,
                    &association.target.socket,
                    &association.target.device.to_string(),
                    &association.target.inode.to_string(),
                    &association.target.terminal_id,
                    &association.target.runtime_generation,
                    &endpoint.config.id,
                    &card.agent,
                    &card.generation,
                    &card.terminal.pane_id,
                    &card.terminal.workspace_id,
                    &card.terminal.tab_id,
                ]);
                candidates.push((
                    row_index,
                    card_index,
                    Link {
                        conversation: id.clone(),
                        agent: card.agent.clone(),
                        endpoint: endpoint.config.id.clone(),
                        fingerprint,
                        binding: String::new(),
                        expires,
                        cache_generation: card.cache_generation,
                        inventory_generation: card.generation.clone(),
                        terminal: card.terminal.clone(),
                        proof: proof.clone(),
                    },
                ));
            }
        }
        complete &= self.conversations.card_proof(login).as_deref() == Some(proof.as_str());
        let mut links = Vec::new();
        for (row, card_index, link) in &candidates {
            let count_row = candidates.iter().filter(|(r, _, _)| r == row).count();
            // Row uniqueness rejects fleet aliases; card uniqueness rejects
            // multiple authorized conversations claiming the same terminal.
            let count_card = candidates
                .iter()
                .filter(|(_, c, _)| c == card_index)
                .count();
            if count_row != 1 || count_card != 1 {
                rows[*row]["association_status"] = json!("ambiguous");
            } else if complete
                && link.expires > Instant::now()
                && self.card_current(&cards[*card_index])
                && self
                    .conversations
                    .selected(login, &link.conversation)
                    .is_ok()
            {
                links.push((*row, link.clone()));
            }
        }
        let mut state = self.cardlinks.lock().unwrap_or_else(|p| p.into_inner());
        if self.conversations.card_proof(login).as_deref() != Some(proof.as_str()) {
            links.clear();
        }
        let superseded = state.revisions.get(login).copied().unwrap_or_default() != revision;
        for (row, link) in &mut links {
            if link.expires <= Instant::now() {
                continue;
            }
            let previous =
                state.links.get(login).into_iter().flatten().find(|old| {
                    old.fingerprint == link.fingerprint && old.expires > Instant::now()
                });
            link.binding = if let Some(old) = previous {
                if superseded {
                    // Reconcile only independently verified, still-committed
                    // identity. A losing sample cannot renew or restore a lease.
                    link.expires = link.expires.min(old.expires);
                }
                old.binding.clone()
            } else if superseded {
                continue;
            } else {
                state.serial += 1;
                digest(&[&link.fingerprint, &state.serial.to_string()])
            };
            rows[*row]["association_status"] = json!("verified");
            rows[*row]["card"] = json!({"fleet_agent_id":link.agent,
                "fleet_endpoint_id":link.endpoint,"binding_id":link.binding});
        }
        if !superseded {
            state
                .links
                .insert(login.into(), links.into_iter().map(|(_, l)| l).collect());
            state
                .revisions
                .insert(login.into(), revision.wrapping_add(1));
        }
        // A losing acquisition keeps its original revision, never a newer winner's.
        (
            if superseded {
                revision
            } else {
                revision.wrapping_add(1)
            },
            state
                .links
                .get(login)
                .into_iter()
                .flatten()
                .map(|l| l.expires)
                .min(),
        )
    }

    pub(super) fn bound(
        &self,
        login: &str,
        agent: &str,
        conversation: &str,
        binding: &str,
    ) -> bool {
        let link = self
            .cardlinks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .links
            .get(login)
            .into_iter()
            .flatten()
            .find(|l| {
                l.agent == agent
                    && l.conversation == conversation
                    && l.binding == binding
                    && l.expires > Instant::now()
            })
            .cloned();
        link.is_some_and(|link| {
            let fleet_current = self.endpoints.iter().any(|endpoint| {
                if endpoint.config.id != link.endpoint {
                    return false;
                }
                let cache = endpoint.cache.lock().unwrap_or_else(|p| p.into_inner());
                cache.generation == link.cache_generation
                    && cache.fresh.is_some_and(|at| at.elapsed() < STALE)
                    && cache.inventory.as_ref().is_some_and(|i| {
                        i.generation == link.inventory_generation
                            && i.agents
                                .iter()
                                .zip(&cache.ids)
                                .any(|(a, id)| id == agent && a.same_terminal(&link.terminal))
                    })
            });
            fleet_current
                && self.conversations.card_proof(login).as_deref() == Some(link.proof.as_str())
        })
    }

    /// Revalidate only source/grant/card metadata. Never fetch a transcript.
    pub(super) fn verify_card_binding(
        &self,
        login: &str,
        agent: &str,
        conversation: &str,
        binding: &str,
    ) -> Result<()> {
        if ![agent, conversation, binding].iter().all(|id| valid_id(id))
            || self.conversations.selected(login, conversation).is_err()
            || !self.bound(login, agent, conversation, binding)
        {
            return Err(Error("invalid_reference"));
        }
        let value = self.conversation_list(login);
        if !value["conversations"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["id"] == conversation
                    && row["association_status"] == "verified"
                    && row["card"]["fleet_agent_id"] == agent
                    && row["card"]["binding_id"] == binding
            })
        }) || !self.bound(login, agent, conversation, binding)
        {
            return Err(Error("stale_reference"));
        }
        Ok(())
    }

    pub fn linked_conversation(
        &self,
        login: &str,
        agent: &str,
        conversation: &str,
        binding: &str,
    ) -> Result<Value> {
        if ![agent, conversation, binding].iter().all(|id| valid_id(id))
            || self.conversations.selected(login, conversation).is_err()
            || !self.bound(login, agent, conversation, binding)
        {
            return Err(Error("invalid_reference"));
        }
        let confirms = |value: &Value| {
            value["conversations"].as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row["id"] == conversation
                        && row["association_status"] == "verified"
                        && row["card"]["fleet_agent_id"] == agent
                        && row["card"]["binding_id"] == binding
                })
            })
        };
        if !confirms(&self.conversation_list(login))
            || !self.bound(login, agent, conversation, binding)
        {
            return Err(Error("stale_reference"));
        }
        let value = self.conversations.read(login, conversation)?;
        if !confirms(&self.conversation_list(login))
            || !self.bound(login, agent, conversation, binding)
        {
            return Err(Error("stale_reference"));
        }
        Ok(value)
    }
}
