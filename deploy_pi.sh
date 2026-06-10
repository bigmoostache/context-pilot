#!/bin/bash
# deploy_pi.sh — Cross-compile and deploy Context Pilot (Nestor) to the Raspberry Pi
#
# Usage: ./deploy_pi.sh [--no-build] [--web]
#
# What it does:
#   1. Cross-compiles tui + cp-console-server for aarch64 (via cross + Docker)
#   2. Copies the binaries to the Pi (~/nestor/bin/) — atomic swap, no "Text file busy"
#   3. Installs the launcher scripts (nestor-tui, nestor-web)
#   4. With --web: also builds web/ (Vite) and deploys the SPA to ~/nestor/web-dist/
#
# Requirements (PC): rustup + target aarch64-unknown-linux-gnu, cross, Docker, SSH key on the Pi.
# Requirements (Pi): Raspberry Pi OS 64-bit, zram swap recommended.

set -e

PI_HOST="${PI_HOST:-huser@192.168.1.145}"
PI_DIR="${PI_DIR:-/home/huser/nestor}"
TARGET="aarch64-unknown-linux-gnu"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

export PATH="$HOME/.cargo/bin:$PATH"

NO_BUILD=0
WITH_WEB=0
for arg in "$@"; do
    case "$arg" in
        --no-build) NO_BUILD=1 ;;
        --web) WITH_WEB=1 ;;
        *) echo "Unknown flag: $arg" >&2; exit 1 ;;
    esac
done

echo "=== Nestor — Deploy to Pi ($PI_HOST) ==="

# 1. Cross-compile (512 MB on the Pi cannot link the workspace — always build on the PC)
if [ "$NO_BUILD" -eq 0 ]; then
    echo "[1/4] Cross-compiling for $TARGET..."
    cd "$SCRIPT_DIR"
    cross build --release --target "$TARGET" -p tui -p cp-console-server
else
    echo "[1/4] Skipping build (--no-build)."
fi

# 2. Copy binaries — write to .new then mv, so a running instance keeps its inode
echo "[2/4] Deploying binaries..."
ssh "$PI_HOST" "mkdir -p $PI_DIR/bin $PI_DIR/workspace"
scp -q "$SCRIPT_DIR/target/$TARGET/release/tui" "$PI_HOST:$PI_DIR/bin/cpilot.new"
scp -q "$SCRIPT_DIR/target/$TARGET/release/cp-console-server" "$PI_HOST:$PI_DIR/bin/cp-console-server.new"
ssh "$PI_HOST" "mv $PI_DIR/bin/cpilot.new $PI_DIR/bin/cpilot && mv $PI_DIR/bin/cp-console-server.new $PI_DIR/bin/cp-console-server && chmod +x $PI_DIR/bin/cpilot $PI_DIR/bin/cp-console-server"

# 3. Launcher scripts
echo "[3/4] Installing launchers..."
ssh "$PI_HOST" "cat > $PI_DIR/bin/nestor-tui" <<'EOF'
#!/bin/bash
# nestor-tui — launch the Context Pilot TUI on the Pi (SSH fallback client).
# The binary self-restarts via exec() on reload — no cargo supervisor needed.
cd "${NESTOR_WORKSPACE:-$HOME/nestor/workspace}"
[ -f "$HOME/nestor/.env" ] && export $(grep -v '^#' "$HOME/nestor/.env" | xargs)
ulimit -n 2048 2>/dev/null
exec "$HOME/nestor/bin/cpilot" "$@"
EOF
ssh "$PI_HOST" "cat > $PI_DIR/bin/nestor-web" <<'EOF'
#!/bin/bash
# nestor-web — launch Context Pilot headless with the web server (Nestor mode).
cd "${NESTOR_WORKSPACE:-$HOME/nestor/workspace}"
[ -f "$HOME/nestor/.env" ] && export $(grep -v '^#' "$HOME/nestor/.env" | xargs)
ulimit -n 2048 2>/dev/null
exec "$HOME/nestor/bin/cpilot" --headless --web-bind "${NESTOR_BIND:-192.168.1.145:8787}" --web-dist "$HOME/nestor/web-dist" "$@"
EOF
ssh "$PI_HOST" "chmod +x $PI_DIR/bin/nestor-tui $PI_DIR/bin/nestor-web"

# 4. Web SPA (optional)
if [ "$WITH_WEB" -eq 1 ]; then
    echo "[4/4] Building + deploying the web SPA..."
    cd "$SCRIPT_DIR/web"
    npm run build
    rsync -az --delete "$SCRIPT_DIR/web/dist/" "$PI_HOST:$PI_DIR/web-dist/"
else
    echo "[4/4] Skipping web SPA (use --web to deploy it)."
fi

echo ""
echo "=== Done! ==="
echo "  TUI over SSH : ssh -t $PI_HOST $PI_DIR/bin/nestor-tui"
echo "  Headless web : ssh $PI_HOST $PI_DIR/bin/nestor-web"
