#!/usr/bin/env bash
# check-rust-lints.sh — the RUST-LINTS family guard.
#
# One script, one entry, invoked identically by the CI rust job and the
# rust-lints blocking callback — only the --ci / --callback flag differs. Per the
# T518 rigorous-equality mandate the callback covers the ENTIRETY of the CI lint
# set; the flag is reserved for a future CI-vs-local environment tweak (there is
# none today, so both modes run the same full checks).
#
# Sub-checks (all fail-fast to a single non-zero exit):
#   1. cargo fmt -- --check                     (rustfmt twin)
#   2. cargo clippy --all-targets -- -D warnings (the clippy gate)
#   3. RUSTFLAGS="-D warnings" cargo check       (rustc-forbid twin)
#   4. lint-exception registry — delegates to check-lint-exceptions.sh
#   5. vault-bypass (FULL repo scan) — delegates to check-vault-bypass.sh
#
# vault-bypass runs a WHOLE-REPO scan in both modes (no $CP_CHANGED_FILES
# narrowing): coverage equality trumps the incremental speed-up the callback
# used to take. Repo root via git so it is cwd-independent.
set -uo pipefail

MODE="${1:---ci}"   # --ci | --callback (behaviourally identical today)
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
fail=0

echo "=== cargo fmt --check ==="
cargo fmt -- --check 2>&1 || fail=1

echo "=== cargo clippy --all-targets -D warnings ==="
cargo clippy --all-targets -- -D warnings 2>&1 || fail=1

echo "=== cargo check -D warnings ==="
RUSTFLAGS="-D warnings" cargo check 2>&1 || fail=1

echo "=== lint-exception registry ==="
bash "$ROOT/.github/checks/check-lint-exceptions.sh" || fail=1

echo "=== vault-bypass (full scan) ==="
bash "$ROOT/.github/checks/check-vault-bypass.sh" || fail=1

if [ "$fail" -eq 0 ]; then
  echo "check-rust-lints OK ($MODE): fmt · clippy · check · exceptions · vault ✓"
fi
exit "$fail"
