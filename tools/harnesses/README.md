# Focused Development Harnesses

These isolated crates exercise the real parent `src/` modules. They live outside
`src/` because Cargo omits entire nested packages from a source archive, even
when their production modules match the parent's package include patterns.
Neither harness is included in the product source package or installation.

Run from the checkout root with a populated offline dependency cache:

```sh
cargo test --offline --locked --manifest-path tools/harnesses/group_publisher/Cargo.toml --target-dir target/group-publisher-focused --lib
cargo test --offline --locked --manifest-path tools/harnesses/group_live/Cargo.toml --target-dir target/live-focused --lib live --no-fail-fast
```

Keep explicit target directories under the checkout's `target/`; existing build
outputs need not move. The live harness stages a source mirror in `OUT_DIR` and
the publisher harness generates imported module wiring there. Both remain
checkout-only development gates, including Python `-B` oracle comparisons;
neither supplies an application fallback or replaces the full regression gate.
