#!/usr/bin/env bash
#
# check-web-structure.sh — frontend structural guardrail for web/src.
#
# Mirrors the Rust workspace's folder-size / file-length policy (the sibling
# .github/checks/check-file-lengths.sh + check-folder-sizes.sh enforce ≤8
# entries per directory and ≤500 lines per source file) for the TypeScript
# frontend, which those hash-chain-protected Rust scripts do not cover.
#
# Rules (applied to every directory + every .ts/.tsx file under web/src):
#   • a directory may hold at most MAX_ENTRIES (8) immediate children;
#   • a source file may be at most MAX_LINES (500) lines long.
#
# Exceptions:
#   • web/src/components/ui       — shadcn primitives, vendored (not authored here);
#   • web/src/lib/api/generated   — OpenAPI-generated client, machine-generated.
#
# Exit 0 when clean, 1 (listing every offender) otherwise. Invocation-cwd
# independent: the repo root is resolved via `git rev-parse --show-toplevel`,
# so it runs identically from a callback (project root), CI, or any subdir.

set -euo pipefail

MAX_ENTRIES=8
MAX_LINES=500

# Repo root — resolved independently of the caller's cwd, matching the other
# .github/checks/ scripts (this one lives here after being moved out of
# web/scripts/).
ROOT="$(git rev-parse --show-toplevel)"
WEB_ROOT="$ROOT/web"
SRC="$WEB_ROOT/src"
# shadcn vendored primitives — exempt from both rules.
EXEMPT="$SRC/components/ui"
# OpenAPI-generated TypeScript client — machine-generated, not authored here.
EXEMPT_GENERATED="$SRC/lib/api/generated"

fail=0

is_exempt() {
  # True when $1 is an exempt dir itself or anything beneath it.
  case "$1" in
    "$EXEMPT" | "$EXEMPT"/*) return 0 ;;
    "$EXEMPT_GENERATED" | "$EXEMPT_GENERATED"/*) return 0 ;;
    *) return 1 ;;
  esac
}

# ── file length ────────────────────────────────────────────────────────
while IFS= read -r f; do
  is_exempt "$f" && continue
  n=$(wc -l < "$f" | tr -d '[:space:]')
  if [ "$n" -gt "$MAX_LINES" ]; then
    echo "✗ file >$MAX_LINES lines ($n): ${f#"$WEB_ROOT"/}"
    fail=1
  fi
done < <(find "$SRC" -type f \( -name '*.ts' -o -name '*.tsx' \))

# ── directory entry count ──────────────────────────────────────────────
while IFS= read -r d; do
  is_exempt "$d" && continue
  n=$(find "$d" -mindepth 1 -maxdepth 1 | wc -l | tr -d '[:space:]')
  if [ "$n" -gt "$MAX_ENTRIES" ]; then
    echo "✗ dir >$MAX_ENTRIES entries ($n): ${d#"$WEB_ROOT"/}"
    fail=1
  fi
done < <(find "$SRC" -type d)

if [ "$fail" -eq 0 ]; then
  echo "web structure OK ✓ (≤$MAX_ENTRIES entries/dir, ≤$MAX_LINES lines/file; shadcn ui/ + generated/ exempt)"
fi

exit "$fail"
