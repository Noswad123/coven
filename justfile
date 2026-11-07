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
