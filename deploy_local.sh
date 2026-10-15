#!/bin/bash
# deploy_local.sh — Build and install Context Pilot as a standalone binary
#
# Usage: ./deploy_local.sh
#
# What it does:
#   1. Builds the release binary (as current user — uses YOUR rustup toolchain)
#   2. Copies it to /usr/local/bin/cpilot (elevates to sudo only for this step),
#      refreshes the active release slot, and sha256-verifies every copy
#   3. Sets up global gitignore for .context-pilot/
#   4. Checks for a .env (loaded by cpilot itself; secrets are never written
#      into ~/.bashrc)
#
# NOTE: Do NOT run this script with sudo. It will request elevation only
#       for the install step. Building under sudo uses root's toolchain,
#       which may not have the same Rust version.
#
# After running, use from any project:
#   cd /path/to/project && cpilot

set -e

# Guard against running the whole script as root.
if [ "$(id -u)" -eq 0 ]; then
  echo "ERROR: Do not run this script with sudo." >&2
  echo "  It will request elevation only for the install step." >&2
  echo "  Usage: ./deploy_local.sh" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_PATH="/usr/local/bin/cpilot"
GITIGNORE_GLOBAL="$HOME/.gitignore_global"

echo "=== Context Pilot — Local Deploy ==="
echo ""

# 1. Build release binary
echo "[1/4] Building release binary..."
cd "$SCRIPT_DIR"

# Prefer system OpenSSL over vendored build (avoids Perl dependency maze)
if pkg-config --exists openssl 2>/dev/null; then
    export OPENSSL_NO_VENDOR=1
fi

cargo build --release
cargo build --release -p cp-console-server
echo "      Build complete."

# 2. Install binary (sudo only for this step)
echo "[2/4] Installing to $INSTALL_PATH..."
# Remove old binary first to avoid "Text file busy" when the running process holds it open.
# The running process keeps its inode alive, but the directory entry is freed for the new copy.
sudo rm -f "$INSTALL_PATH"
sudo cp "$SCRIPT_DIR/target/release/tui" "$INSTALL_PATH"
sudo chmod +x "$INSTALL_PATH"
# Also install the console server binary alongside
INSTALL_DIR="$(dirname "$INSTALL_PATH")"
sudo rm -f "$INSTALL_DIR/cp-console-server"
sudo cp "$SCRIPT_DIR/target/release/cp-console-server" "$INSTALL_DIR/cp-console-server"
sudo chmod +x "$INSTALL_DIR/cp-console-server"

# sha256 of a file (macOS ships shasum, Linux sha256sum).
file_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# Fail unless $2 is byte-identical to $1. A stale install looks like a working
# fix that "didn't take", so never report success without this check.
verify_copy() {
    local want got
    want="$(file_sha256 "$1")"
    got="$(file_sha256 "$2")"
    if [ "$want" != "$got" ]; then
        echo "ERROR: $2 does not match $1 (got $got, want $want)" >&2
        exit 1
    fi
    echo "      verified $2 (sha256 ${got:0:12})"
}

verify_copy "$SCRIPT_DIR/target/release/tui" "$INSTALL_PATH"
verify_copy "$SCRIPT_DIR/target/release/cp-console-server" "$INSTALL_DIR/cp-console-server"
echo "      Installed. ($(du -h "$INSTALL_PATH" | cut -f1))"

