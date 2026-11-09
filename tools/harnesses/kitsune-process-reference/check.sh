#!/usr/bin/env bash
set -euo pipefail

# Invoke from Coven's root, with the same explicit source environment as capture.
: "${KITSUNE_REFERENCE_ROOT:?absolute reference root required}"
: "${KITSUNE_REFERENCE_DETECT:?absolute detect file required}"
: "${KITSUNE_REFERENCE_PLATFORM:?absolute platform file required}"
: "${KITSUNE_REFERENCE_WINDOWS:?absolute Windows file required}"
: "${CARGO_TARGET_DIR:?external target directory required}"
manifest=tools/harnesses/kitsune-process-reference/Cargo.toml

expect_rejection() {
    local expected=$1 output
    shift
    if output=$("$@" 2>&1); then
        printf '%s\n' 'ERROR: invalid reference was accepted' >&2
        exit 1
    fi
    case "$output" in
        *"$expected"*) printf 'Rejected as expected: %s\n' "$expected" ;;
        *) printf '%s\n' "$output" >&2; exit 1 ;;
    esac
}

(
    unset KITSUNE_REFERENCE_ROOT
    expect_rejection 'explicit reference root' cargo check --offline --locked --manifest-path "$manifest"
)
(
    export KITSUNE_REFERENCE_ROOT=relative-reference
    expect_rejection 'reference root must be absolute' cargo check --offline --locked --manifest-path "$manifest"
)
(
    unset KITSUNE_REFERENCE_DETECT
    expect_rejection 'explicit source file required' cargo check --offline --locked --manifest-path "$manifest"
)
(
    export KITSUNE_REFERENCE_DETECT=relative-source.rs
    expect_rejection 'source file must be absolute' cargo check --offline --locked --manifest-path "$manifest"
)
(
    export KITSUNE_REFERENCE_DETECT=$KITSUNE_REFERENCE_WINDOWS
    expect_rejection 'supplied source differs from pinned Git blob' cargo check --offline --locked --manifest-path "$manifest"
)
cargo test --offline --locked --manifest-path "$manifest"
cargo run --offline --locked --manifest-path "$manifest" -- --check "${1:?absolute frozen fixture path required}"
