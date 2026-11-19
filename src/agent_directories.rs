//! Explicit source-checkout isolation, not a security sandbox or worktree manager.
use crate::{Error, Result, error::require, records::TeamRecord, values::Value};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

pub(crate) fn configured(team: &TeamRecord) -> Result<Option<&Value>> {
    let Some(runtime) = team.value().as_object().and_then(|v| v.get("runtime")) else {
        return Ok(None);
    };
    Ok(runtime
        .as_object()
        .ok_or(Error("invalid_runtime_configuration"))?
        .get("agent_working_directories"))
}

/// Pure schema validation must not make shutdown or coordination depend on mounts.
pub(crate) fn validate(team: &TeamRecord) -> Result<BTreeMap<String, PathBuf>> {
    let Some(value) = configured(team)? else {
        return Ok(BTreeMap::new());
    };
    let entries = value
        .as_object()
        .ok_or(Error("invalid_agent_working_directories"))?;
    let roster = team.agent_ids()?;
    require(
        entries.entries().len() == roster.len()
            && roster.iter().all(|agent| entries.get(agent).is_some()),
        "agent_working_directories_roster_mismatch",
    )?;
    let mut result = BTreeMap::new();
    for agent in roster {
        let text = entries
            .get(agent)
            .and_then(Value::as_str)
            .ok_or(Error("invalid_agent_working_directory"))?;
        let path = Path::new(text);
        require(
            path.is_absolute() && !text.contains(['\0', '\n', '\r']),
            "invalid_agent_working_directory",
        )?;
        result.insert(agent.to_owned(), path.to_owned());
    }
    Ok(result)
}

pub(crate) fn resolve(root: &Path, team: &TeamRecord) -> Result<BTreeMap<String, PathBuf>> {
    let configured = validate(team)?;
    if configured.is_empty() {
        return Ok(configured);
    }
    let control = root.canonicalize()?;
    let control_git_dir = crate::repositories::working_tree_paths(&control)
        .ok()
        .map(|(_, git)| git);
    let mut seen = HashSet::new();
    let mut git_dirs = HashSet::new();
    let mut result = BTreeMap::new();
    for (agent, path) in configured {
        let path = path
            .canonicalize()
            .map_err(|_| Error("invalid_agent_working_directory"))?;
        require(
            path.is_dir() && path != control && !control.starts_with(&path),
            "invalid_agent_working_directory",
        )?;
        let (git, git_dir) = crate::repositories::working_tree_paths(&path)
            .map_err(|_| Error("invalid_agent_working_directory"))?;
        require(
            git == path && control_git_dir.as_ref() != Some(&git_dir),
            "invalid_agent_working_directory",
        )?;
        require(
            seen.insert(path.clone()) && git_dirs.insert(git_dir),
            "duplicate_agent_working_directory",
        )?;
        result.insert(agent, path);
    }
    Ok(result)
}

pub(crate) fn validate_starters(root: &Path, team: &TeamRecord) -> Result<()> {
    for (agent, cwd) in resolve(root, team)? {
        let expected = crate::workspace_assets::starter_text_at(
            &agent,
            &crate::read_fs::path_text(root)?,
            Some(&crate::read_fs::path_text(&cwd)?),
        );
        let actual = crate::workspace::optional_bytes(
            root,
            &PathBuf::from(format!("prompts/{agent}.start.txt")),
            crate::workspace::MAX_TEXT,
            "unsupported_runtime_prompt_profile",
        )?;
        require(
            actual.as_deref() == Some(expected.as_bytes()),
            "agent_working_directories_refresh_required",
        )?;
    }
    Ok(())
}
