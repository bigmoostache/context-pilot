#!/usr/bin/env bash
# Check that every #[expect(...)] and #[allow(...)] annotation in Rust source
# files is registered in the curated exceptions YAML — and vice versa.
#
# Three enforcement rules:
#   1. Every annotation in code MUST have a matching YAML entry (no stowaways).
#   2. Every YAML entry MUST match exactly one annotation (no orphans).
#   3. Every YAML entry MUST match at most one annotation (no ambiguity).
#
# Usage: .github/checks/check-lint-exceptions.sh
set -euo pipefail

YAML=".github/checks/allowed-lint-exceptions.yaml"
# How many lines after the annotation to include in the match window
WINDOW=10

if [ ! -f "$YAML" ]; then
  echo "ERROR: Exceptions manifest not found: $YAML" >&2
  exit 1
fi

# Pre-parse YAML: extract (path, match) pairs into parallel arrays.
paths=()
matches=()
current_path=""
while IFS= read -r yaml_line; do
  if [[ "$yaml_line" =~ ^[[:space:]]+\-\ path:\ *[\"\'](.*)[\"\'] ]]; then
    current_path="${BASH_REMATCH[1]}"
    continue
  fi
  if [[ "$yaml_line" =~ ^[[:space:]]+match:\ *[\"\'](.*)[\"\'] ]]; then
    paths+=("$current_path")
    matches+=("${BASH_REMATCH[1]}")
    continue
  fi
done < "$YAML"

entry_count=${#paths[@]}

# Track how many code annotations each YAML entry matches.
# Used for orphan detection (0 matches) and ambiguity detection (>1 match).
declare -a match_counts
for (( i=0; i<entry_count; i++ )); do
  match_counts[$i]=0
done

exit_code=0
unregistered=0

# ── Rule 1: Every annotation in code must have a registered YAML entry ───
while IFS= read -r hit; do
  file="${hit%%:*}"
  rest="${hit#*:}"
  lineno="${rest%%:*}"
  line_content="${rest#*:}"

  # Read a window of lines starting from the annotation for multi-line matching
  window=$(sed -n "${lineno},$((lineno + WINDOW))p" "$file")

  found=false
  for i in "${!paths[@]}"; do
    if [ "$file" = "${paths[$i]}" ] && echo "$window" | grep -qF "${matches[$i]}"; then
      match_counts[$i]=$(( ${match_counts[$i]} + 1 ))
      found=true
      break
    fi
  done

  if [ "$found" = false ]; then
    trimmed=$(echo "$line_content" | sed 's/^[[:space:]]*//')
    echo "::error file=$file,line=$lineno::Unregistered lint exception: $trimmed"
    echo "FAIL: $file:$lineno — unregistered lint exception" >&2
    echo "  → Line: $trimmed" >&2
    echo "  → Add an entry to $YAML or remove the annotation." >&2
    exit_code=1
    unregistered=$((unregistered + 1))
  fi
done < <(grep -rn '#\[expect\|#\[allow\|#!\[expect\|#!\[allow' --include='*.rs' src/ crates/ 2>/dev/null \
  | grep -v '/target/' \
  | grep -vE 'reason\s*=\s*".*#\[(allow|expect)' )

# ── Rule 2 & 3: Every YAML entry must match exactly one annotation ────────
orphans=0
ambiguous=0
for (( i=0; i<entry_count; i++ )); do
  count=${match_counts[$i]}
  if [ "$count" -eq 0 ]; then
    echo "::error::Orphan YAML entry: ${paths[$i]} match='${matches[$i]}'"
    echo "FAIL: Orphan entry in $YAML — no annotation found" >&2
    echo "  → path: ${paths[$i]}" >&2
    echo "  → match: '${matches[$i]}'" >&2
    echo "  → Remove the entry or restore the annotation." >&2
    exit_code=1
    orphans=$((orphans + 1))
  elif [ "$count" -gt 1 ]; then
    echo "::error::Ambiguous YAML entry matches $count annotations: ${paths[$i]} match='${matches[$i]}'"
    echo "FAIL: Ambiguous entry in $YAML — matches $count annotations" >&2
    echo "  → path: ${paths[$i]}" >&2
    echo "  → match: '${matches[$i]}'" >&2
    echo "  → Narrow the match string to be unique." >&2
    exit_code=1
    ambiguous=$((ambiguous + 1))
  fi
done

# ── Summary ───────────────────────────────────────────────────────────────
if [ "$exit_code" -eq 0 ]; then
  echo "All $entry_count lint exceptions registered and verified. ✓"
else
  echo "" >&2
  [ "$unregistered" -gt 0 ] && echo "  $unregistered unregistered annotation(s) in code." >&2
  [ "$orphans" -gt 0 ] && echo "  $orphans orphan entry(ies) in YAML (no matching annotation)." >&2
  [ "$ambiguous" -gt 0 ] && echo "  $ambiguous ambiguous entry(ies) in YAML (multiple matches)." >&2
  echo "Fix the above and re-run." >&2
fi
exit $exit_code
