# Example Project: Interactive Coven Demo Agent

## Goal

Build an interactive terminal demo agent for this Coven workspace. The final app should run with `python3 output/demo.py`, read the workspace artifacts the coven creates, and answer questions about what was built, who did what, why decisions were made, which checkpoints were approved, and what QA verified. It should work with no setup and may optionally use a free/local model.

## Desired behavior

When the app runs, it should start an interactive terminal session that can answer questions about the project and the collaborating agents. It should read context from the workspace files the coven creates, including `goal.md`, `dashboard.md`, `state/tasks.json`, `checkpoints/*.md`, `work/*.md`, and `output/*.md`.

The demo agent should be able to answer questions like:

- `who did what?`
- `what was built?`
- `why did you choose Python?`
- `ask Eric what QA verified`
- `ask Kris about the UI`
- `what checkpoints happened?`
- `what files should I review?`

The app should speak about these coven members with role-specific flavor:

- Clippy — planner
- Srikanth — tech lead
- Jamal — senior developer
- Kris aka Dr Oom — frontend/UI
- Eric — QA

## Model behavior

- Default behavior must work with no external dependencies or API keys.
- Optional free/local model mode is allowed, preferably via Ollama, but must have a deterministic fallback if the model is unavailable.

## Acceptance criteria

- Final runnable command is `python3 output/demo.py`.
- The app starts an interactive prompt and supports `help` and `exit`.
- Answers are grounded in workspace files rather than only hardcoded strings.
- The app can answer who did what, what was built, why the architecture was chosen, what checkpoints happened, what QA verified, and what files matter.
- The terminal output is readable and intentionally formatted.
- QA records representative questions, command output, and whether expectations passed.
- The coven consults the user at the planning checkpoint before implementation and at the pre-delivery checkpoint before calling the work complete.
