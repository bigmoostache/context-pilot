#!/usr/bin/env bash
# Frontend anti-suppression guard — the web twin of check-vault-bypass.sh and
# the CI-side enforcement of the P4 anti-cheat layer.
#
# Inline lint/type suppression is FORBIDDEN in hand-written frontend source.
# There is NO per-site allow-list: the only legal way to relax a rule is a
# config-level scoped override in web/eslint.config.ts, registered + justified
# in .github/checks/allowed-eslint-exceptions.yaml (hash-locked). This script
# enforces two invariants, independent of ESLint's own config so a reconfigured
# eslint.config.ts can't silently re-open the door:
#
#   1. STOWAWAY  — no eslint-disable / @ts-ignore / @ts-nocheck / @ts-expect-error
#                  / @ts-check anywhere under web/src (excluding generated/**).
#   2. ORPHAN/UNREGISTERED — every `"<rule>": "off"` scoped override in
#                  eslint.config.ts must have a matching entry in the exceptions
#                  registry, and every registry rule must appear in the config
#                  (no silent add, no stale entry).
#
# Usage (CI):       .github/checks/check-frontend-suppressions.sh
# Usage (callback): .github/checks/check-frontend-suppressions.sh [file ...]
#
# In callback mode only the STOWAWAY grep runs, over the supplied files (the
# config-override audit is a whole-repo invariant, run in CI mode only).
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SRC="$ROOT/web/src"
CONFIG="$ROOT/web/eslint.config.ts"
REGISTRY="$ROOT/.github/checks/allowed-eslint-exceptions.yaml"

# Banned inline suppression directives. A directive is only recognised by ESLint
# / tsc when its keyword is the FIRST token inside the comment — so we anchor to
# `//` or `/*` immediately followed (optional whitespace) by the keyword. This
# matches a real `// eslint-disable-next-line` / `/* @ts-ignore */` while NEVER
# matching the word appearing mid-prose (e.g. this file, or a doc comment that
# merely mentions `eslint-disable` in backticks). @ts-nocheck is legal ONLY in
# generated/** (the codegen bindings), which every scan below excludes.
BANNED='(//|/\*)[[:space:]]*(eslint-disable|eslint-enable|@ts-ignore|@ts-nocheck|@ts-expect-error|@ts-check)'

exit_code=0

# ── Invariant 1: STOWAWAY — no inline suppression in hand-written source ──
check_file() {
  local file="$1"
  case "$file" in
    *"/generated/"*) return ;;            # codegen bindings: @ts-nocheck is legal
    *.ts | *.tsx | *.js | *.jsx | *.cts | *.mts) ;;
    *) return ;;
  esac
  [ -f "$file" ] || return 0

  local hits
  hits=$(grep -nE "$BANNED" "$file" 2>/dev/null || true)
  if [ -n "$hits" ]; then
    while IFS= read -r line; do
      echo "::error file=$file::$line"
    done <<< "$hits"
    echo "FAIL: $file carries a banned inline suppression directive" >&2
    exit_code=1
  fi
}

if [ $# -gt 0 ]; then
  # Callback mode: scan only the supplied files (stowaway grep only).
  for file in "$@"; do
    check_file "$file"
  done
  exit $exit_code
fi

# CI mode: scan the whole frontend source tree.
if [ -d "$SRC" ]; then
  while IFS= read -r file; do
    check_file "$file"
  done < <(find "$SRC" -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.jsx' \) -not -path '*/generated/*')
fi

# ── Invariant 2: config-level scoped overrides ⇄ registry (bidirectional) ──
# Extract every `"<rule>": "off"` from eslint.config.ts — these are the only
# rule-disabling scoped overrides (calibrations use the array form
# `["error", {...}]`, never the bare `"off"` string, so they aren't matched).
config_offs=""
if [ -f "$CONFIG" ]; then
  config_offs=$(grep -oE '"[^"]+"[[:space:]]*:[[:space:]]*"off"' "$CONFIG" \
    | sed -E 's/^"([^"]+)".*/\1/' | sort -u || true)
fi

# Extract every `rule:` value from the registry. Match only the quoted value
# and strip the quotes (`[[:space:]]` not `\s` so BSD/macOS sed works too); the
# wildcard `*` entry is quoted (`rule: "*"`) so it extracts as `*`.
registry_rules=""
if [ -f "$REGISTRY" ]; then
  registry_rules=$(grep -oE 'rule:[[:space:]]*"[^"]*"' "$REGISTRY" \
    | sed -E 's/rule:[[:space:]]*"([^"]*)"/\1/' | sort -u || true)
else
  echo "::error::Exceptions registry not found: $REGISTRY" >&2
  exit_code=1
fi

# 2a — every config `"off"` override must be registered (no silent exception).
if [ -n "$config_offs" ]; then
  while IFS= read -r rule; do
    [ -z "$rule" ] && continue
    if ! echo "$registry_rules" | grep -qxF "$rule"; then
      echo "::error file=$CONFIG::Unregistered scoped override \"$rule\": \"off\" — add it to $REGISTRY" >&2
      echo "FAIL: eslint.config.ts disables '$rule' with no registry entry" >&2
      exit_code=1
    fi
  done <<< "$config_offs"
fi

# 2b — every registry rule (except the wildcard '*' globalIgnore doc) must
#      correspond to a live config override (no stale/orphan entry).
if [ -n "$registry_rules" ]; then
  while IFS= read -r rule; do
    [ -z "$rule" ] && continue
    [ "$rule" = "*" ] && continue          # '*' documents the generated/** globalIgnore, not an "off"
    if ! echo "$config_offs" | grep -qxF "$rule"; then
      echo "::error file=$REGISTRY::Orphan registry entry '$rule' — no matching \"off\" override in eslint.config.ts" >&2
      echo "FAIL: registry lists '$rule' but eslint.config.ts no longer disables it" >&2
      exit_code=1
    fi
  done <<< "$registry_rules"
fi

if [ "$exit_code" -eq 0 ]; then
  echo "Frontend suppression guard OK — no inline suppressions; exceptions registered. ✓"
else
  echo "" >&2
  echo "Inline lint/type suppression is banned in web/src." >&2
  echo "The only legal escape is a config-level scoped override in" >&2
  echo "web/eslint.config.ts, registered + justified in" >&2
  echo ".github/checks/allowed-eslint-exceptions.yaml." >&2
fi

exit $exit_code
