//! Writer-only admission. Historical reads and source-free escapes do not call this.
use crate::execution::{self, *};

// A closed cancel/revoke entry has three reason copies. JSON uses at most six
// bytes per UTF-8 input byte: 3*16384*6 = 294912. All punctuation, field names,
// request key (128), UUIDs (32), revisions (<=128), timestamp (32), generation
// (<=1024), comma and root generation growth together fit in another 8192.
// 303104 < 524288, including the ASCII-escaped spelling (more than native UTF-8
// needs). Results contain one reason and no task snapshot. With pretty spacing
// and delivery metadata they use <=6*16384+16384 <1 MiB, far below 32 MiB.
const BOUND: usize = CANCELLATION_RESERVE;
const ESCAPE_NODES: usize = 128;
const CAPACITY: &str = "unsupported_execution_escape_capacity";

pub(super) fn escape_count(view: &Validated) -> Result<usize> {
    let active = view
        .replayed
        .state
        .attempts
        .iter()
        .filter(|a| a.active())
        .count();
    let approved = if let Some(graph) = &view.graph {
        let head = get(graph, "current_graph_revision")?;
        if matches!(head, Value::Null) {
            false
        } else {
            let current = get(get(graph, "graphs")?, text(head)?)?;
            text(get(current, "current_review")?)? == "approved_for_preparation"
        }
    } else {
        false
    };
    active
        .checked_add(usize::from(approved))
        .ok_or(Error(CAPACITY))
}

/// `persisted_len` includes LF; `replay_work` is the actual fresh candidate replay
/// charge from `validated_with_work`, not a prefix-size estimate. The coordinator
/// must still admit all current output forms independently before publication.
pub(super) fn admit(
    control: &Value,
    view: &Validated,
    persisted_len: usize,
    replay_work: usize,
) -> Result<()> {
    let escapes = escape_count(view)?;
    let entries = view.replayed.receipts.len(); // Includes retained v1 in v2.
    let reserved_entries = entries.checked_add(escapes).ok_or(Error(CAPACITY))?;
    require(reserved_entries <= MAX_HISTORY, CAPACITY)?;
    let growth = escapes.checked_mul(BOUND).ok_or(Error(CAPACITY))?;
    let enlarged = persisted_len.checked_add(growth).ok_or(Error(CAPACITY))?;
    require(enlarged <= values::MAX_INPUT, CAPACITY)?;
    // preflight itself adds 32 for generation/encoding overhead. Keep its reader
    // convention, with combined history even when the root contains legacy_v1.
    execution::preflight(enlarged, reserved_entries)?;

    // Appending escapes changes no old binding, effect, result or prefix (nor
    // legacy_v1). Only the upfront root encoding grows, by at most E*BOUND.
    // Each new entry adds one prefix encoding <= enlarged+32. Cancel has two
    // result encodings; revoke also compares two decision values. Together these
    // contain <= six reason copies, each <= BOUND, with fixed fields absorbed by
    // the remaining 2*BOUND. There are no task snapshots/source-match calls in
    // either escape result. Thus 8*BOUND bounds all non-prefix Work per escape.
    // This is canonical-byte accounting, not a universal CPU/heap guarantee.
    let per_escape = BOUND
        .checked_mul(8)
        .and_then(|n| n.checked_add(enlarged))
        .and_then(|n| n.checked_add(32))
        .ok_or(Error(CAPACITY))?;
    let future_work = escapes
        .checked_mul(per_escape)
        .and_then(|n| n.checked_add(growth))
        .and_then(|n| n.checked_add(replay_work))
        .ok_or(Error(CAPACITY))?;
    require(
        future_work <= MAX_WORK,
        "unsupported_execution_work_profile",
    )?;

    // Parser nodes count values, not object keys. The closed escape schemas have
    // <128 values, depth <=5 from the root, <=7 object members, no new arrays,
    // and strings <=16 KiB (fixed metadata <=128 bytes). Existing parsed fields
    // retain their profiles; only history grows, bounded above by 1024 entries.
    let nodes = node_count(control, 0)?;
    let future_nodes = escapes
        .checked_mul(ESCAPE_NODES)
        .and_then(|n| n.checked_add(nodes))
        .ok_or(Error(CAPACITY))?;
    require(
        future_nodes <= values::MAX_NODES,
        "unsupported_json_node_profile",
    )
}

fn node_count(value: &Value, depth: usize) -> Result<usize> {
    require(depth <= values::MAX_DEPTH, "unsupported_json_depth_profile")?;
    let mut count = 1usize;
    match value {
        Value::Array(items) => {
            for item in items {
                count = count
                    .checked_add(node_count(item, depth + 1)?)
                    .ok_or(Error(CAPACITY))?;
            }
        }
        Value::Object(object) => {
            for (_, item) in object.entries() {
                count = count
                    .checked_add(node_count(item, depth + 1)?)
                    .ok_or(Error(CAPACITY))?;
            }
        }
        _ => {}
    }
    require(count <= values::MAX_NODES, "unsupported_json_node_profile")?;
    Ok(count)
}

#[cfg(test)]
mod tests;
