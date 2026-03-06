#!/usr/bin/env bash
# Guard protected files with a cryptographic hash chain.
#
# Reads .github/checks/protected-files.yaml to determine WHAT to protect.
# Computes a combined SHA-256 fingerprint of all declared files/sections.
#
# Each chain entry: <seq>:<prev_chain_hash>:<content_hash>:<chain_hash>
# Where chain_hash = SHA-256(seq + prev_chain_hash + content_hash + password)
#
# Usage:
#   .github/checks/check-lint-config.sh            # verify (CI)
#   .github/checks/check-lint-config.sh --update    # append new entry (needs password)
#   .github/checks/check-lint-config.sh --history   # show the chain log
#   .github/checks/check-lint-config.sh --show      # show what's being protected
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CHAIN_FILE="$SCRIPT_DIR/lint-config.chain"
MANIFEST="$SCRIPT_DIR/protected-files.yaml"

# --- YAML parser (pure bash, no dependencies) ---
# Parses protected-files.yaml and extracts content for hashing.
# Supports two modes per entry:
#   whole_file: true  → hash entire file
#   sections:         → extract lines between start/end_before markers
compute_protected_content() {
  if [ ! -f "$MANIFEST" ]; then
    echo "FAIL: Missing $MANIFEST" >&2
    exit 1
  fi

  local current_path="" in_sections=0 whole_file=0
  local section_start="" section_end=""

  while IFS= read -r line || [ -n "$line" ]; do
    # Strip leading whitespace for matching.
    local trimmed="${line#"${line%%[![:space:]]*}"}"

    # Skip comments and blank lines.
    [[ -z "$trimmed" || "$trimmed" == \#* ]] && continue

    # New entry: "- path: ..."
    if [[ "$trimmed" =~ ^-\ path:\ (.+)$ ]]; then
      # Flush previous entry if it was whole_file.
      if [ -n "$current_path" ] && [ "$whole_file" -eq 1 ]; then
        flush_whole_file "$current_path"
      fi
      current_path="${BASH_REMATCH[1]}"
      in_sections=0
      whole_file=0
      section_start=""
      section_end=""
      continue
    fi

    # "whole_file: true"
    if [[ "$trimmed" == "whole_file: true" ]]; then
      whole_file=1
      continue
    fi

    # "sections:" header
    if [[ "$trimmed" == "sections:" ]]; then
      in_sections=1
      continue
    fi

    # Section start marker: "- start: ..."
    if [ "$in_sections" -eq 1 ] && [[ "$trimmed" =~ ^-\ start:\ (.+)$ ]]; then
      section_start="${BASH_REMATCH[1]}"
      # Strip surrounding quotes.
      section_start="${section_start#\"}"
      section_start="${section_start%\"}"
      continue
    fi

    # Section end marker: "end_before: ..."
    if [ "$in_sections" -eq 1 ] && [[ "$trimmed" =~ ^end_before:\ (.+)$ ]]; then
      section_end="${BASH_REMATCH[1]}"
      section_end="${section_end#\"}"
      section_end="${section_end%\"}"
      # We have both start and end — extract the section.
      flush_section "$current_path" "$section_start" "$section_end"
      section_start=""
      section_end=""
      continue
    fi
  done < "$MANIFEST"

  # Flush last entry.
  if [ -n "$current_path" ] && [ "$whole_file" -eq 1 ]; then
    flush_whole_file "$current_path"
  fi
}

flush_whole_file() {
  local fpath="$ROOT/$1"
  if [ ! -f "$fpath" ]; then
    echo "WARNING: Protected file not found: $1" >&2
    echo "MISSING:$1"
    return
  fi
  echo "===FILE:$1==="
  cat "$fpath"
}

flush_section() {
  local fpath="$ROOT/$1" start_marker="$2" end_prefix="$3"
  if [ ! -f "$fpath" ]; then
    echo "WARNING: Protected file not found: $1" >&2
    echo "MISSING:$1"
    return
  fi
  echo "===SECTION:$1:$start_marker==="
  sed -n "/^\\${start_marker}/,/^\\${end_prefix}/p" "$fpath" | head -n -1
}

content_hash() {
  compute_protected_content | sha256sum | cut -d' ' -f1
}

compute_chain_hash() {
  local seq="$1" prev="$2" ch="$3" password="$4"
  printf '%s:%s:%s:%s' "$seq" "$prev" "$ch" "$password" | sha256sum | cut -d' ' -f1
}

# --- Show mode: display what's protected ---
if [ "${1:-}" = "--show" ]; then
  echo "=== Protected Files ==="
  compute_protected_content | grep -E '^===(FILE|SECTION|MISSING):' | while IFS= read -r line; do
    echo "  $line"
  done
  echo ""
  echo "Content hash: $(content_hash)"
  exit 0
fi

# --- History mode ---
if [ "${1:-}" = "--history" ]; then
  if [ ! -f "$CHAIN_FILE" ]; then
    echo "No chain file yet."
    exit 0
  fi
  echo "=== Protected Files Chain ==="
  while IFS=: read -r seq prev ch chain; do
    echo "  #$seq  content=${ch:0:12}…  chain=${chain:0:12}…  prev=${prev:0:12}…"
  done < "$CHAIN_FILE"
  exit 0
fi

# --- Update mode: password required ---
if [ "${1:-}" = "--update" ]; then
  password="${LINT_GUARD_PASSWORD:-}"
  if [ -z "$password" ]; then
    read -r -s -p "Password: " password
    echo ""
  fi
  if [ -z "$password" ]; then
    echo "FAIL: Password cannot be empty." >&2
    echo "  Set LINT_GUARD_PASSWORD or enter it interactively." >&2
    exit 1
  fi

  ch=$(content_hash)

  if [ ! -f "$CHAIN_FILE" ]; then
    seq=1
    prev="GENESIS"
    chain=$(compute_chain_hash "$seq" "$prev" "$ch" "$password")
    echo "${seq}:${prev}:${ch}:${chain}" > "$CHAIN_FILE"
    echo "Genesis block created (entry #1) ✓"
    exit 0
  fi

  last_line=$(tail -n 1 "$CHAIN_FILE")
  IFS=: read -r last_seq last_prev last_ch last_chain <<< "$last_line"

  # Verify password against the last chain entry.
  expected_last=$(compute_chain_hash "$last_seq" "$last_prev" "$last_ch" "$password")
  if [ "$expected_last" != "$last_chain" ]; then
    echo "FAIL: Wrong password." >&2
    exit 1
  fi

  if [ "$ch" = "$last_ch" ]; then
    echo "No changes detected — chain already up to date."
    exit 0
  fi

  seq=$((last_seq + 1))
  prev="$last_chain"
  chain=$(compute_chain_hash "$seq" "$prev" "$ch" "$password")
  echo "${seq}:${prev}:${ch}:${chain}" >> "$CHAIN_FILE"
  echo "Appended entry #$seq to chain ✓"
  exit 0
fi

# --- Verify mode: no password needed ---
if [ ! -f "$CHAIN_FILE" ]; then
  echo "::error::No protected files chain found at $CHAIN_FILE"
  echo "FAIL: Missing $CHAIN_FILE — run: .github/checks/check-lint-config.sh --update" >&2
  exit 1
fi

# Verify chain link integrity.
prev_chain="GENESIS"
entry_count=0
while IFS=: read -r seq prev ch chain; do
  entry_count=$((entry_count + 1))
  if [ "$prev" != "$prev_chain" ]; then
    echo "::error::Chain broken at entry #$seq: prev=$prev expected=$prev_chain"
    echo "FAIL: Chain integrity violation at entry #$seq." >&2
    exit 1
  fi
  prev_chain="$chain"
done < "$CHAIN_FILE"

if [ "$entry_count" -eq 0 ]; then
  echo "::error::Chain file is empty."
  echo "FAIL: $CHAIN_FILE is empty — run: .github/checks/check-lint-config.sh --update" >&2
  exit 1
fi

# Verify current content matches the latest entry.
last_line=$(tail -n 1 "$CHAIN_FILE")
IFS=: read -r last_seq last_prev last_ch last_chain <<< "$last_line"
current_ch=$(content_hash)

if [ "$current_ch" != "$last_ch" ]; then
  echo "::error::Protected files have been modified!"
  echo "FAIL: Protected content changed (chain entry #$last_seq)." >&2
  echo "" >&2
  echo "  If this change is INTENTIONAL, update the chain:" >&2
  echo "    .github/checks/check-lint-config.sh --update" >&2
  echo "" >&2
  echo "  If this change is NOT intentional, revert the modified files." >&2
  echo "  Run --show to see what's protected." >&2
  exit 1
fi

echo "Protected files chain verified ($entry_count entries). ✓"
