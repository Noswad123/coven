# coven

`coven` is a personal, magic-themed multi-agent workspace command. It was
inspired by the D4RT `team` command, but it is intentionally allowed to diverge:
features should be documented here and ported manually only when they are useful
for this tool.

The source of truth for a coven workspace is local, inspectable files. The
command creates those files, keeps lightweight state, and launches agent windows
through the existing `d4rt tmux` runner.

## Install

Homebrew tap install:

```bash
brew tap Noswad123/jamal-arcana
brew install coven
```

Or install directly from the tap:

```bash
brew install Noswad123/jamal-arcana/coven
```

The tap formula currently tracks `https://github.com/Noswad123/coven` on the
`main` branch. Publish this repository there before using the tap from another
machine. After the first tagged release, update the formula to a tag tarball and
SHA like the other Jamal Arcana tools.

The Homebrew formula installs `bin/coven`, `runner/`, and `examples/` together
under Homebrew's Cellar so the wrapper can find the Python runner and example
seeds.

Local development can still use the repository wrapper directly:

```bash
/Users/jdawson/Projects/coven/bin/coven --help
```

## Quick start

```bash
/Users/jdawson/Projects/coven/bin/coven init ~/Projects/new-coven
cd ~/Projects/new-coven
/Users/jdawson/Projects/coven/bin/coven plan
/Users/jdawson/Projects/coven/bin/coven up
```

For a shorter command, add this project's `bin/` directory to `PATH` or symlink
`bin/coven` into `~/.local/bin`.

## Project layout

This repository currently uses a flat standalone layout:

```text
coven/
├── bin/
│   └── coven              # executable wrapper
├── runner/
│   └── coven.py           # main CLI and workspace runner
├── examples/
│   ├── README.md          # example authoring notes
│   └── interactive-demo/  # default seeded workspace example
└── README.md
```

There is no `tools/coven/` tree in this project.

## Generated workspace layout

`coven init` creates a file-backed workspace with this shape:

```text
new-coven/
├── coven.json                  # workspace metadata
├── coven.toml                  # coven agents/checkpoints/runtime config
├── goal.md                     # shared goal
├── spec.md                     # example/spec notes when seeded
├── dashboard.md                # human-readable state projection
├── agents/                     # per-agent role and notes files
├── prompts/                    # generated agent prompts and starter prompts
├── state/
│   ├── agents.json
│   ├── tasks.json
│   └── lead.json               # present when a lead is set
├── logs/
│   ├── events.jsonl            # append-only event log
│   └── messages.jsonl          # append-only message log
├── checkpoints/
│   ├── plan-approval.md
│   └── pre-delivery-review.md
├── work/                       # in-progress artifacts
├── output/                     # final/demo artifacts
└── tmux/
    └── coven.toml              # manifest passed to d4rt tmux
```

## Examples

By default, `init` seeds the `interactive-demo` example:

```bash
coven init ~/Projects/new-coven --example interactive-demo
```

Create an empty workspace instead:

```bash
coven init ~/Projects/new-coven --blank --agents orchestrator,architect,implementer,reviewer
```

Starter examples live under `examples/`. See `examples/README.md` for the
example directory format.

## Common commands

```bash
coven status       # render and print dashboard.md
coven next         # show the recommended next user action
coven standup      # summarize agents, tasks, checkpoints, and lead
coven events       # print event log, or use --raw for JSONL
coven messages     # print message log, or use --raw for JSONL
coven refresh      # regenerate generated prompts and tmux manifest
coven bootstrap    # paste starter prompts into running agent panes
```

## User interaction

```bash
coven review
coven console
coven approve plan-approval
coven suggest pre-delivery-review "Please include QA evidence before final approval."
coven send jamal "Please continue core-chatbot and update the logs."
coven send all "Pause and summarize current status."
```

`coven review` walks pending checkpoint files and lets you approve, request
changes, skip, or quit. `coven console` is a small interactive shell for status,
standup, agents, send, approve, suggest, and review operations.

## Tmux lifecycle

`coven` delegates tmux launching to `d4rt tmux` using the generated
`tmux/coven.toml` manifest.

```bash
coven plan
coven up
coven up --replace
coven down
coven restart

# aliases
coven start
coven stop
```

Target a specific agent window:

```bash
coven agent status jamal
coven agent send jamal "Please summarize your blocker."
coven agent stop jamal
coven agent start jamal
coven agent restart jamal
```

## Coven lead

Set one agent to lead until the goal is achieved or a critical blocker requires
the user:

```bash
coven lead srikanth
coven lead
coven lead clear
```

The lead is recorded in `state/lead.json`, shown in the dashboard/standup,
logged, and sent to the lead's tmux pane when it is running.

## Relationship to D4RT `team`

`coven` is no longer trying to mirror the D4RT `team` command or keep the same
file structure. Treat it as a separate personal tool:

- document coven features in this README as they are added;
- manually port useful ideas from D4RT `team` when desired;
- keep coven-specific naming, files, examples, and workflow choices here;
- avoid assuming feature parity unless it is explicitly documented.

## License

MIT © Jamal Dawson
