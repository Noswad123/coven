default:
    @just --list

format:
    cargo fmt --all

lock:
    cargo generate-lockfile --offline

fmt:
    cargo fmt --all -- --check

lint:
    cargo clippy --offline --locked --all-targets -- -D warnings

test-rust:
    CARGO_BUILD_JOBS=2 cargo test --offline --locked

test-rust-verbose:
    CARGO_BUILD_JOBS=2 cargo test --offline --locked -- --nocapture

test-python:
    PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -q

test: test-rust test-python

check: fmt lint test

# Staged path only: never changes bin/coven or installs over an existing prefix.
install-native binary prefix:
    sh scripts/install-native.sh --binary {{quote(binary)}} --prefix {{quote(prefix)}}

test-native-install:
    CARGO_BUILD_JOBS=2 cargo test --offline --locked --test native_install

# Validate the frozen baseline and print counts without claiming retirement.
check-retirement-coverage:
    PYTHONDONTWRITEBYTECODE=1 python3 tools/retirement_coverage.py --check --summary

# Intentionally red while obligations remain unresolved; not part of check.
require-retirement-complete:
    PYTHONDONTWRITEBYTECODE=1 python3 tools/retirement_coverage.py --check --summary --require-complete
