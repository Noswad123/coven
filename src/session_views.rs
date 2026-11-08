//! Pure selection after a complete bounded observation, never a scanner shortcut.
use crate::{Result, error::require};
use serde_json::{Value, json};

pub fn validate_selection(group: Option<&str>, member: Option<&str>) -> Result<()> {
    for selector in [group, member].into_iter().flatten() {
        crate::groups::selector(selector)?;
    }
    require(member.is_none() || group.is_some(), "member_requires_group")
}

/// Input must be an original validated inspection report, not a selected view.
pub fn select_report(
    report: &Value,
    group: Option<&str>,
    member: Option<&str>,
    recognized_only: bool,
) -> Result<Value> {
    validate_selection(group, member)?;
    let mut result = report.clone();
    if group.is_none() && !recognized_only {
        return Ok(result);
    }
    result["view"] = json!({"group_key":group,"member_key":member,"recognized_only":recognized_only,
        "observed_target_count":report["target_count"],
        "observed_recognized_target_count":report["recognized_target_count"],
        "declaration_count_scope":"full_observation"});
    if report["assessment"] != "observed" {
        return Ok(result);
    }
    let matches = |a: &Value| {
        a["group_key"].as_str() == group
            && member.is_none_or(|m| a["member_key"].as_str() == Some(m))
    };
    let mut targets = Vec::new();
    for mut target in report["targets"].as_array().into_iter().flatten().cloned() {
        if recognized_only && target["recognition"] != "djinn_executable_family" {
            continue;
        }
        if group.is_some() {
            let associations: Vec<_> = target["associations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| matches(a))
                .cloned()
                .collect();
            if associations.is_empty() {
                continue;
            }
            target["associations"] = json!(associations);
        }
        targets.push(target);
    }
    result["target_count"] = json!(targets.len());
    result["recognized_target_count"] = json!(
        targets
            .iter()
            .filter(|t| t["recognition"] == "djinn_executable_family")
            .count()
    );
    result["targets"] = json!(targets);
    if group.is_some() {
        result["declarations"]["unmatched"] = json!(
            report["declarations"]["unmatched"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| matches(a))
                .collect::<Vec<_>>()
        );
    }
    Ok(result)
}

#[cfg(test)]
#[path = "session_views/tests.rs"]
mod tests;
