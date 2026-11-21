# Agent Directory Isolation

Legacy team launch and individual agent start/restart can opt into explicit,
pre-created source checkouts. Coven never creates/deletes worktrees, switches
branches, or moves existing panes. This is directory isolation, **not a security
sandbox**: agents still have the caller's filesystem access and permissions.

In `coven.json`, add:

```json
{
  "name": "example",
  "agents": ["implementer", "reviewer"],
  "runtime": {
    "agent_working_directories": {
      "implementer": "/home/me/checkouts/implementation",
      "reviewer": "/home/me/checkouts/review"
    }
  }
}
```

Alternatively, `coven.toml` overrides the entire JSON mapping (not individual
entries):

```toml
[runtime.agent_working_directories]
implementer = "/home/me/checkouts/implementation"
reviewer = "/home/me/checkouts/review"
```

The mapping must cover exactly the roster, even for a single-agent start. Empty,
partial, extra-ID, non-string, relative, missing and non-directory entries are
errors. Every path is canonicalized and must equal an existing non-bare Git
working-tree root. Subdirectories do not qualify. Canonical aliases cannot give
two agents the same root. The control workspace and any checkout containing it
are forbidden, as are roots aliasing the same per-worktree Git directory (index).
Distinct independent Git checkouts qualify as well as linked Git
worktrees; sharing a common Git object store is allowed. No branch ownership,
cleanliness, ancestry, or revision guarantee is implied.

Git inspection uses the existing bounded conservative probe runner with a closed
rev-parse grammar, sanitized environment, disabled hooks/fsmonitor, no optional
locks and no transport. Caller PATH selects the trusted Git executable; repository
configuration does not select a helper. Transport subprocesses still retain the
caller's cwd/PATH contract; agent cwd is an explicit backend argument.

General workspace loads validate the mapping's schema, exact roster coverage and
absolute path syntax without probing execution directories. Missing/unmounted
checkouts do not block shutdown, individual stop, messages or standup. Live
full-roster Git/root validation remains mandatory for refresh, start, restart and
bootstrap, before first locks or launch effects, and is repeated during preparation.
Python team launch also stages all starters before requested history and revalidates
under transport admission. Reviewed-v1 preparation refuses the configured mapping
without requiring live checkouts.

Cwds and commands are
staged together. Filesystem observations are not race-free leases: do not rename
or retarget checkouts while launching.

Run `coven refresh /absolute/control/path` after configuring the mapping. Generated
starters must match the current mapping before launch or bootstrap; stale/missing
starters refuse with `agent_working_directories_refresh_required` before effects.
This avoids sending old relative-path guidance into a newly isolated checkout.
Generated tmux agent panes and direct tmux/Herdr launches use the same canonical cwds.
Starter files and role prompts remain under the control root; isolated startup
loads the starter via an absolute, shell-quoted path. Refreshed starters explicitly
distinguish execution source work from control artifacts and demonstrate quoted
`cd` wrappers for coordination CLI calls. Existing custom role prompts are not
overwritten; starter location rules take precedence. Monitors, dashboard, tasks,
messages, checkpoints, work/output handoffs and prompts stay in the control root.
An already running pane is never silently relocated: explicitly restart it when
ready. Without the mapping, legacy cwd and startup behavior is retained.

Reviewed `runtime_launch` v1 cannot represent this mapping. Preparation explicitly
refuses with `runtime_launch_v1_agent_directories_unsupported`, before publishing
any bundle or checkpoint. No frozen v1 plan meaning is changed; this feature is
not available on that launch path. Python runner changes are development oracle
parity only, not a production Python execution path.
