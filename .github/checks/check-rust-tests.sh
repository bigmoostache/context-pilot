#!/usr/bin/env bash
# check-rust-tests.sh — the RUST-TESTS family guard.
#
# One script, one entry, invoked identically by the CI rust job and the
# rust-tests blocking callback. This is the ONE family where --ci / --callback
# genuinely differ on an environment detail (not on coverage): CI builds the
# release profile (what ships), the local callback builds debug (faster feedback
# on every .rs edit). BOTH run the full test suite — coverage is equal.
#
# Sub-checks:
#   1. cargo build [--release in --ci]
#   2. cargo test --workspace  (all 26 crates + the tui binary, not just tui)
#
# Repo root via git so it is cwd-independent.
set -uo pipefail

MODE="${1:---ci}"   # --ci = release build · --callback = debug build (tests identical)
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
fail=0

if [ "$MODE" = "--callback" ]; then
  echo "=== cargo build (debug) ==="
  cargo build 2>&1 || fail=1
else
  echo "=== cargo build --release ==="
  cargo build --release 2>&1 || fail=1
fi

echo "=== cargo test --workspace ==="
cargo test --workspace 2>&1 || fail=1

if [ "$fail" -eq 0 ]; then
  echo "check-rust-tests OK ($MODE): build · test ✓"
fi
exit "$fail"
