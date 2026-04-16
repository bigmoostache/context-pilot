#!/usr/bin/env bash
# Check that no directory has more than 8 entries.
# Usage: .github/checks/check-folder-sizes.sh
set -euo pipefail

exit_code=0
while IFS= read -r dir; do
  count=$(find "$dir" -maxdepth 1 -mindepth 1 | wc -l)
  if [ "$count" -gt 8 ]; then
    echo "::error::$dir has $count entries (max 8)"
    echo "FAIL: $dir has $count entries (max 8)" >&2
    echo "  → Refactor: group related files into a sub-directory or merge small files." >&2
    echo "  → Do NOT rename, flatten, or delete files just to fit the limit." >&2
    exit_code=1
  fi
done < <(find . -mindepth 1 -type d \
  -not -path './target/*' \
  -not -path '*/target/*' \
  -not -path './.git' \
  -not -path './.git/*' \
  -not -path './crates' \
  -not -path './.context-pilot' \
  -not -path './.context-pilot/*' \
  -not -path './website/*' \
  -not -path './docs' \
  -not -path './docs/*' \
  -not -path './brilliant-cv/*' \
  -not -path './graceful-genetics/*' \
  -not -path './test-typst/*' \
  -not -path './.github/workflows' \
  -not -path './.github/checks' \
  -not -path './yamls/tools' \
  -not -path './crates/cp-base/src/state')
exit $exit_code
