#!/usr/bin/env bash
# check-ts-lints.sh — the TS-LINTS family guard.
#
# One script, one entry, invoked identically by the CI frontend job and the
# ts-lints blocking callback — only the --ci / --callback flag differs. Per the
# T518 rigorous-equality mandate the callback covers the ENTIRETY of the CI
# frontend lint set (the former web-lint incremental changed-files pass is GONE;
# the callback now runs the full-tree checks, whole-graph passes included, per
# the explicit "run them anyway" decision). The flag is reserved for a future
# CI-vs-local environment tweak.
#
# Sub-checks (all run; single non-zero exit aggregates failures):
#   1. eslint . --max-warnings 0                 (the type-aware ratchet)
#   2. prettier --check .                         (formatter)
#   3. stylelint "src/**/*.css"                   (CSS; NEVER --fix, M147)
#   4. tsc -b                                     (type check)
#   5. type-coverage --project tsconfig.app.json  (unsafe-any ratchet)
#   6. suppressions   — delegates to check-frontend-suppressions.sh (ts lint exceptions)
#   7. rule census    — delegates to check-frontend-rule-census.sh   (ruleset drift)
#   8. dead code      — delegates to check-web-deadcode.sh           (knip whole-graph)
#
# Self-locating: cd's into web/ for the npx tools (they resolve web's local
# eslint/prettier/tsc/etc + config); the delegated guards self-locate via git.
set -uo pipefail

MODE="${1:---ci}"   # --ci | --callback (behaviourally identical today)
ROOT="$(git rev-parse --show-toplevel)"
fail=0

cd "$ROOT/web"

echo "=== eslint (--max-warnings 0) ==="
npx eslint . --max-warnings 0 || fail=1

echo "=== prettier (--check) ==="
npx prettier --check . || fail=1

echo "=== stylelint ==="
npx stylelint "src/**/*.css" || fail=1

echo "=== tsc -b ==="
npx tsc -b || fail=1

echo "=== type-coverage ==="
npx type-coverage --project tsconfig.app.json || fail=1

echo "=== frontend suppressions (ts lint exceptions) ==="
bash "$ROOT/.github/checks/check-frontend-suppressions.sh" || fail=1

echo "=== frontend rule census (ruleset drift) ==="
bash "$ROOT/.github/checks/check-frontend-rule-census.sh" || fail=1

echo "=== dead code (knip) ==="
bash "$ROOT/.github/checks/check-web-deadcode.sh" || fail=1

if [ "$fail" -eq 0 ]; then
  echo "check-ts-lints OK ($MODE): eslint · prettier · stylelint · tsc · type-coverage · suppressions · census · knip ✓"
fi
exit "$fail"
