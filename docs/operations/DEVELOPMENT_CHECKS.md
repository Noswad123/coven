# Development Checks

Run checks from the repository root. The product is the native Rust `coven`
binary; retained `runner/` Python is a development test oracle, never a runtime
fallback. These are source-check recipes, not installation, deployment, provider
activation or permission to exercise existing user sessions.

## Check recipes

The [justfile](../../justfile) defines the practical aggregate:

| Recipe | Command / scope |
| --- | --- |
| `just fmt` | `cargo fmt --all -- --check` (read-only formatting check) |
| `just lint` | `cargo clippy --offline --locked --all-targets -- -D warnings` |
| `just test-rust` | `CARGO_BUILD_JOBS=2 cargo test --offline --locked` |
| `just test-python` | `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q` |
| `just test-web` | `node --test clients/web/*.test.mjs` |
| `just test` | Rust, Python and Node tests |
| `just check` | Formatting, lint and all three test recipes |

`just format` **changes** Rust formatting; it is not `just fmt`. `just lock`
regenerates the lockfile offline and is not routine validation. The verbose Rust
recipe adds `-- --nocapture`; use it selectively rather than keeping unbounded logs.

### Build and test prerequisites

- Rust/Cargo with rustfmt and Clippy; `Cargo.toml` declares edition 2024 and
  Rust **1.85** as the minimum, not a pinned local toolchain. Use the repository's
  lockfile. MSRV checking is a separate gate from testing with a newer compiler.
- A native C toolchain for dependencies such as `ring`, plus OpenSSL development
  headers/libraries and appropriate discovery tools. macOS builds compile the real
  SDK ACL source through `build.rs`; Rust-target typechecks alone do not validate
  SDK C, linking, APFS behavior or native execution.
- Python for checkout regressions, fixtures and development tools. The retained
  migration oracle is qualified against **Python 3.14.8**; the old README's 3.11+
  statement is not equivalent to that full-oracle qualification. The installer
  helper also uses Python at build time only.
- Node **20+** for synthetic web tests. No `npm install`, browser framework or
  client build is needed. `node --check clients/web/app.js` is an additional syntax
  check, not part of `just check`.
- Local POSIX locking for filesystem workflows on Linux/macOS. Some fixture
  layers use local Git or compiled inert backends; see the selected test's source
  before treating a narrowly named test as process-free.

Build dependencies are not runtime prerequisites. Installed `coven` does not
invoke Cargo, Python or Node; a dynamically linked build still needs its selected
native libraries. Runtime operations can independently require the chosen
multiplexer, Git or a trusted provider executable. Tests using fakes do not prove
installed-provider compatibility.

For OpenSSL discovery and explicit overrides, see
[Development OpenSSL Selection](WEB_PUSH_BUILD.md). The distro-OpenSSL preference
belongs to `scripts/install_dev.py` / `just install`, **not** ordinary `cargo` or
`just check` invocations. It respects explicit configuration and does not install
dependencies or arrange a custom runtime loader.

## What the checks establish

Formatting checks source; Clippy checks all Cargo targets; Rust tests exercise
native units/integrations and may invoke development oracle fixtures. Python
discovery retains the legacy assertions separately. Node tests use synthetic
client fixtures, not live APIs or native agent sessions. A passing aggregate is
not browser, phone, authentication, deployment or provider-activation acceptance.
Use the separate [browser acceptance checklist](../../clients/web/README.md#browser-acceptance)
only with an operator-approved private deployment.

Offline Rust recipes require the locked crates and toolchain to be cached already.
Missing caches are a prerequisite failure, not a reason to drop `--locked`, rewrite
the lockfile, suppress warnings or silently fetch dependencies. Any necessary
fetch/toolchain setup should be an explicitly approved separate step. Reuse shared
Cargo registry/toolchain caches; do not download a fresh toolchain per run.

## Running checks safely

### Scope and artifacts

- Documentation-only changes normally need source/link/whitespace review, not
  builds, dependency installation or full suites. Code changes should start with
  the smallest relevant existing target; broaden checks proportionally.
- Before a substantial run, verify the filesystem and free space under
  `${XDG_CACHE_HOME:-$HOME/.cache}/agent-test-runs`. It must be disk-backed; do not
  assume `$HOME` or a cache path guarantees that. Create one uniquely owned run
  directory there only after checking capacity.
- Scope `TMPDIR` and `CARGO_TARGET_DIR` to that run and its children. Keep bounded
  logs/fixtures there too. `TMPDIR` alone does not redirect Cargo outputs, nested
  harnesses, hardcoded `/tmp` paths or every tool's cache. Inspect selected harness
  settings first; report any output that cannot be safely redirected.
- Do not put bulk fixtures, build trees or logs in `/tmp`, `/tmp/opencode` or
  `/dev/shm`. Shared/RAM-backed storage exhaustion is not a semantic test failure.
- Establish fixture teardown, `try/finally` or exit/signal cleanup **before** work
  starts. Stop and reap owned children before removing only the exact owned run
  directory on success, failure or catchable interruption; preserve the test exit
  status. Never delete shared caches, other runs, session history or active locks.
- SIGKILL/power loss may bypass cleanup. Check ownership and active use before
  removing stale artifacts, not just age. Report cleanup failures. Retain only
  bounded, genuinely necessary debugging evidence with its path, reason and cleanup
  plan. Installed payload generations are not disposable test artifacts.

### Separate gates

`just check-retirement-coverage` validates the frozen obligation ledger and prints
its summary. `just require-retirement-complete` additionally refuses unresolved
obligations and is intentionally red while they remain. Neither is part of
`just check`; Python source removal is not implied by native route representation.
See [retirement requirements](../plans/RUST_RETIREMENT.md) and
[assertion/provenance rules](../../tests/retirement/README.md).

Never add old milestone totals to current runs or count nested helpers twice.
Record the actual toolchain, command, scope, failures/reruns and platform with any
new result. Historical Linux passes do not qualify native macOS or a fresh full
suite. This extraction ran no tests or builds and makes no new pass claim.

## Choose a proportional check

After prerequisites, disk capacity, scoped output locations and cleanup have been
established, select the needed existing recipe from the repository root:

```sh
just fmt
just test-web
node --check clients/web/app.js
# Broader gate when justified:
just check
```

These are instructions, not commands executed for this documentation change.
For building/installing a checkout or staging an existing binary, use the single
[packaging guide](../../packaging/README.md), including
[prebuilt staging](../../packaging/README.md#install-a-prebuilt-binary); installation
is not a validation step to run against a user's selection without approval.

## Related documentation and source

- [Cargo.toml](../../Cargo.toml), [build.rs](../../build.rs): toolchain, dependencies,
  package inputs and macOS SDK compilation.
- [justfile](../../justfile): authoritative recipe commands and composition.
- [clients/web/README.md](../../clients/web/README.md): synthetic tests vs browser acceptance.
- [scripts/install_dev.py](../../scripts/install_dev.py): checkout-only build helper.
- [Rust migration evidence](../plans/RUST_MIGRATION.md): historical counts and retry limits.
- [README evidence map](../changelog/readme-27cc6ae-evidence.md): original source ranges.
