#!/usr/bin/env bash
# Frontend ESLint rule census — the clippy_compare.py twin for the web ratchet.
#
# The bidirectional suppression guard (check-frontend-suppressions.sh) proves no
# rule is disabled via an *inline* directive or an *unregistered scoped "off"*.
# This script closes the last gap: a silent change to the RESOLVED SEVERITY of a
# rule in the main ruleset — someone flipping an `error` to `warn`/`off` at the
# tree-wide level, or a plugin bump that quietly drops/downgrades a rule.
#
# It snapshots the FULLY RESOLVED ruleset ESLint applies to a representative
# app-source file (src/App.tsx — it inherits every tree-wide rule plus the
# src/** overrides) as a sorted `rule = severity` list, and diffs it against a
# committed, hash-locked baseline. Any add / drop / severity change fails CI
# until the baseline is deliberately regenerated (--update) and re-hash-locked.
#
# Only rule NAME + SEVERITY are tracked, not option payloads: the census guards
# "which rules are active at what severity", while option calibrations are
# reviewed in eslint.config.ts (itself hash-locked). Severity is normalised
# 0→off / 1→warn / 2→error.
#
# Usage:
#   .github/checks/check-frontend-rule-census.sh            # verify (CI)
#   .github/checks/check-frontend-rule-census.sh --update   # regenerate baseline
#
# Requires web/node_modules (run inside the CI frontend job, which installs deps).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
WEB="$ROOT/web"
SAMPLE="src/App.tsx"
BASELINE="$ROOT/.github/checks/frontend-rule-census.txt"

# Resolve the active ruleset for the sample file and reduce it to a sorted
# `rule = severity` list (names + normalised severity only).
extract() {
  cd "$WEB"
  npx --no-install eslint --print-config "$SAMPLE" | python3 -c '
import json, sys
cfg = json.load(sys.stdin)
rules = cfg.get("rules", {})
sev = {0: "off", 1: "warn", 2: "error"}
lines = []
for name in sorted(rules):
    v = rules[name]
    s = v[0] if isinstance(v, list) else v
    lines.append(f"{name} = {sev.get(s, s)}")
sys.stdout.write("\n".join(lines) + "\n")
'
}

census="$(extract)"
count="$(printf '%s' "$census" | grep -c ' = ' || true)"

if [ "${1:-}" = "--update" ]; then
  printf '%s\n' "$census" > "$BASELINE"
  echo "Frontend rule census baseline updated ($count rules) ✓"
  echo "Remember to re-hash-lock: .github/checks/check-lint-config.sh --update"
  exit 0
fi

if [ ! -f "$BASELINE" ]; then
  echo "::error::No frontend rule census baseline at $BASELINE" >&2
  echo "FAIL: missing baseline — run: $0 --update" >&2
  exit 1
fi

if diff -u "$BASELINE" <(printf '%s\n' "$census"); then
  echo "Frontend rule census OK — ruleset matches baseline ($count rules) ✓"
else
  echo "" >&2
  echo "::error::ESLint resolved ruleset drifted from the committed baseline." >&2
  echo "FAIL: a rule was added / dropped / changed severity (diff above)." >&2
  echo "  If INTENTIONAL: regenerate + re-hash-lock:" >&2
  echo "    $0 --update" >&2
  echo "    .github/checks/check-lint-config.sh --update" >&2
  echo "  If NOT: revert the eslint.config.ts / plugin change." >&2
  exit 1
fi
