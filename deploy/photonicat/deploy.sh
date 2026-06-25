#!/usr/bin/env bash
# Native deploy of the Context Pilot orchestrator to the Photonicat (photonicatWrt
# / OpenWrt, aarch64 musl). No Docker: a single static binary runs under procd
# and serves both the API and the SPA on one port.
#
# Prereqs on the dev box: rustup + the aarch64-unknown-linux-musl target, `cross`
# + docker (for the cross toolchain image), node/npm. Run from the repo root:
#   deploy/photonicat/deploy.sh [HOST]
set -euo pipefail

HOST="${1:-root@192.168.1.116}"
ROOT="/mnt/data/context-pilot"
TARGET=aarch64-unknown-linux-musl
BIN="target/$TARGET/release/cp-orchestrator"

cd "$(git rev-parse --show-toplevel)"

echo "==> cross-building orchestrator + agent ($TARGET, static musl)"
# --cap-lints warn (NOT allow): allow breaks cross's cargo-metadata step, see
# the soft_unstable lint note. This downgrades the removed-lint forbid to a warn.
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" -p cp-orchestrator
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" --bin tui

echo "==> building SPA (relative same-origin API URLs)"
( cd web && VITE_API_URL="" npm run build )

echo "==> shipping to $HOST"
ssh "$HOST" "mkdir -p $ROOT/bin $ROOT/home $ROOT/web"
scp -q "$BIN" "$HOST:$ROOT/bin/cp-orchestrator"
scp -q "target/$TARGET/release/tui" "$HOST:$ROOT/bin/tui"
scp -qr web/dist/. "$HOST:$ROOT/web/"
scp -q deploy/photonicat/context-pilot.init "$HOST:/etc/init.d/context-pilot"

echo "==> installing + (re)starting procd service"
ssh "$HOST" '
  chmod +x /mnt/data/context-pilot/bin/cp-orchestrator /mnt/data/context-pilot/bin/tui /etc/init.d/context-pilot
  /etc/init.d/context-pilot enable
  /etc/init.d/context-pilot restart
  sleep 2
  echo -n "health: "; wget -qO- http://127.0.0.1:7878/api/health; echo
  echo -n "auth:   "; wget -qO- http://127.0.0.1:7878/api/auth/status; echo
'
echo "==> done — cockpit at http://${HOST#*@}:7878/"
