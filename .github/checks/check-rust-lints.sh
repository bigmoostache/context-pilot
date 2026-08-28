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
#   2. cargo clippy --workspace --all-targets -- -D warnings (the clippy gate)
#   3. RUSTFLAGS="-D warnings" cargo check --workspace (rustc-forbid twin,
#      --ci only — redundant with the rust-tests callback's full-workspace
#      build in --callback)
#   4. lint-exception registry — delegates to check-lint-exceptions.sh
#   5. vault-bypass (FULL repo scan) — delegates to check-vault-bypass.sh
#
# --workspace is LOAD-BEARING (added after the campaign that drove
# cp-orchestrator/cp-oplog/cp-console-server to clippy-clean): a bare
# --all-targets from the repo root only compiles the `tui` root package and
# its dependency graph, so standalone binaries outside that graph
# (cp-orchestrator, cp-oplog, cp-console-server) were NEVER linted and
# accumulated lint debt invisibly. --workspace lints every crate, closing
# that blind spot permanently.
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

echo "=== cargo clippy --workspace --all-targets -D warnings ==="
cargo clippy --workspace --all-targets -- -D warnings 2>&1 || fail=1

# cargo check runs in --ci only. In --callback it is redundant + too slow: the
# rust-tests callback already does a full-workspace `cargo build` (debug) which
# compiles every crate and surfaces the exact same `-D warnings` rustc errors,
# so re-checking here would double the workspace compile on every .rs edit for
# zero extra coverage. CI keeps it (the rust job's cache is warm; explicit gate).
if [ "$MODE" = "--ci" ]; then
  echo "=== cargo check --workspace -D warnings ==="
  RUSTFLAGS="-D warnings" cargo check --workspace 2>&1 || fail=1
fi

echo "=== lint-exception registry ==="
bash "$ROOT/.github/checks/check-lint-exceptions.sh" || fail=1

echo "=== vault-bypass (full scan) ==="
bash "$ROOT/.github/checks/check-vault-bypass.sh" || fail=1

if [ "$fail" -eq 0 ]; then
  echo "check-rust-lints OK ($MODE): fmt · clippy · check · exceptions · vault ✓"
fi
exit "$fail"
