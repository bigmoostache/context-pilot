#!/usr/bin/env bash
# Build the appliance bundle LOCALLY — a stand-in for a GitHub release while you
# can't cut one. Produces the SAME `cpilot-appliance-aarch64.tar.gz` that
# .github/workflows/release.yml publishes (bin/cp-orchestrator, bin/tui, web/<spa>)
# plus a stock Caddy arm64, into deploy/ansible/.artifacts/, so the Ansible
# playbook can deploy it with `-e release=local` (no GitHub fetch).
#
# Prereqs on the dev box: rustup + `cross` + docker, node/npm, curl.
# Usage (from anywhere in the repo):
#   deploy/photonicat/build.sh
#   ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml -e release=local
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
TARGET=aarch64-unknown-linux-musl
ART=deploy/ansible/.artifacts
CADDY_URL="https://caddyserver.com/api/download?os=linux&arch=arm64"

echo "==> cross-building orchestrator + agent ($TARGET, static musl)"
# --cap-lints warn (NOT allow): allow breaks cross's cargo-metadata step.
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" -p cp-orchestrator
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" --bin tui

echo "==> building SPA (relative same-origin API URLs)"
( cd web && VITE_API_URL="" npm run build )

echo "==> packaging cpilot-appliance-aarch64.tar.gz (matches release.yml layout)"
rm -rf "$ART/staging"
mkdir -p "$ART/staging/bin" "$ART/staging/web"
cp "target/$TARGET/release/cp-orchestrator" "$ART/staging/bin/"
cp "target/$TARGET/release/tui" "$ART/staging/bin/"
chmod 755 "$ART/staging/bin/"*
cp -r web/dist/. "$ART/staging/web/"
tar -czf "$ART/cpilot-appliance-aarch64.tar.gz" -C "$ART/staging" .
rm -rf "$ART/staging"

echo "==> fetching stock Caddy arm64 (static Go, ships tls internal) if absent"
if [ ! -f "$ART/caddy" ]; then
  curl -fsSL "$CADDY_URL" -o "$ART/caddy"
  chmod +x "$ART/caddy"
fi

echo "==> done. Artifacts in $ART/:"
ls -la "$ART/"
echo
echo "Next: ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml -e release=local"
