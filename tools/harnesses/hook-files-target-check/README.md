# Hook-file Rust target check

This development-only harness imports Coven's actual `hook_files.rs`, its platform
modules, `private_access.rs`, the agent-only filesystem, shared delivery type and
their tests. It does not copy their implementations or enter the
installed/source-package application boundary.

With the relevant Rust target standard libraries installed:

```text
cargo check --offline --manifest-path tools/harnesses/hook-files-target-check/Cargo.toml --target aarch64-apple-darwin --tests
cargo check --offline --manifest-path tools/harnesses/hook-files-target-check/Cargo.toml --target x86_64-apple-darwin --tests
```

These commands check **the imported Rust source types for the actual target**. The
full receiver/RPC module is not imported or validated by this harness. They do not build
the SDK-compiled C ACL adapter, link a binary, run tests or exercise macOS/APFS.
The ordinary application build, including `build.rs`, remains required on macOS
with an Apple SDK. No synthetic `target_os` flags or mock ACL implementation may
substitute for that native build. Linux C-mock execution belongs to the ordinary
Coven `hook_files` tests, not this harness.
