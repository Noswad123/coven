# Djinn process shadow: installed Linux validation

## Decision

The first prompt-free live comparison **passed without implementation changes**.
Keep the narrow executable-family rule. Do not expand wrapper recognition or
enable group adoption based on these results alone.

Observed on **2026-10-24 UTC** (October 23 local time), using Coven checkout
`10d5d4e857ab0605c6a75743dd20ae78a17d407c`. This validates installed Linux binaries,
not newly built Kitsune/Djinn checkouts or universal packaging compatibility.

## Runtime identity

| Component | Installed build |
| --- | --- |
| Kitsune | `kitsune 0.7.5`, Linuxbrew `Cellar/kitsune/0.1.0-dev` |
| Djinn | Linuxbrew `Cellar/djinn/0.1.0-dev_1`; `--version` is unsupported |
| Djinn UI | `0.0.0-local`, same Linuxbrew Djinn installation |

SHA-256 of the tested installed files:

```text
kitsune executable:
9e41a9d4e9ca71323040202de4e9350866d6892f1f159724c6f2cf1f402f1b41
djinn bin launcher:
a18c2aa7bc74606f27bc5676a9595bce19c7ed0e2a0d9cbe75a095c52a72eb97
djinn libexec executable:
20545f890d4993eef867e0f127e9d0735237c8c2c0396b9db67e248c0504b28b
djinn-ui executable:
dfaba66dff7d364b46c44ce9cbd6c60a36dbd09ac1634cc77f803fbebbe7fa40
```

The checked-out Djinn source was at `fcc7ffef1238d5c40929e083105b08cf19502c70`;
no identity equivalence between that revision and installed binaries is claimed.

## Isolation and procedure

- Created a unique disposable named Kitsune session, `repro-coven-6koyizog`.
  Read its actual socket and pane/terminal IDs from session/pane commands.
- Used only a temporary Kitsune config with `[experimental] allow_nested = true`.
  The first nested launch was rejected by the default nesting guard; no global
  configuration or detector manifest override was changed to work around it.
  The user's `~/.config/kitsune/agent-detection` override directory was absent.
- Named-session control cleared inherited socket and pane/workspace/tab overrides.
  The selected pane was refreshed after its initial locator changed during setup;
  the measured target was `w4:p1`, terminal `term_65e8db034de155`.
- Djinn probes ran in a disposable directory with separate HOME and XDG
  config/data/state/cache roots **for each launch form**. The environment was an
  explicit allowlist, not an inherited environment with a few keys removed.
- Set `DJINN_UI_BIN` to the installed UI; used `OPENCODE_TEST_HOME`,
  `OPENCODE_AUTH_CONTENT={}`, project-config disabling, pure mode, disabled model
  fetching/autoupdate/autocompact, and deny-all tool permission configuration.
  No user authentication, project configuration, or external plugin selection was
  passed through. No prompt was entered and no model request was submitted.
- For each stable process state, captured sanitized `pane.process_info`, Coven's
  three-read shadow report, and Kitsune's independent legacy classification. Also
  exercised the real `bin/coven observe kitsune --process-pane ... --json` wrapper.
- Exited each UI with its supported `ctrl+c` action and verified shell process
  evidence before moving on. A negative control was likewise interrupted.

## Observed results

| State / invocation | Generic foreground evidence | Coven result | Kitsune legacy inventory |
| --- | --- | --- | --- |
| Shell baseline | `/usr/bin/zsh` | `unknown`, no matches | Empty for target |
| Python sleep with literal `djinn` as a later argument | `/usr/bin/python3` | `unknown`, no matches | Empty for target |
| `djinn -b` | Native `djinn` plus `djinn-ui` | One pane observation, two executable matches | `djinn`, `idle` |
| `djinn` (interactive, no arguments) | Native `djinn` plus `djinn-ui` | One pane observation, two executable matches | `djinn`, `idle` |
| `djinn-ui --pure` | Direct `djinn-ui` | One pane observation, one executable match | `djinn`, `idle` |
| After native/UI exit | `/usr/bin/zsh` | `unknown`, no matches | Empty for target |

The native launcher resolves to:

```text
/home/linuxbrew/.linuxbrew/Cellar/djinn/0.1.0-dev_1/libexec/djinn
```

Its UI child uses `/home/linuxbrew/.linuxbrew/opt/djinn/bin/djinn-ui`; direct UI
launch uses `/home/linuxbrew/.linuxbrew/bin/djinn-ui`. Literal basename matching
handles all three without shell parsing, path resolution, or command-name fallback.

All shadow reports were `observed`, with `authorization: none`,
`dispatchable: false`, unknown activity/attention and unestablished native
lifecycle/job completion. In particular, Coven did **not** copy Kitsune's `idle`
classification. Process exit yielded unknown evidence, not successful completion.

## Evidence, cleanup and checks

Local sanitized captures and the exact probe/capture scripts are retained at
`/var/tmp/coven-djinn-observe-6koYIzoG/`. They are temporary local evidence, not a
repository dependency. The isolated Djinn caches remain there with the captures.

The named session was stopped and deleted; session listing then contained only
the still-running default session. All six captured non-shell probe PIDs were
absent at cleanup. The created outer pane was already absent when checked; no
replacement or unrelated pane was closed. The main server was not stopped or
restarted, and no existing user application was controlled.

The focused observation/CLI suites were rerun: **49 tests passed**. The preceding
implementation full suite passed 565 tests; it was not rerun for this docs-only
follow-up. No runtime implementation or detection rule changed.
Diff whitespace checks passed for both the tracked contract and the new report.

## Remaining limits

This does not cover Bun/Node source launchers, renamed binaries, native resume,
background/headless work, other operating systems, paid execution, task admission,
cancellation, lifecycle truth, or policy enforcement. No such support was added.
The existing sampled-mapping/ABA and freshness limitations still apply. Group
membership continues to require its existing explicit binding contract.
