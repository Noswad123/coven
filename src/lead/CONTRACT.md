# Lead And Presentation Integration

Owned files are `src/lead.rs`, `src/lead/`, `src/runtime/presentation.rs`, and
`src/runtime/presentation/`. Existing files are intentionally not edited.

Parent wiring required:

- Declare `pub mod lead;` in `src/lib.rs`.
- Declare `pub(crate) mod presentation;` in `src/runtime.rs` (or public if external
  library clients need the presentation entry points).
- Make `runtime::transport::Backend::pane` `pub(super)` so its sibling presentation
  module can observe panes. No roster requirement belongs in this lookup.
- Route CLI and console through these same APIs. Print `body` to stdout and lead
  `warnings` individually to stderr; use `exit_code`. Append one printing newline
  without stripping content. Standup's body preserves its intrinsic final newline,
  so printing it ends with two newlines, exactly like Python `print(render_standup(...))`.

Public entry points:

```rust
runtime::presentation::standup(workspace: &Path) -> Result<runtime::CommandResult>
runtime::presentation::agent_status(workspace: &Path, agents: &[String])
    -> Result<runtime::CommandResult>
lead::command(workspace: &Path, operation: lead::Operation, message: Option<&str>)
    -> Result<lead::CommandResult>
```

`Operation` is `Show`, `Set(String)`, or `Clear`. Lead results contain `body: String`,
`warnings: Vec<String>`, and `exit_code: u8`.

Legacy argument mapping, after optional-workspace splitting: `--clear` overrides
all operands; no operands, `show`, and `status` select Show; `clear`, `remove`,
and `unset` select Clear. Otherwise `set AGENT` selects its second operand and a
bare first operand selects that exact ID. A lone `set` is the literal ID `set`,
not a special arity error. Legacy extra operands are ignored. Pass `--message`
through unchanged; absent or empty messages use the exact default prompt.

Every public call reloads the full workspace. Read-only observation holds shared
transport admission and checks current control before constructing a backend.
Approved mode returns unverified without any backend construction. Historical
lead and persisted state-row IDs need not belong to the current roster. Explicit
agent-status selections must all be roster IDs before any backend construction.

Lead mutations preflight operands/goal before workspace exclusion and reload
under exclusion. Set preserves extension fields and uses UTF-8 atomic storage.
Clear unlinks and fsyncs the parent, never writes JSON null. Known changes with
uncertain durability return exit 1 and skip every secondary/notice effect.
Committed secondary failures warn separately and continue with exit 0. Message
history is appended once before unlocking; notice delivery uses `send_notice`,
not public send. Approved mode permits metadata and suppresses delivery. A mode
activation racing with notice is a warning, never rollback or a legacy fallback.

Focused tests use injected metadata and I/O or Python `-B` temporary-workspace
oracles. They never invoke a real backend or provider. Existing replacement
storage tests own mode preservation and pre/post-rename filesystem faults; lead
tests characterize domain behavior at those typed result boundaries and actual
unlink/parent-sync uncertainty.