# Refresh the active release slot too: system_reload re-execs the agent from
# ~/.context-pilot/releases/<active_tag>/cpilot, not from $INSTALL_PATH, so
# skipping it leaves reloaded agents on the old build.
RELEASES_DIR="$HOME/.context-pilot/releases"
ACTIVE_TAG="$(sed -nE 's/.*"active_tag"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' "$RELEASES_DIR/config.json" 2>/dev/null | head -n1)"
SLOT_DIR="$RELEASES_DIR/$ACTIVE_TAG"
if [ -n "$ACTIVE_TAG" ] && [ -d "$SLOT_DIR" ]; then
    echo "      Refreshing release slot $ACTIVE_TAG..."
    for pair in "tui:cpilot" "cp-console-server:cp-console-server"; do
        src="$SCRIPT_DIR/target/release/${pair%%:*}"
        dst="$SLOT_DIR/${pair##*:}"
        # Keep the previous binary for rollback; rm before cp avoids "Text file busy".
        if [ -f "$dst" ]; then
            cp -p "$dst" "$dst.bak"
            rm -f "$dst"
        fi
        cp "$src" "$dst"
        chmod +x "$dst"
        verify_copy "$src" "$dst"
    done
else
    echo "      No active release slot found under $RELEASES_DIR — slot refresh skipped."
fi

# 3. Global gitignore
echo "[3/4] Setting up global gitignore..."
touch "$GITIGNORE_GLOBAL"
# Use .context-pilot/* (not .context-pilot/) so git enters the directory,
# allowing the negation !.context-pilot/shared/ to track shared configs.
if grep -qxF ".context-pilot/*" "$GITIGNORE_GLOBAL" 2>/dev/null; then
    # Pattern exists — just ensure all negations are present
    for dir in shared agents skills commands; do
        if ! grep -qxF "!.context-pilot/$dir/" "$GITIGNORE_GLOBAL" 2>/dev/null; then
            echo "!.context-pilot/$dir/" >> "$GITIGNORE_GLOBAL"
            echo "      Added !.context-pilot/$dir/ to $GITIGNORE_GLOBAL"
        fi
    done
    echo "      .context-pilot/* already in $GITIGNORE_GLOBAL — negations verified."
else
    # Migrate old pattern if present
    if grep -qxF ".context-pilot/" "$GITIGNORE_GLOBAL" 2>/dev/null; then
        sed -i.bak 's|^\.context-pilot/$|.context-pilot/*|' "$GITIGNORE_GLOBAL"
        rm -f "$GITIGNORE_GLOBAL.bak"
        echo "      Migrated .context-pilot/ → .context-pilot/* in $GITIGNORE_GLOBAL"
    else
        echo ".context-pilot/*" >> "$GITIGNORE_GLOBAL"
    fi
    # Ensure negations are present for shared dirs (shared/, agents/, skills/, commands/)
    for dir in shared agents skills commands; do
        if ! grep -qxF "!.context-pilot/$dir/" "$GITIGNORE_GLOBAL" 2>/dev/null; then
            echo "!.context-pilot/$dir/" >> "$GITIGNORE_GLOBAL"
        fi
    done
    echo "      Set up .context-pilot/* + negations for shared/agents/skills/commands in $GITIGNORE_GLOBAL"
fi
git config --global core.excludesFile "$GITIGNORE_GLOBAL"

# 4. API keys: never copy .env secrets into shell rc files (plaintext, sourced
# by every shell, readable by every child process). cpilot loads .env itself
# via dotenvy, with ~/.context-pilot/.env as the global fallback.
echo "[4/4] Checking API keys..."
if [ -f "$SCRIPT_DIR/.env" ] || [ -f "$HOME/.context-pilot/.env" ]; then
    echo "      .env found — cpilot loads it at startup, nothing to export."
else
    echo "      No .env found. Put API keys (e.g. ANTHROPIC_API_KEY) in"
    echo "      ~/.context-pilot/.env (chmod 600); cpilot loads it at startup."
fi
if grep -qE '^export [A-Z0-9_]*(KEY|TOKEN|SECRET)[A-Z0-9_]*=' "$HOME/.bashrc" 2>/dev/null; then
    echo "      WARNING: ~/.bashrc exports secrets (older deploy_local.sh wrote them)."
    echo "      Consider removing those lines; they are no longer needed."
fi

echo ""
echo "=== Done! ==="
echo ""
echo "Usage:"
echo "  cd /path/to/any/project"
echo "  cpilot"
