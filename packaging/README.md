# Native Packaging

## Overview

The selected scope is **native production cutover with Python source retained as a
development oracle**, not full source deletion or macOS closure. All 78 original
routes are represented, not certified as full compatibility. The checkout's
`bin/coven` is a native-only shell launcher; installed `bin/coven` is the Rust
executable itself. Neither delegates to Python. The external tap is unchanged;
the local formula remains a candidate. No merge, source push or promotion is authorized.

## API / Interfaces

Build explicitly in the canonical checkout (requires cached locked dependencies):

```sh
cd /home/jdawson/Projects/coven-rust-migration
cargo build --offline --locked --release --bin coven
bin/coven --help
COVEN_NATIVE_BIN=/abs/prebuilt/coven /abs/checkout/bin/coven --help
```

`/abs/...` values are placeholders. The launcher selects absolute `COVEN_NATIVE_BIN`
when set, otherwise only the normal checkout's `target/release/coven`. Missing
selection fails with explicit build guidance; no Cargo, Python, debug-binary or
PATH fallback exists. Symlinked/relocated launchers require explicit absolute
native selection. Help is titled `Manage multi-agent coven workspaces`.

### Install a Prebuilt Binary

Supply a trusted, already-built native executable for the host OS/architecture.
The installer does **not** build, download, invoke Cargo, install Python, run the
product, or install a Python wrapper/runner. It uses host `sh`, `install`, `mkdir`
and `cp` only. Building is a separate, explicit developer/release step; an offline
build requires a populated locked dependency cache.

For example, with `/opt/coven-staging` already present and trusted, and
`/opt/coven-staging/candidate-1` nonexistent:

```sh
just install-native /absolute/prebuilt/coven /opt/coven-staging/candidate-1
```

The preserved recipe calls `scripts/install-native.sh`. Its default examples
come from the distribution's `examples/` beside `scripts/`. For another asset
bundle, call the script explicitly:

```sh
sh scripts/install-native.sh \
  --binary /absolute/prebuilt/coven \
  --prefix /opt/coven-staging/candidate-2 \
  --examples /absolute/distribution/examples
```

All supplied paths must be absolute, without `..` components. The binary must
be a regular executable, not a symlink. The prefix must not exist, **even as an
empty directory**; its immediate parent must already exist and not be a symlink.
Prefix and examples-root symlinks are refused. Ancestors must be trusted and
stable; this is not protection against hostile concurrent ancestor replacement.

Only `bin/coven` and `share/coven/examples/` are installed. No checkout, runner
payload, wrapper or docs are copied. Prefix reservation is exclusive. A failure
after reservation preserves the partial prefix, reports failure and performs no
rollback or automatic cleanup; retries require a new prefix.

**In-place upgrades are REFUSED**, including over this installer's own output.
This is not an upgrade mechanism, an uninstaller or a way to clean an existing
Python install. Keep existing prefixes untouched. A release still needs reviewed
clean-install, upgrade, selection and rollback procedures before external promotion.

## Data Flow

### Source package vs installed payload

Cargo uses a positive `package.include` allowlist for production Rust and embedding
inputs. It excludes all `runner/`, `.pyc`/cache files, target outputs and interim
trees. Development `*oracle*.py` inputs remain in the source package, including
compile-time inputs, but are not installed. The full retained Python-oracle suite
is checkout-only; building a source package does not claim standalone-package
test closure. Python 3.14.8 and Python development/audit tooling are needed for
the full checkout checks, not native runtime execution.

Nested harness packages previously caused Cargo to omit production modules. The
harnesses now live in `tools/harnesses/group_publisher` and
`tools/harnesses/group_live`; all production Rust remains under `src/`.

### Asset Resolution

The native `src/example_assets.rs` resolver uses this precedence:

1. `COVEN_EXAMPLES_DIR`, if set, is the authoritative examples root.
2. Otherwise `COVEN_HOME/examples`, if `COVEN_HOME` is set, is authoritative.
3. Otherwise discover relative to the canonical executable: first
   `<prefix>/share/coven/examples`, then `<prefix>/libexec/examples`.
