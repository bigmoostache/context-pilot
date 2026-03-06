#!/bin/bash
# deploy_local.sh — Build and install Context Pilot as a standalone binary
#
# Usage: ./deploy_local.sh
#
# What it does:
#   1. Builds the release binary (as current user — uses YOUR rustup toolchain)
#   2. Copies it to /usr/local/bin/cpilot (elevates to sudo only for this step)
#   3. Sets up global gitignore for .context-pilot/
#   4. Exports API keys from .env to ~/.bashrc (if not already there)
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
echo "      Installed. ($(du -h "$INSTALL_PATH" | cut -f1))"

# 3. Global gitignore
echo "[3/4] Setting up global gitignore..."
touch "$GITIGNORE_GLOBAL"
if grep -qxF ".context-pilot/" "$GITIGNORE_GLOBAL" 2>/dev/null; then
    echo "      .context-pilot/ already in $GITIGNORE_GLOBAL — skipping."
else
    echo ".context-pilot/" >> "$GITIGNORE_GLOBAL"
    echo "      Added .context-pilot/ to $GITIGNORE_GLOBAL"
fi
git config --global core.excludesFile "$GITIGNORE_GLOBAL"

# 4. Export API keys from .env
echo "[4/4] Checking API keys in ~/.bashrc..."
if [ -f "$SCRIPT_DIR/.env" ]; then
    KEYS_ADDED=0
    while IFS= read -r line; do
        # Skip comments and empty lines
        [[ -z "$line" || "$line" == \#* ]] && continue
        KEY_NAME="${line%%=*}"
        if ! grep -q "export $KEY_NAME=" "$HOME/.bashrc" 2>/dev/null; then
            echo "export $line" >> "$HOME/.bashrc"
            echo "      Added $KEY_NAME to ~/.bashrc"
            KEYS_ADDED=$((KEYS_ADDED + 1))
        fi
    done < "$SCRIPT_DIR/.env"
    if [ "$KEYS_ADDED" -eq 0 ]; then
        echo "      All API keys already in ~/.bashrc — skipping."
    fi
else
    echo "      No .env file found — skipping API key export."
    echo "      You'll need to set API keys manually (e.g., ANTHROPIC_API_KEY)."
fi

echo ""
echo "=== Done! ==="
echo ""
echo "Usage:"
echo "  cd /path/to/any/project"
echo "  cpilot"
echo ""
echo "If this is a new shell session, run: source ~/.bashrc"
