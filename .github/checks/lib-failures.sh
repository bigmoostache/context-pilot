# shellcheck shell=bash
# lib-failures.sh — failure ledger shared by the aggregating family scripts.
#
# Sourced, never executed. The family scripts run every sub-check and exit
# non-zero once at the end, so in a CI log the "exit code 1" lands under the
# LAST section header whatever actually failed (M-long-10: knip blamed, prettier
# guilty). The ledger names the culprits twice: an ::error:: annotation right
# after the failing sub-check, and a closing summary listing every one of them.
#
# Usage:
#   . "$ROOT/.github/checks/lib-failures.sh"
#   ledger_init check-ts-lints
#   npx eslint . || ledger_fail eslint
#   ledger_finish "eslint · prettier ✓"   # exits 0, or 1 with the summary

ledger_init() {
  LEDGER_SCRIPT="$1"
  LEDGER_FAILED=""
}

# Record a failed sub-check. A name recorded twice (e.g. one per offending file
# in a loop) appears once in the summary.
ledger_fail() {
  case " · $LEDGER_FAILED · " in
    *" · $1 · "*) return 0 ;;
  esac
  LEDGER_FAILED="${LEDGER_FAILED:+$LEDGER_FAILED · }$1"
  echo "::error::$LEDGER_SCRIPT: sub-check '$1' FAILED (its diagnostics are just above)"
}

# Print the verdict and exit. $1 is the success line suffix.
ledger_finish() {
  if [ -z "$LEDGER_FAILED" ]; then
    echo "$LEDGER_SCRIPT OK ($MODE): $1"
    exit 0
  fi
  echo ""
  echo "::error::$LEDGER_SCRIPT FAILED sub-checks: $LEDGER_FAILED"
  echo "FAIL: $LEDGER_SCRIPT — failed sub-checks: $LEDGER_FAILED" >&2
  echo "      Read the output under each named '=== … ===' header; the last header is not the culprit by default." >&2
  exit 1
}
