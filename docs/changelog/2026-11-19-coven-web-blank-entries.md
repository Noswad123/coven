# Blank-entry presentation fix deployed, 2026-11-19

Historical record; gateway selection superseded later on 2026-11-19 by
[unified cards](2026-11-19-coven-web-unified-cards.md). [History index](index.md).

The gateway now runs `~/.local/libexec/coven-web/8b1d3f7aa9b7121f/bin/coven`
(SHA256 `8b1d3f7aa9b7121fc3a4c5872f7d626ce1895ea2de640d4b4f359dc8d3872420`).
The browser hides entries without displayable text, reports their count, preserves
real text and omission/truncation labels, and explains all-empty samples. This is
presentation-only; native projection/digest/API and sharing policy are unchanged.
Refresh the browser; **no Djinn restart is needed** for this fix. Only the gateway
was restarted, with an exact unit backup at
`~/.config/systemd/user/coven-web.service.rollback-e0857322`. Future dev Coven launches
select `coven-native-obx75bs4`; the previous payload is retained.

Validation: 56 Node tests, eight HTTP gateway tests and two native-package tests
passed; three opt-in gateway tests were not rerun for this frontend-only change.
Syntax, formatting, whitespace, offline release build, candidate and real Serve
HTTPS assets/health/metadata checks passed. Observer configuration and Serve were
unchanged; no live prompts/model calls or native restarts were performed. Owned
build/test artifacts were cleaned. No worktrees, source commits or pushes were
created for this fix.
