#!/usr/bin/env bash
# run-stack.sh — Launch the full Context Pilot live stack:
#   1. Orchestrator backend   (port 7878)
#   2. Web client dev server  (port 5174)
#   3. TUI with bridge active (CP_BRIDGE=1)
#
# Usage:
#   ./web/run-stack.sh            # all three
#   ./web/run-stack.sh --no-tui   # backend + web only (run TUI separately)
#
# Prerequisites:
#   - Rust toolchain (cargo)
#   - Node ≥ 18 + pnpm
#   - ANTHROPIC_API_KEY in .env or environment
#
# Environment (override via .env or export):
#   CP_ORCH_PORT   — backend port         (default 7878)
#   VITE_API_URL   — backend URL for web  (default http://localhost:7878)
set -euo pipefail
cd "$(dirname "$0")/.."

# ── Parse flags ──────────────────────────────────────────────────────
RUN_TUI=true
for arg in "$@"; do
  case "$arg" in
    --no-tui) RUN_TUI=false ;;
    *) echo "Unknown flag: $arg" >&2; exit 1 ;;
  esac
done

# ── Cleanup on exit ──────────────────────────────────────────────────
PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT

# ── 1. Build + start orchestrator backend ────────────────────────────
echo "▸ Building orchestrator…"
cargo build --release -p cp-orchestrator 2>&1 | tail -1

echo "▸ Starting orchestrator backend on port ${CP_ORCH_PORT:-7878}…"
./target/release/cp-orchestrator &
PIDS+=($!)
sleep 0.5

# Verify health
if curl -sf http://localhost:"${CP_ORCH_PORT:-7878}"/api/health > /dev/null 2>&1; then
  echo "  ✓ Backend healthy"
else
  echo "  ✗ Backend failed to start" >&2
  exit 1
fi

# ── 2. Start web dev server ──────────────────────────────────────────
echo "▸ Starting web dev server…"
(cd web && pnpm dev --port 5174) &
PIDS+=($!)
sleep 2
echo "  ✓ Web client at http://localhost:5174"

# ── 3. Optionally start TUI with bridge ─────────────────────────────
if [ "$RUN_TUI" = true ]; then
  echo "▸ Starting TUI with bridge activation (CP_BRIDGE=1)…"
  CP_BRIDGE=1 ./target/release/tui &
  PIDS+=($!)
  echo "  ✓ TUI launched with bridge"
fi

# ── Ready ────────────────────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════"
echo "  Context Pilot live stack running"
echo "  Backend:  http://localhost:${CP_ORCH_PORT:-7878}"
echo "  Web:      http://localhost:5174"
if [ "$RUN_TUI" = true ]; then
  echo "  TUI:      running with CP_BRIDGE=1"
fi
echo ""
echo "  Press Ctrl+C to stop all services"
echo "══════════════════════════════════════════════════"

# Wait for any child to exit
wait
