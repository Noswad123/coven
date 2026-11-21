# Complete supported titles deployed, 2026-11-21

Historical deployment evidence, not a live-state probe. See [dated deployment state](../operations/COVEN_WEB_DEPLOYMENT_STATE.md)
and [history index](index.md). Source-state assertions below describe this rollout;
the later documentation-split baseline is Coven `aea93a6`, with full titles committed.

Coven now forwards complete supported titles through 4096 UTF-8 bytes and renders
them on the third card line, below the short pane name and plain provider. It does
not slice titles or add ellipses; literal trailing dots remain intact. Title-only
bounds do not widen other display fields, wire response budgets, or authorization.
Invalid/oversized candidates within the wire profile have explicit unavailable
labels; the existing global wire guard can reject larger inputs before projection.
Firefox checks cover complete 4096-byte ASCII/emoji titles at 320px, 390px and
desktop widths without clipping or horizontal overflow.

Gateway and matching Megatron helper use immutable payload
`~/.local/libexec/coven-web/5e3b99d14e57a1b8/bin/coven`, SHA256
`5e3b99d14e57a1b8f39c2e5a80840dab22ba0b1350061e5ffbdf2a53e325dc64`.
Future dev Coven selection is `coven-native-rr9vz35i`. Private provenance and
rollout reports are retained beside the payload. Remote hash, x86-64 ABI, default
OpenSSL 3 loader, help and bounded-inventory checks passed. Candidate and live
loopback/Serve HTTPS health, assets and metadata passed. Both endpoints were
reachable/observed; the final sample had one local agent with a manual pane name
and zero remote agents. These are dynamic counts, not a guarantee of coverage.

For paired rollback, atomically restore both exact private backups before reloading
the user manager and restarting only `coven-web.service`:

- `~/.config/systemd/user/coven-web.service.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`
- `~/.config/coven-web/observer.json.full-title-rollback-20261121T092445-5e3b99d14e57a1b8`

They restore the previous paired gateway/helper pin `75bb7f3a5b8b0cc5`; old
payloads remain retained. ACLs, native configuration, Serve, push key and the one
phone subscription were preserved. No live message, decision or push was sent.

Local Djinn future launches now select managed UI bundle
`cfc7e72931097d77b6ab5e3339247e084a6116b78260173e372a1cb5bb8e75af`.
Named sessions pass their complete stored title to the supported renderer API,
without the old `OC | ` prefix or 40-character formatter. The native CLI is
byte-identical (SHA256
`4d7f8f1ebd49cab7f16eb145185eda18d9d53281521060856ac4c560ee9c98b4`).
The preceding matched bundle
`08a998a3848af9d86eb4882604969787ff5bea49826d3ec4b66e2f19c1f9d106`
remains intact for an atomic `.djinn/current` selection rollback. Existing
dependencies and the models snapshot were reused without installation/downloads.

Only the gateway restarted. Running Djinn/Kit/native processes were not signaled,
and Megatron's native UI was not updated. The active local metadata still emitted
the old 45-byte decorated/truncated title: discarded suffixes cannot be recovered
by the website. Finish current native work before deliberately relaunching the
updated local UI. Megatron needs its normal native update separately. OpenTUI
0.4.5 still limits native title transport to roughly 1019 UTF-8 bytes; the 4096-byte
Coven contract is not an end-to-end native title guarantee. No raw OSC/private API
workaround was introduced.

Validation passed: 135 Node tests (real Firefox, no skips), 156 observer tests,
eight gateway tests (five opt-ins ignored), two packaging tests, offline locked
all-target check/release build, formatting and whitespace. Native validation passed
234 TUI tests (one skipped), package typecheck and compiled/staged capability
probes. The broader library suite was not rerun: earlier unrelated failures in
`djinn_observation::tests::controlled_real_producer_supervisor_and_source_independence`
and `integration_runtime::unix::tests::normal_slow_commit_acknowledges_after_commit_without_retry`
remain outside this passing focused gate. Physical-phone/interactive-native title
acceptance was not performed.

Source integrity was checked across both builds/publications. No source commits,
staging, pushes or worktrees were created. All owned disk-backed run/build/browser
artifacts were cleaned after child shutdown; the unattributed `pn.HC9lGV` directory
and other unowned data were left untouched. Source remains uncommitted on Coven
`2d1fb8d` and Djinn `c9398c4`.
