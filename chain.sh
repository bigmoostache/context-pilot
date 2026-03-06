#!/usr/bin/env bash
# Convenience wrapper to update the protected files hash chain.
#
# Usage:
#   ./chain.sh                      # prompts for password interactively
#   ./chain.sh --password SECRET    # non-interactive (e.g. CI setup)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CHECK_SCRIPT="$SCRIPT_DIR/.github/checks/check-lint-config.sh"

# Parse arguments.
password=""
while [ $# -gt 0 ]; do
  case "$1" in
    --password)
      password="${2:-}"
      if [ -z "$password" ]; then
        echo "FAIL: --password requires a value." >&2
        exit 1
      fi
      shift 2
      ;;
    *)
      echo "Usage: $0 [--password SECRET]" >&2
      exit 1
      ;;
  esac
done

# Prompt if no password provided.
if [ -z "$password" ]; then
  read -r -s -p "Password: " password
  echo ""
  if [ -z "$password" ]; then
    echo "FAIL: Password cannot be empty." >&2
    exit 1
  fi
fi

# Step 1: Verify the current chain is intact.
echo "→ Verifying current chain..."
if ! bash "$CHECK_SCRIPT" 2>&1; then
  echo ""
  echo "Chain verification failed — see above. Proceeding with update anyway"
  echo "(the update will record the new state)."
  echo ""
fi

# Step 2: Update the chain with the password.
echo "→ Updating chain..."
LINT_GUARD_PASSWORD="$password" bash "$CHECK_SCRIPT" --update 2>&1
