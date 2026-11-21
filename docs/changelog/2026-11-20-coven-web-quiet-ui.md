# Quiet UI, header alerts toggle and latest-first positioning, 2026-11-20

Historical record; gateway selection was superseded later on 2026-11-20 by
[title-prefix cleanup](2026-11-20-coven-web-title-prefix.md). [History index](index.md).

The frontend polish is deployed. Routine verified-link/no-linked-request text,
healthy native metadata explanations, host-health chips and the static coverage
paragraph are removed. Actual errors, unavailable empty hosts, permission scope
warnings and uncertain-delivery notices remain visible only when relevant.

The old Private badge is now one accessible header alerts toggle, reflecting
on/off/unknown/checking without a healthy helper paragraph. Passive inspection and
opt-in/opt-out semantics are preserved; server-cleanup uncertainty has an exception-
only action. The existing phone subscription and VAPID key were unchanged.

Chat opens at latest content/reply controls, or at an explicitly opened request's
meaning. One outer message scroll context allows scrolling up for history without
metadata/late-response jumps. The sticky heading clearance is measured dynamically,
including long wrapped titles; no approval control is automatically focused. Scroll
effects recheck view, request, interaction and authorization generations.

Gateway: `~/.local/libexec/coven-web/3fde7b16ed6e4e65/bin/coven`, SHA256
`3fde7b16ed6e4e6553c36817b031bc2ae2247d5d468a07924b2e282ab64f2ce9`.
Future dev selection is `coven-native-tcf57kxp`. Exact previous unit backup:
`~/.config/systemd/user/coven-web.service.rollback-a01e9a08`.

127 Node tests including real Firefox passed at phone/desktop widths, with long
history, 320px header fit, both message/request response orders, wrapped permission
heading clearance, stale-callback and focus checks. Eight HTTP and two package
tests passed; five producer opt-ins were not rerun for this UI-only change. Syntax,
formatting, release build and actual Serve HTTPS asset/metadata checks passed.
Physical-phone keyboard behavior remains device acceptance, not a Firefox claim.

Only the gateway restarted. Native agents, backend contracts, access policy,
configuration, Serve, subscription bindings (one before/after) and private key
were preserved. No live prompt, decision or vendor test push was submitted. All
owned browser/build/test artifacts were cleaned; no worktrees, commits or pushes
were created. Source changes are in the main checkout at `b5c9ece`, uncommitted.
Refresh the phone page; no native relaunch or alert re-enablement is required.
