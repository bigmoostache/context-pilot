#!/usr/bin/env bash
# fetch-lcd-deps.sh — (re)download the arm64 .deb the init-SD installer needs for
# the LCD progress screen, into ../deps/, straight from the Debian pool.
#
# WHY THIS EXISTS: the Armbian minimal image lacks python3-spidev +
# python3-libgpiod, and we cannot apt-install them cross-arch on an x86 build host
# (chroot DNS under qemu fails with getaddrinfo EBUSY). So the installer carries
# them as offline .deb and dpkg-installs them on the pcat. These two are the ONLY
# missing deps — xz and libgpiod2 already ship in the Armbian base image.
#
# Provenance, on purpose: the URLs + versions are pinned here so deps/ is a
# rebuildable artifact, not an opaque blob. Bump the versions when trixie moves.
#
# Usage:  ./build/fetch-lcd-deps.sh      # writes ../deps/*.deb
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DEPS="$HERE/../deps"
POOL="http://deb.debian.org/debian/pool/main"

# pinned arm64 packages (Debian trixie). Update when a version moves.
FILES=(
  "$POOL/libg/libgpiod/python3-libgpiod_2.2.1-2%2bdeb13u1_arm64.deb"
  "$POOL/s/spidev/python3-spidev_3.6-1%2bb6_arm64.deb"
)

command -v wget >/dev/null 2>&1 || { echo "MISSING TOOL: wget"; exit 1; }
mkdir -p "$DEPS"
cd "$DEPS"

for url in "${FILES[@]}"; do
  echo "fetching $(basename "$url")"
  wget -q "$url"
done

echo "== deps/ contents =="
for f in "$DEPS"/*.deb; do
  dpkg-deb -f "$f" Package Version Architecture 2>/dev/null | tr '\n' ' '; echo
done
echo "done. build-init-sd.sh will seed these onto the init-SD."
