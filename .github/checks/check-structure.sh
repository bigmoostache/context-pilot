#!/usr/bin/env bash
# check-structure.sh — the STRUCTURE family guard (unified).
#
# One script, one entry, invoked identically by the CI structure job and the
# structure blocking callback — only the --ci / --callback flag differs (and, for
# now, they behave identically: the callback covers the ENTIRETY of the CI check,
# per the T518 rigorous-equality mandate; the flag exists only for a future
# CI-vs-local environment tweak).
#
# Absorbs the three former structural scripts (check-file-lengths.sh,
# check-folder-sizes.sh, check-web-structure.sh) into a single traversal over
# BOTH source trees, plus the hash-chain integrity verify:
#   1. File length  ≤ 500 lines   — Rust (*.rs) + web (src/*.ts,*.tsx)
#   2. Dir entries  ≤ 8            — Rust source tree + web/src tree
#   3. Hash-chain integrity        — delegates to check-lint-config.sh (verify)
#
# Exemptions mirror the originals: Rust skips target/ and the historically-
# exempt product/vendor dirs; web skips the vendored shadcn ui/ + the generated
# OpenAPI client. Repo root resolved via git so it is cwd-independent.
set -uo pipefail

MODE="${1:---ci}"   # --ci | --callback (behaviourally identical today)
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

MAX_LINES=500
MAX_ENTRIES=8
fail=0

# ── 1 + 2: Rust tree (src/ + crates/) — ≤500 lines/file, ≤8 entries/dir ──────
# (former check-file-lengths.sh + check-folder-sizes.sh)
while IFS= read -r f; do
  n=$(wc -l < "$f" | tr -d '[:space:]')
  if [ "$n" -gt "$MAX_LINES" ]; then
    echo "::error file=$f::$f has $n lines (max $MAX_LINES)"
    echo "FAIL: $f has $n lines (max $MAX_LINES) — extract into a sibling module." >&2
    fail=1
  fi
done < <(find . -name '*.rs' -not -path './target/*' -not -path '*/target/*')

while IFS= read -r dir; do
  count=$(find "$dir" -maxdepth 1 -mindepth 1 | wc -l | tr -d '[:space:]')
  if [ "$count" -gt "$MAX_ENTRIES" ]; then
    echo "::error::$dir has $count entries (max $MAX_ENTRIES)"
    echo "FAIL: $dir has $count entries (max $MAX_ENTRIES) — group into a sub-dir." >&2
    fail=1
  fi
done < <(find . -mindepth 1 -type d \
  -not -path './target/*' -not -path '*/target/*' \
  -not -path './.git' -not -path './.git/*' \
  -not -path './crates' \
  -not -path './.context-pilot' -not -path './.context-pilot/*' \
  -not -path './website/*' \
  -not -path './docs' -not -path './docs/*' \
  -not -path './brilliant-cv/*' -not -path './graceful-genetics/*' \
  -not -path './test-typst/*' \
  -not -path './.github/workflows' -not -path './.github/checks' \
  -not -path './yamls/tools' \
  -not -path './jobs' -not -path './jobs/*' \
  -not -path './benchmarks/terminal-bench/jobs' -not -path './benchmarks/terminal-bench/jobs/*' \
  -not -path './ui' -not -path './ui/*' \
  -not -path './web' -not -path './web/*' \
  -not -path './.uploads' -not -path './.uploads/*' \
  -not -path './oplog' -not -path './oplog/*' \
  -not -path './logs' -not -path './logs/*' \
  -not -path './dumps' -not -path './dumps/*' \
  -not -path './sandbox' -not -path './sandbox/*' \
  -not -path './gaia' -not -path './gaia/*' \
  -not -path './test-results' -not -path './test-results/*' \
  -not -path './report' -not -path './report/*')

# ── 1 + 2: web/src tree — ≤500 lines/file, ≤8 entries/dir ────────────────────
# (former check-web-structure.sh). shadcn ui/ + generated OpenAPI client exempt.
WEB_SRC="$ROOT/web/src"
EXEMPT_UI="$WEB_SRC/components/ui"
EXEMPT_GEN="$WEB_SRC/lib/api/generated"
# The mobile mirror's ui/ twin is a byte-for-byte mirror of the vendored shadcn
# ui/ (same 20-file shape), so it inherits the same folder-cap exemption — the
# scaffold cannot re-group it without breaking the EXACT parity contract.
EXEMPT_MOBILE_UI="$WEB_SRC/mobile-components/ui"
web_exempt() {
  case "$1" in
    "$EXEMPT_UI" | "$EXEMPT_UI"/* | "$EXEMPT_GEN" | "$EXEMPT_GEN"/* \
      | "$EXEMPT_MOBILE_UI" | "$EXEMPT_MOBILE_UI"/*) return 0 ;;
    *) return 1 ;;
  esac
}

if [ -d "$WEB_SRC" ]; then
  while IFS= read -r f; do
    web_exempt "$f" && continue
    n=$(wc -l < "$f" | tr -d '[:space:]')
    if [ "$n" -gt "$MAX_LINES" ]; then
      echo "::error file=$f::web file has $n lines (max $MAX_LINES): ${f#"$ROOT"/}"
      echo "FAIL: web ${f#"$ROOT"/} has $n lines (max $MAX_LINES)." >&2
      fail=1
    fi
  done < <(find "$WEB_SRC" -type f \( -name '*.ts' -o -name '*.tsx' \))

  while IFS= read -r d; do
    web_exempt "$d" && continue
    count=$(find "$d" -mindepth 1 -maxdepth 1 | wc -l | tr -d '[:space:]')
    if [ "$count" -gt "$MAX_ENTRIES" ]; then
      echo "::error::web dir has $count entries (max $MAX_ENTRIES): ${d#"$ROOT"/}"
      echo "FAIL: web ${d#"$ROOT"/} has $count entries (max $MAX_ENTRIES)." >&2
      fail=1
    fi
  done < <(find "$WEB_SRC" -type d)
fi

# ── 3: hash-chain integrity (the protected-files seal) ───────────────────────
echo "check-structure: verifying protected-files hash chain…"
bash "$ROOT/.github/checks/check-lint-config.sh" || fail=1

if [ "$fail" -eq 0 ]; then
  echo "check-structure OK ($MODE): lengths ≤$MAX_LINES, entries ≤$MAX_ENTRIES, chain intact ✓"
fi
exit "$fail"