4. Only a binary actually inside a Cargo `target` tree may additionally discover
   the corresponding checkout's `examples/`.

Explicit roots must be absolute directories, not symlink leaves; invalid explicit
roots fail rather than falling through. The first discovered implicit root wins;
an unsafe candidate fails rather than silently selecting a later one. Explicit
roots govern even `interactive-demo`; without an explicit root that demo remains
embedded, so an implicitly discovered copy does not replace it. Custom examples
use the selected asset root. Blank initialization needs no example content.
Relocation does not depend on a compiled-in checkout path.

The checkout launcher preserves, rather than overwrites, both `COVEN_HOME` and
`COVEN_COMMAND`. `COVEN_HOME` selects assets, not a native binary. `COVEN_COMMAND`
overrides generated commands; absent that override, generation uses canonical
`coven`. Installed binaries have no launcher or checkout dependency.

## Completed locally vs External checklist

- **Local scoped evidence:** five independent entrypoint tests, two package tests
  and nine native-install tests provide runtime-guard coverage. An agent verified
  the 300-file crate build, extracted release build/install and help, blank/demo
  init and refresh with a cleared environment and empty PATH. This is not a fresh
  aggregate full-suite result.
- **Local acceptance:** entrypoint **5**, package **2** and installer **9** tests pass.
  Actual Cargo packaging verified 300 files with no runner/bytecode/target payload
  and all production modules retained. Root release build and cleared-environment,
  empty-PATH native help pass. Strict all-target Clippy and Rust **1.85.0** all-target
  check pass after the two MSRV fixes, without production-policy changes.
  [Final split validation](../RUST_MIGRATION.md#final-local-acceptance) totals **1,225
  native passes**, plus the standalone **941 Python tests in 191.978s** and **22 tool
  tests in 156.071s**; it is not a single fresh all-command green invocation.
  Ready for local review and manual native use within documented profiles.
- **External:** macOS has not run; actual Homebrew clean install/upgrade is untested.
  `packaging/coven.rb` has no Python dependency or runner payload and asserts the
  native help title, but remains unpromoted. The external tap was not modified.
  No changes have been committed, pushed or merged.
- **Residual policy/cleanup:** non-UTF-8 and finite resource/value restrictions
  remain subject to explicit disposition; native product readiness does not approve
  them automatically. The retained source-oracle audit is separate from runtime coverage.

### Local and CI Gates

```sh
just test-native-install
just check-retirement-coverage
just require-retirement-complete
```

`test-native-install` is preserved unchanged. The full Rust suite also discovers
`tests/native_install.rs`: it installs a prebuilt real binary into disposable
prefixes, relocates it, removes its distribution, checks the installed file
inventory has no runner payload, and exercises product children with empty PATH
and a cleared environment. The installer itself still needs host tools. These
tests cover blank/demo/custom init, refresh, task operations, safe prompt preview,
generated commands and refusal/failure cases, not all workflows through a package.

`check-retirement-coverage` validates the frozen ledger and prints its summary.
`require-retirement-complete` additionally rejects unresolved obligations. It is
an **intentionally red development-oracle removal gate**: currently 67 of 941 original
methods are mapped and **874 remain unresolved**. This is not 874 missing features
or a count of native product execution coverage. Candidate mappings are not assertion reviews.
Neither recipe is added to the current default `check`; in particular, do not make
the completion gate a required green check until the obligations are reviewed and
mapped. See `tests/retirement/README.md` for assertion and provenance requirements.

The new `.github/workflows/rust-migration.yml` defines Ubuntu and macOS jobs with
Python **3.14.8**, the pinned development oracle, and stable Rust with rustfmt and
Clippy. Each job fetches dependencies with `cargo fetch --locked` before offline,
locked Clippy and full `cargo test --offline --locked --no-fail-fast`. Warnings
are denied. It retains the 941-method Python suite, separate audit-tool
tests and ledger `--check --summary`. Jobs have a 60-minute budget; the complete
Rust suite has 45 minutes and the Python/tool test steps each have 15 minutes.
A separate Rust **1.85.0** job fetches the same lockfile
then runs offline locked all-target `cargo check` with warnings denied. MSRV
dependency failures require repository dependency/lockfile changes, not unlocked
resolution or warning suppression.

These workflow definitions are **not evidence that CI has run**. macOS CI remains
unexecuted until actual results are reviewed, as does the clean-runner MSRV gate.
The workflow uses read-only repository permissions, no privileged PR event,
no secrets, no persisted checkout credentials and no artifact uploads. The Python
oracle stays a test dependency, never an installed product fallback.

A local Linux attempt exhausted an initial 15-minute full-Rust-test budget.
The subsequently completed full run passed all 1,191 native tests with a larger
tool deadline; CI therefore allows 45 minutes for the complete Rust suite rather
than omitting tests. Clean-runner timing, particularly macOS, remains unverified.
That full run is historical pre-cutover evidence; current results are the final
split local acceptance linked above. Local Rust 1.85.0 success is not evidence that
the clean-runner CI job or macOS has run. The moved publisher/live harness commands
also pass 177/25 tests respectively, separate from the native aggregate.

## Edge Cases

`packaging/coven.rb` is a **candidate only**, not promoted to the external Homebrew
tap. This work neither edits nor publishes that tap. Formula promotion needs
reviewed native install/upgrade evidence, macOS acceptance and release approval;
a candidate formula is not proof of release readiness. Its source URL tracks remote
`main`; do not assume the local candidate is published or promote it before a
reviewed release. No source push is authorized by these instructions.

All 941 original Python tests remain active. Eleven legacy-wrapper methods now
invoke the Python runner explicitly with former wrapper exports recreated and
identical assertions, remaining argv, cwd and timeouts. Independent AST review
accepted source-hash transitions preserving original/current hashes and all 941 IDs,
not baseline shrinkage or source deletion. Retain fixture provenance and review
before removing development oracles. Product cutover is not automatic source deletion.

Existing workspaces need an explicit generated-command refresh strategy after
relocation or cutover. Inventory legacy checkout-wrapper paths and
`COVEN_COMMAND` overrides, distinguish generated files from authored/preserved
content, preview and review changes with backups, then regenerate only known
generated commands using the accepted native refresh path. New generation defaults
to canonical `coven`; an explicit `COVEN_COMMAND` can pin a stable installed native
path. Merely moving a binary does not repair old prompts/manifests, and native
refresh is not a blanket authored-content migration. Arbitrary search-and-replace
of user-authored files is not authorized. Explicit compatibility-policy disposition,
reviewed workspace migration, platform results and safe upgrade/default selection
remain release blockers, even when all 78 routes are represented. Source-oracle
removal remains a separately reviewed cleanup gate.

## Example usage

With `/abs/existing-parent` already present/trusted and `new-prefix` absent:

```sh
sh scripts/install-native.sh --binary /abs/prebuilt/coven --prefix /abs/existing-parent/new-prefix --examples /abs/checkout/examples
/abs/existing-parent/new-prefix/bin/coven --help
```

The installer needs host tools, but the installed product does not invoke Cargo or
Python. Partial prefixes are preserved; retries require a different new prefix.

## Related files

- `../Cargo.toml`: positive source-package allowlist.
- `../bin/coven`, `../src/main.rs`: checkout selection and native help/routing.
- `../scripts/install-native.sh`, `coven.rb`: staged payload and candidate formula.
- `../src/example_assets.rs`: relocatable asset resolver and `AssetBundle`.
- `../tests/native_entrypoint.rs`, `../tests/native_package.rs`, `../tests/native_install.rs`:
  independent runtime, inventory and relocation guards.
- [../RUST_RETIREMENT.md](../RUST_RETIREMENT.md): release vs source-cleanup gates.
