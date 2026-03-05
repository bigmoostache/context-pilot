#!/usr/bin/env bash
# Check that no .rs file exceeds 500 lines.
# Usage: .github/checks/check-file-lengths.sh
set -euo pipefail

exit_code=0
while IFS= read -r f; do
  lines=$(wc -l < "$f")
  if [ "$lines" -gt 500 ]; then
    echo "::error file=$f::$f has $lines lines (max 500)"
    echo "FAIL: $f has $lines lines (max 500)" >&2
    echo "  → Refactor: extract functions, types, or logic into a new sibling file/module." >&2
    echo "  → Do NOT compress code, remove comments, or reduce readability to fit." >&2
    exit_code=1
  fi
done < <(find . -name '*.rs' -not -path './target/*' -not -path '*/target/*')
exit $exit_code
