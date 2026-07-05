#!/usr/bin/env bash
# Check that no .rs file (outside cp-vault) directly accesses vault-managed
# credential keys via std::env::var().  All credential access MUST go through
# cp-vault: vault().get("key") or vault().require("key").
#
# Usage (CI):       .github/checks/check-vault-bypass.sh
# Usage (callback): .github/checks/check-vault-bypass.sh [file ...]
#
# When called without arguments, scans the entire repo.  When called with
# explicit file paths (callback mode), only checks those files.
set -euo pipefail

# Credential env-var patterns that MUST go through cp-vault.
# Matches: _API_KEY, _BOT_TOKEN, GITHUB_TOKEN, _API_HASH, _API_ID
PATTERN='env::var.*\(_API_KEY\|_BOT_TOKEN\|GITHUB_TOKEN\|_API_HASH\|_API_ID\)'

exit_code=0

check_file() {
  local file="$1"

  # Skip cp-vault itself — it's the authorised accessor.
  case "$file" in
    */cp-vault/* | crates/cp-vault/*) return ;;
  esac

  # Only .rs files.
  case "$file" in
    *.rs) ;;
    *) return ;;
  esac

  [ -f "$file" ] || return 0

  hits=$(grep -n "$PATTERN" "$file" 2>/dev/null || true)
  if [ -n "$hits" ]; then
    while IFS= read -r line; do
      echo "::error file=$file::$line"
    done <<< "$hits"
    echo "FAIL: $file bypasses cp-vault — use vault().get() or vault().require()" >&2
    exit_code=1
  fi
}

if [ $# -gt 0 ]; then
  # Callback mode: check only the supplied files.
  for file in "$@"; do
    check_file "$file"
  done
else
  # CI mode: scan the entire repo.
  while IFS= read -r file; do
    check_file "$file"
  done < <(find . -name '*.rs' -not -path './target/*' -not -path '*/target/*')
fi

if [ $exit_code -ne 0 ]; then
  echo "" >&2
  echo "All credential keys must be accessed via cp-vault." >&2
  echo "Replace env::var(\"KEY\") with vault().get(\"key\") or vault().require(\"key\")." >&2
fi

exit $exit_code
