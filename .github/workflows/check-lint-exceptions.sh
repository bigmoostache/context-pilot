#!/usr/bin/env bash
# Check that every #[expect(...)] and #[allow(...)] annotation in Rust source
# files is registered in the curated exceptions YAML.
#
# Usage: .github/workflows/check-lint-exceptions.sh
# Exit code 0 = all annotations are registered. Non-zero = unregistered found.
set -euo pipefail

YAML=".github/workflows/allowed-lint-exceptions.yaml"
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

exit_code=0
unregistered=0

# Collect all #[allow( / #[expect( / #![allow( / #![expect( lines
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
    ((unregistered++))
  fi
done < <(grep -rn '#\[expect\|#\[allow\|#!\[expect\|#!\[allow' --include='*.rs' src/ crates/ 2>/dev/null \
  | grep -v '/target/' \
  | grep -vE 'reason\s*=\s*".*#\[(allow|expect)' )

if [ "$exit_code" -eq 0 ]; then
  echo "All lint exceptions are registered in $YAML. ✓"
else
  echo "" >&2
  echo "Found $unregistered unregistered lint exception(s)." >&2
  echo "Register them in $YAML or remove the annotations." >&2
fi
exit $exit_code
