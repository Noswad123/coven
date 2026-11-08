use super::*;

pub(super) const DASHBOARD: &str = "while true; do clear; cat dashboard.md; sleep 2; done";

pub(super) fn tmux(
    root: &Path,
    ns: &str,
    agents: &[(String, String)],
    monitors: &[(&str, String)],
    backend: &transport::Backend<'_>,
) -> Result<()> {
    let cwd = crate::read_fs::path_text(root)?;
    let orchestrator = format!("{ns}:orchestrator");
    checked(
        backend,
        &[
            "new-session",
            "-d",
            "-s",
            ns,
            "-n",
            "orchestrator",
            "-c",
            &cwd,
            DASHBOARD,
        ],
    )?;
    backend.capture(&[
        "select-pane",
        "-t",
        &format!("{orchestrator}.0"),
        "-T",
        "dashboard",
    ])?;
    for (title, command) in monitors {
        let pane = backend.capture(&[
            "split-window",
            "-t",
            &orchestrator,
            "-c",
            &cwd,
            "-P",
            "-F",
            "#{pane_id}",
            command,
        ])?;
        require(pane.code == 0, "runtime_backend_command_failed")?;
        let pane = crate::records::trimmed(&pane.stdout);
        if !pane.is_empty() {
            require(
                pane.starts_with('%')
                    && pane.len() > 1
                    && pane[1..].bytes().all(|v| v.is_ascii_digit()),
                "invalid_runtime_handle",
            )?;
            backend.capture(&["select-pane", "-t", pane, "-T", title])?;
        }
    }
    backend.capture(&["select-layout", "-t", &orchestrator, "tiled"])?;
    for (agent, command) in agents {
        checked(
            backend,
            &[
                "new-window",
                "-t",
                ns,
                "-n",
                &format!("agent-{agent}"),
                "-c",
                &cwd,
                command,
            ],
        )?;
    }
    backend.capture(&["select-window", "-t", &orchestrator])?;
    Ok(())
}

pub(super) fn herdr(
    root: &Path,
    ns: &str,
    agents: &[(String, String)],
    monitors: &[(&str, String)],
    backend: &transport::Backend<'_>,
) -> Result<String> {
    let cwd = crate::read_fs::path_text(root)?;
    let result = payload(
        backend,
        &[
            "workspace",
            "create",
            "--cwd",
            &cwd,
            "--label",
            ns,
            "--no-focus",
        ],
    )?;
    // Validate all returned handles before using any of them, including optional
    // tab handles. Missing optional panes retain Python's skip behavior.
    let workspace = nested_handle(&result, "workspace", "workspace_id")?
        .ok_or(Error("missing_runtime_handle"))?;
    let pane =
        nested_handle(&result, "root_pane", "pane_id")?.ok_or(Error("missing_runtime_handle"))?;
    let tab = nested_handle(&result, "tab", "tab_id")?;
    if let Some(tab) = &tab {
        backend.capture(&["tab", "rename", tab, "orchestrator"])?;
    }
    backend.capture(&["pane", "rename", &pane, "dashboard"])?;
    backend.capture(&["pane", "run", &pane, DASHBOARD])?;
    for (title, command) in monitors {
        let result = payload(
            backend,
            &[
                "pane",
                "split",
                "--pane",
                &pane,
                "--direction",
                "right",
                "--ratio",
                "0.5",
                "--cwd",
                &cwd,
                "--no-focus",
            ],
        )?;
        if let Some(id) = nested_handle(&result, "pane", "pane_id")? {
            backend.capture(&["pane", "rename", &id, title])?;
            backend.capture(&["pane", "run", &id, command])?;
        }
    }
    for (agent, command) in agents {
        let result = payload(
            backend,
            &[
                "tab",
                "create",
                "--workspace",
                &workspace,
                "--cwd",
                &cwd,
                "--label",
                &format!("agent-{agent}"),
                "--no-focus",
            ],
        )?;
        if let Some(pane) = nested_handle(&result, "root_pane", "pane_id")? {
            backend.capture(&["pane", "rename", &pane, agent])?;
            backend.capture(&["pane", "run", &pane, command])?;
        }
    }
    if let Some(tab) = &tab {
        backend.capture(&["tab", "focus", tab])?;
    }
    Ok(workspace)
}

pub(super) fn agent(
    root: &Path,
    ns: &str,
    agent: &str,
    command: &str,
    multiplexer: &str,
    backend: &transport::Backend<'_>,
) -> Result<()> {
    let cwd = crate::read_fs::path_text(root)?;
    if multiplexer == "tmux" {
        require(
            backend.capture(&["has-session", "-t", ns])?.code == 0,
            "runtime_session_not_running",
        )?;
        return checked(
            backend,
            &[
                "new-window",
                "-t",
                ns,
                "-n",
                &format!("agent-{agent}"),
                "-c",
                &cwd,
                command,
            ],
        );
    }
    let workspace = backend
        .workspace(ns)?
        .ok_or(Error("runtime_workspace_not_running"))?;
    let id = required_handle(&workspace, "workspace_id")?;
    let result = payload(
        backend,
        &[
            "tab",
            "create",
            "--workspace",
            &id,
            "--cwd",
            &cwd,
            "--label",
            &format!("agent-{agent}"),
            "--focus",
        ],
    )?;
    let pane =
        nested_handle(&result, "root_pane", "pane_id")?.ok_or(Error("missing_runtime_handle"))?;
    backend.capture(&["pane", "rename", &pane, agent])?;
    backend.capture(&["pane", "run", &pane, command])?;
    Ok(())
}
