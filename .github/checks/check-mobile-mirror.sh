#!/usr/bin/env bash
# check-mobile-mirror — enforce that web/src/mobile-components is an EXACT
# structural mirror of web/src/components (design docs/design-mobile.md §5).
#
# Three invariants:
#   1. PATH PARITY (bidirectional, case-sensitive) — the set of mirror-source
#      relative paths is identical on both sides. A file/dir in one tree but not
#      the other fails. This is the "EXACT" guarantee.
#   2. LEAK GUARD — a hand-authored (marker-LESS) mobile file must not import or
#      re-export from `@/components/…`; it must reference its own tree
#      (`@/mobile-components/…`). Generated stubs are exempt: re-exporting the
#      desktop leaf IS the stub mechanism, so any file bearing the @generated
#      marker is skipped. The regex is anchored to a real import/export-from (or
#      dynamic-import) specifier position, so a `@/components/` mention in a
#      string or comment does not false-positive.
#   3. DRIFT (CI only) — regenerate the mirror and `git diff --exit-code`. Since
#      the scaffold is deterministic, the committed stub tree must equal a fresh
#      generation; hand-authored files are skipped by the scaffold so they never
#      drift. The codegen-contract pattern (check-typescript-contract.sh twin).
#      Drift needs node + writes files, so it is CI-only — never run in the
#      per-edit blocking callback.
#
# Dual-mode (the T518 CI ⇄ callback shape):
#   no args        → full CI scan: invariants 1 + 2 + 3.
#   any arg(s)     → callback scan: invariants 1 + 2 only (no node, no regen).
#
# BSD/macOS-safe (bash 3.2 process substitution; [[:space:]] not \s).

set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
WEB="$ROOT/web"
COMPONENTS="src/components"
MOBILE="src/mobile-components"
MARKER="// @generated mobile-mirror stub"

mode="ci"
[ "$#" -gt 0 ] && mode="callback"
fail=0

# The mirror-source relative paths under a web-relative subtree root ($1):
# *.ts/*.tsx, excluding test/spec/stories/ambient-declaration files (§11.4),
# sorted for a stable, case-sensitive comparison.
mirror_paths() {
  ( cd "$WEB" && find "$1" -type f \( -name '*.ts' -o -name '*.tsx' \) \
    | sed "s#^$1/##" \
    | grep -vE '\.(test|spec|stories)\.|\.d\.ts$' \
    | LC_ALL=C sort )
}

# ── Invariant 1: bidirectional path parity ───────────────────────────────────
desktop_paths="$(mirror_paths "$COMPONENTS")"
mobile_paths="$(mirror_paths "$MOBILE")"

only_desktop="$(comm -23 <(printf '%s\n' "$desktop_paths") <(printf '%s\n' "$mobile_paths"))"
only_mobile="$(comm -13 <(printf '%s\n' "$desktop_paths") <(printf '%s\n' "$mobile_paths"))"

if [ -n "$only_desktop" ]; then
  echo "::error::mobile-mirror parity: desktop paths with NO mobile twin (run pnpm mirror:scaffold):"
  printf '  %s\n' $only_desktop
  fail=1
fi
if [ -n "$only_mobile" ]; then
  echo "::error::mobile-mirror parity: mobile paths with NO desktop source (delete or add the desktop twin):"
  printf '  %s\n' $only_mobile
  fail=1
fi

# ── Invariant 2: leak guard (hand-authored mobile files only) ────────────────
# A marker-bearing stub is exempt (its whole job is to re-export @/components).
while IFS= read -r rel; do
  [ -z "$rel" ] && continue
  file="$WEB/$MOBILE/$rel"
  [ -f "$file" ] || continue
  head -n 1 "$file" | grep -qF "$MARKER" && continue  # generated stub → skip
  if grep -nE "^[[:space:]]*(import|export)[[:space:]].*from[[:space:]]*['\"]@/components/" "$file" >/dev/null \
     || grep -nE "import\([[:space:]]*['\"]@/components/" "$file" >/dev/null; then
    echo "::error::mobile-mirror leak: $MOBILE/$rel imports from @/components (a hand-authored mobile file must use @/mobile-components)"
    fail=1
  fi
done <<EOF
$mobile_paths
EOF

# ── Invariant 3: drift (CI only) ─────────────────────────────────────────────
if [ "$mode" = "ci" ]; then
  if ! command -v node >/dev/null 2>&1; then
    echo "::error::mobile-mirror drift: node not found (required for the scaffold regen)"
    fail=1
  else
    ( cd "$WEB" && node scripts/scaffold-mobile-mirror.mjs >/dev/null )
    if ! ( cd "$ROOT" && git diff --exit-code -- "web/$MOBILE" >/dev/null 2>&1 ); then
      echo "::error::mobile-mirror drift: mobile-components is out of sync with the scaffold — run pnpm mirror:scaffold and commit"
      fail=1
    fi
  fi
fi

if [ "$fail" -eq 0 ]; then
  echo "mobile-mirror OK ✓ ($(printf '%s\n' "$desktop_paths" | grep -c . ) twins, mode=$mode)"
fi
exit "$fail"
