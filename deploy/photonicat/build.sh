#!/usr/bin/env bash
# Build the appliance bundle LOCALLY — a stand-in for a GitHub release while you
# can't cut one. Produces the SAME `cpilot-linux-aarch64-musl.tar.gz` that
# .github/workflows/release.yml publishes (flat layout: cpilot, cp-console-server,
# cp-orchestrator, web/<spa> — no meilisearch, none is built upstream for musl)
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
# Pinned Caddy release + sha512 of its tarball (from caddy_<v>_checksums.txt).
# Keep in step with caddy_version / caddy_sha512 in deploy/ansible/site.yml.
CADDY_VERSION="2.11.7"
CADDY_SHA512="3db36ba90c7a6e8dda40ee3dd71fa08844c76b5fb08f61b31e5e78d2ed38e71c51dc7baed875e50d1ca1279196e84302967237386ae87c91ae9f2aaceada682e"
CADDY_TGZ="caddy_${CADDY_VERSION}_linux_arm64.tar.gz"
CADDY_URL="https://github.com/caddyserver/caddy/releases/download/v${CADDY_VERSION}/${CADDY_TGZ}"

echo "==> cross-building orchestrator + console-server + agent ($TARGET, static musl)"
# --cap-lints warn (NOT allow): allow breaks cross's cargo-metadata step.
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" -p cp-orchestrator
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" -p cp-console-server
RUSTFLAGS="--cap-lints warn" cross build --release --target "$TARGET" --bin tui

echo "==> building SPA (relative same-origin API URLs)"
( cd web && VITE_API_URL="" npm run build )

echo "==> packaging cpilot-linux-aarch64-musl.tar.gz (matches release.yml flat layout)"
rm -rf "$ART/staging"
mkdir -p "$ART/staging/web"
cp "target/$TARGET/release/tui" "$ART/staging/cpilot"
cp "target/$TARGET/release/cp-console-server" "$ART/staging/cp-console-server"
cp "target/$TARGET/release/cp-orchestrator" "$ART/staging/cp-orchestrator"
chmod 755 "$ART/staging/cpilot" "$ART/staging/cp-console-server" "$ART/staging/cp-orchestrator"
cp -r web/dist/. "$ART/staging/web/"
tar -czf "$ART/cpilot-linux-aarch64-musl.tar.gz" -C "$ART/staging" .
rm -rf "$ART/staging"

echo "==> fetching pinned Caddy v$CADDY_VERSION arm64 (static Go, ships tls internal) if absent"
if [ ! -f "$ART/caddy" ]; then
  curl -fsSL "$CADDY_URL" -o "$ART/$CADDY_TGZ"
  if command -v sha512sum >/dev/null 2>&1; then
    got="$(sha512sum "$ART/$CADDY_TGZ" | cut -d' ' -f1)"
  else
    got="$(shasum -a 512 "$ART/$CADDY_TGZ" | cut -d' ' -f1)"
  fi
  if [ "$got" != "$CADDY_SHA512" ]; then
    rm -f "$ART/$CADDY_TGZ"
    echo "ERROR: Caddy tarball sha512 mismatch (got $got)" >&2
    exit 1
  fi
  tar -xzf "$ART/$CADDY_TGZ" -C "$ART" caddy
  rm -f "$ART/$CADDY_TGZ"
  chmod +x "$ART/caddy"
fi

echo "==> done. Artifacts in $ART/:"
ls -la "$ART/"
echo
echo "Next: ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml -e release=local"
