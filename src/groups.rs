//! Closed participant declarations, independent of task/config/execution authority.
//! Requests are consumed by value; their observations remain borrowed. No source
//! artifacts, sockets, or runtimes are inspected by this module.
mod schema;
mod store;
mod timestamp;

use crate::{Error, Result, error::require, values::Value};
use schema::*;
pub(crate) use schema::{
    display, selector, source, stable_binding, validate_attachment, validate_binding,
};
use std::path::Path;

pub const UNINITIALIZED: &str = "groups-uninitialized:v1";

pub struct View {
    revision: String,
    state: Option<Value>,
}
impl View {
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn state(&self) -> Option<&Value> {
        self.state.as_ref()
    }
    pub fn group(&self, key: &str) -> Result<&Value> {
        selector(key)?;
        group(
            self.state.as_ref().ok_or(Error("groups_uninitialized"))?,
            key,
        )
    }
    pub fn member(&self, key: &str, member: &str) -> Result<&Value> {
        selector(member)?;
        array(get(self.group(key)?, "members")?)?
            .iter()
            .find(|m| get(m, "key").ok().and_then(Value::as_str) == Some(member))
            .ok_or(Error("groups_member_missing"))
    }
}

pub fn read_groups(workspace: &Path) -> Result<View> {
    let root = store::workspace(workspace)?;
    let (state, _) = store::read(&root)?;
    Ok(View {
        revision: revision(state.as_ref())?,
        state,
    })
}

pub struct AddMember<'a> {
    pub group: &'a str,
    pub member: &'a str,
    pub role: &'a str,
    pub binding: &'a Value,
    pub rebind: bool,
    pub expected_revision: Option<&'a str>,
}
pub struct AttachMember<'a> {
    pub group: &'a str,
    pub member: &'a str,
    pub attachment: &'a Value,
    pub expected_revision: &'a str,
    pub expected_participant_id: &'a str,
    pub reattach: bool,
}
pub struct ReserveRequest<'a> {
    pub group: &'a str,
    pub expected_revision: &'a str,
    pub endpoint: &'a Value,
    pub source: &'a str,
    pub epoch: &'a str,
    pub terminal_ids: &'a [String],
    pub allow_new_epoch: bool,
}

pub fn create_group(
    workspace: &Path,
    key: &str,
    name: &str,
    expected: Option<&str>,
) -> Result<Value> {
    display(name)?;
    mutate(
        workspace,
        key,
        expected,
        Operation::Create(name),
        &store::SystemIo,
    )
}
/// Consumes the request, not its borrowed binding. Only caller-supplied CAS is used.
pub fn add_member(workspace: &Path, request: AddMember<'_>) -> Result<Value> {
    selector(request.member)?;
    display(request.role)?;
    validate_binding(request.binding)?;
    mutate(
        workspace,
        request.group,
        request.expected_revision,
        Operation::Add(request),
        &store::SystemIo,
    )
}
pub fn remove_member(
    workspace: &Path,
    key: &str,
    member: &str,
    expected: Option<&str>,
) -> Result<Value> {
    selector(member)?;
    mutate(
        workspace,
        key,
        expected,
        Operation::Remove(member),
        &store::SystemIo,
    )
}
pub fn attach_live(workspace: &Path, request: AttachMember<'_>) -> Result<Value> {
    validate_attachment(request.attachment)?;
    selector(request.group)?;
    fence(
        request.member,
        request.expected_revision,
        request.expected_participant_id,
    )?;
    mutate(
        workspace,
        request.group,
        Some(request.expected_revision),
        Operation::Attach(request),
        &store::SystemIo,
    )
}
pub fn detach_live(
    workspace: &Path,
    key: &str,
    member: &str,
    expected_revision: &str,
    participant: &str,
) -> Result<Value> {
    selector(key)?;
    fence(member, expected_revision, participant)?;
    mutate(
        workspace,
        key,
        Some(expected_revision),
        Operation::Detach(member, participant),
        &store::SystemIo,
    )
}
fn fence(member: &str, token: &str, participant: &str) -> Result<()> {
    selector(member)?;
    uuid(participant)?;
    let parts: Vec<_> = token.split(':').collect();
    require(
        parts.len() == 4 && parts[0] == "groups-v1",
        "groups_current_revision_required",
    )?;
    uuid(parts[1]).map_err(|_| Error("groups_current_revision_required"))?;
    require(
        !parts[2].is_empty()
            && parts[2].len() <= 20
            && parts[2].bytes().all(|b| b.is_ascii_digit())
            && (parts[2] == "0" || !parts[2].starts_with('0'))
            && parts[3].len() == 64
            && parts[3]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "groups_current_revision_required",
    )
}
fn cas(state: Option<&Value>, expected: Option<&str>) -> Result<()> {
    require(
        expected.is_none_or(|e| revision(state).is_ok_and(|r| e == r)),
        "groups_revision_conflict",
    )
}
fn group<'a>(state: &'a Value, key: &str) -> Result<&'a Value> {
    array(get(state, "groups")?)?
        .iter()
        .find(|g| get(g, "key").ok().and_then(Value::as_str) == Some(key))
        .ok_or(Error("groups_group_missing"))
}
fn revision(state: Option<&Value>) -> Result<String> {
    use sha2::{Digest, Sha256};
    let Some(state) = state else {
        return Ok(UNINITIALIZED.into());
    };
    let content = obj(&[
        ("workspace_id", get(state, "workspace_id")?.clone()),
        ("content_revision", get(state, "content_revision")?.clone()),
        ("groups", get(state, "groups")?.clone()),
    ])?;
    let mut digest = Sha256::new();
    digest.update(b"coven.groups.content:v1\0");
    digest.update(content.canonical()?);
    Ok(format!(
        "groups-v1:{}:{}:{:x}",
        text(get(state, "workspace_id")?)?,
        uint(get(state, "content_revision")?)?,
        digest.finalize()
    ))
}

enum Operation<'a> {
    Create(&'a str),
    Add(AddMember<'a>),
    Remove(&'a str),
    Attach(AttachMember<'a>),
    Detach(&'a str, &'a str),
}

fn mutate(
    workspace: &Path,
    key: &str,
    expected: Option<&str>,
    op: Operation<'_>,
    io: &impl store::Io,
) -> Result<Value> {
    selector(key)?;
    let root = store::workspace(workspace)?;
    let guard = crate::storage::WorkspaceGuard::acquire(&root, std::time::Duration::from_secs(5))?;
    store::workspace(&root)?;
    let (mut state, data) = store::read(&root)?;
    cas(state.as_ref(), expected)?;
    if state.is_none() {
        require(matches!(op, Operation::Create(_)), "groups_uninitialized")?;
        state = Some(obj(&[
            ("schema_version", num(1)?),
            ("workspace_id", string(io.uuid()?)),
            ("content_revision", num(0)?),
            ("groups", Value::Array(vec![])),
            ("publication_counters", Value::Array(vec![])),
        ])?);
    }
    let mut state = state.ok_or(Error("groups_uninitialized"))?;
    let mut groups = array(get(&state, "groups")?)?.to_vec();
    let mut gi = groups
        .iter()
        .position(|g| get(g, "key").ok().and_then(Value::as_str) == Some(key));
    let mut changed = false;
    let mut cleared = false;
    let mut returned_member = Value::Null;
    let is_member = !matches!(op, Operation::Create(_));
    if let Operation::Create(name) = &op {
        if let Some(i) = gi {
            require(
                text(get(&groups[i], "name")?)? == *name,
                "groups_name_conflict",
            )?;
        } else {
            groups.push(obj(&[
                ("group_id", string(io.uuid()?)),
                ("key", string(key)),
                ("name", string(*name)),
                ("members", Value::Array(vec![])),
            ])?);
            gi = Some(groups.len() - 1);
            changed = true;
        }
    }
    let gi = gi.ok_or(Error("groups_group_missing"))?;
    let mut members = array(get(&groups[gi], "members")?)?.to_vec();
    let member_key = match &op {
        Operation::Create(_) => None,
        Operation::Add(r) => Some(r.member),
        Operation::Remove(m) | Operation::Detach(m, _) => Some(*m),
        Operation::Attach(r) => Some(r.member),
    };
    let mi = members
        .iter()
        .position(|m| get(m, "key").ok().and_then(Value::as_str) == member_key);
    match &op {
        Operation::Create(_) => {}
        Operation::Remove(_) => {
            if let Some(i) = mi {
                returned_member = members.remove(i);
                changed = true;
            }
        }
        Operation::Add(r) => {
            if let Some(i) = mi {
                let same = equal(
                    &stable_binding(get(&members[i], "binding")?)?,
                    &stable_binding(r.binding)?,
                )?;
                require(same || r.rebind, "groups_rebind_required")?;
                if !same || text(get(&members[i], "role")?)? != r.role {
                    if !same
                        && optional(&members[i], "live_attachment")
                            .is_some_and(|v| !matches!(v, Value::Null))
                    {
                        members[i].set_field("live_attachment", Value::Null)?;
                        cleared = true;
                    }
                    members[i].set_field("role", string(r.role))?;
                    members[i].set_field("binding", r.binding.clone())?;
                    changed = true;
                }
                returned_member = members[i].clone();
            } else {
                let mut member = obj(&[
                    ("participant_id", string(io.uuid()?)),
                    ("key", string(r.member)),
                    ("role", string(r.role)),
                    ("binding", r.binding.clone()),
                ])?;
                if uint(get(&state, "schema_version")?)? == 3 {
                    member.set_field("live_attachment", Value::Null)?;
                }
                returned_member = member.clone();
                members.push(member);
                changed = true;
            }
            if changed
                && text(get(get(r.binding, "evidence")?, "provenance")?)?
                    == "kitsune.pane.process_info"
            {
                let version = uint(get(&state, "schema_version")?)?.max(2);
                state.set_field("schema_version", num(version)?)?;
            }
        }
        Operation::Attach(_) | Operation::Detach(_, _) => {
            let i = mi.ok_or(Error("groups_member_missing"))?;
            let participant = match &op {
                Operation::Attach(r) => r.expected_participant_id,
                Operation::Detach(_, p) => p,
                _ => unreachable!(),
            };
            require(
                text(get(&members[i], "participant_id")?)? == participant,
                "groups_participant_conflict",
            )?;
            let old =
                optional(&members[i], "live_attachment").filter(|v| !matches!(v, Value::Null));
            if let Operation::Attach(r) = &op {
                let same = old
                    .map(|v| equal(&stable_attachment(v)?, &stable_attachment(r.attachment)?))
                    .transpose()?
                    .unwrap_or(false);
                require(
                    old.is_none() || same || r.reattach,
                    "groups_reattach_required",
                )?;
                if !same {
                    if uint(get(&state, "schema_version")?)? < 3 {
                        for g in &mut groups {
                            let mut ms = array(get(g, "members")?)?.to_vec();
                            for m in &mut ms {
                                m.set_field("live_attachment", Value::Null)?;
                            }
                            g.set_field("members", Value::Array(ms))?;
                        }
                        for m in &mut members {
                            m.set_field("live_attachment", Value::Null)?;
                        }
                        state.set_field("schema_version", num(3)?)?;
                    }
                    members[i].set_field("live_attachment", r.attachment.clone())?;
                    changed = true;
                }
            } else if old.is_some() {
                members[i].set_field("live_attachment", Value::Null)?;
                changed = true;
            }
            returned_member = members[i].clone();
        }
    }
    groups[gi].set_field("members", Value::Array(members))?;
    let returned_group = groups[gi].clone();
    state.set_field("groups", Value::Array(groups))?;
    if changed {
        let next = uint(get(&state, "content_revision")?)?
            .checked_add(1)
            .ok_or(Error("groups_content_revision_overflow"))?;
        state.set_field("content_revision", num(next)?)?;
    }
    let delivery = store::finish(&guard, &state, data.as_deref(), changed, io)?;
    let mut result = obj(&[
        (
            "outcome",
            string(if changed { "committed" } else { "unchanged" }),
        ),
        ("revision", string(revision(Some(&state))?)),
        ("group", returned_group),
        ("delivery", delivery),
        ("authorization", string("none")),
        ("dispatchable", Value::Bool(false)),
    ])?;
    if is_member {
        result.set_field("member", returned_member)?;
    }
    if cleared {
        result.set_field("live_attachment_cleared", Value::Bool(true))?;
    }
    Ok(result)
}

pub fn reserve_cycle(workspace: &Path, request: ReserveRequest<'_>) -> Result<Value> {
    reserve_with(workspace, request, &store::SystemIo)
}
fn reserve_with(workspace: &Path, r: ReserveRequest<'_>, io: &impl store::Io) -> Result<Value> {
    selector(r.group)?;
    endpoint(r.endpoint)?;
    source(r.source)?;
    bounded(r.epoch, 4096, false)?;
    require(
        (1..=16).contains(&r.terminal_ids.len()),
        "groups_invalid_reservation_targets",
    )?;
    let mut targets = std::collections::HashSet::new();
    for t in r.terminal_ids {
        bounded(t, 4096, false)?;
        require(
            targets.insert(t.as_str()),
            "groups_duplicate_reservation_terminal",
        )?;
    }
    let root = store::workspace(workspace)?;
    let guard = crate::storage::WorkspaceGuard::acquire(&root, std::time::Duration::from_secs(5))?;
    store::workspace(&root)?;
    let (state, data) = store::read(&root)?;
    cas(state.as_ref(), Some(r.expected_revision))?;
    let mut state = state.ok_or(Error("groups_uninitialized"))?;
    let g = group(&state, r.group)?;
    let members = array(get(g, "members")?)?;
    let mut actual = std::collections::HashSet::new();
    for m in members {
        let p = get(get(m, "binding")?, "presentation")?;
        require(
            equal(get(p, "endpoint")?, r.endpoint)?,
            "groups_endpoint_mismatch",
        )?;
        actual.insert(text(get(p, "terminal_id")?)?);
    }
    require(actual == targets, "groups_whole_target_set_required")?;
    let mut counters = array(get(&state, "publication_counters")?)?.to_vec();
    let mut found = None;
    for (i, c) in counters.iter().enumerate() {
        if equal(get(c, "endpoint")?, r.endpoint)? && text(get(c, "source")?)? == r.source {
            found = Some(i);
            break;
        }
    }
    let i = if let Some(i) = found {
        i
    } else {
        require(counters.len() < 8, "groups_counter_limit")?;
        counters.push(obj(&[
            ("endpoint", r.endpoint.clone()),
            ("source", string(r.source)),
            ("epoch", string(r.epoch)),
            ("watermarks", obj(&[])?),
        ])?);
        counters.len() - 1
    };
    if text(get(&counters[i], "epoch")?)? != r.epoch {
        require(r.allow_new_epoch, "groups_new_epoch_required")?;
        counters[i].set_field("epoch", string(r.epoch))?;
        counters[i].set_field("watermarks", obj(&[])?)?;
    }
    let mut watermarks = get(&counters[i], "watermarks")?.clone();
    let existing = watermarks
        .as_object()
        .ok_or(Error("groups_malformed_authority"))?;
    require(
        existing.entries().len() + targets.iter().filter(|t| existing.get(t).is_none()).count()
            <= 64,
        "groups_watermark_limit",
    )?;
    let mut reservations = obj(&[])?;
    // Compute the entire cycle before publication; overflow never partially burns it.
    for t in r.terminal_ids {
        let previous = optional(&watermarks, t).map(uint).transpose()?.unwrap_or(0);
        let next = previous
            .checked_add(1)
            .ok_or(Error("groups_publication_revision_overflow"))?;
        reservations.set_field(t, num(next)?)?;
    }
    for (t, n) in reservations
        .as_object()
        .ok_or(Error("groups_malformed_authority"))?
        .entries()
    {
        watermarks.set_field(t, n.clone())?;
    }
    counters[i].set_field("watermarks", watermarks)?;
    state.set_field("publication_counters", Value::Array(counters))?;
    let delivery = store::finish(&guard, &state, data.as_deref(), true, io)?;
    obj(&[
        ("revision", string(revision(Some(&state))?)),
        ("reservations", reservations),
        ("delivery", delivery),
    ])
}

#[cfg(test)]
mod tests;
