#!/usr/bin/env bash
# build-init-sd.sh — assemble the "init-SD": a bootable Debian card that, on first
# boot, auto-flashes the golden image onto the Photonicat's eMMC and powers off.
#
# WHERE THIS RUNS: a Linux host with the target init-SD inserted (a small ~16 GB
# card, NOT the 500 GB prod-SD). NOT the Mac Mini. Natural host: asterix.
#
# STRATEGY: the init-SD IS a Debian system — the simplest, most-proven Debian
# that boots this exact board is asterix's own image. So the init-SD is built the
# same way as the golden image (a clone that boots), but with a DIFFERENT payload:
# instead of the firstboot-personalise unit, it carries the installer unit + the
# golden image + the LCD painter under /opt/pcat-install. It must NOT carry the
# firstboot-emmc unit (that runs on the eMMC target, not the installer card).
#
# You can either:
#   (A) write a fresh Debian image to the card and seed it (below), or
#   (B) reuse a spare golden.img as the base and swap the payload.
# This script implements (A)-lite: it expects a base Debian image path and a
# target card device, writes the base, then seeds the installer payload.
set -euo pipefail

# ── inputs ──────────────────────────────────────────────────────────────────
BASE_IMG="${BASE_IMG:?set BASE_IMG=/path/to/base-debian-pcat.img (a card image that boots this board)}"
CARD="${CARD:?set CARD=/dev/sdX (the init-SD device — WILL BE ERASED)}"
GOLDEN_DIR="${GOLDEN_DIR:?set GOLDEN_DIR=dir containing golden.img.zst + .sha256 + .img-size}"
BACKUP_OPENWRT="${BACKUP_OPENWRT:-1}"       # 1 = back up factory eMMC before wipe (D2)
HERE="$(cd "$(dirname "$0")" && pwd)"

echo "=== SAFETY CHECK ==="
echo "About to ERASE $CARD and write $BASE_IMG onto it."
lsblk "$CARD" || true
read -rp "Type the card device again to confirm: " CONFIRM
[ "$CONFIRM" = "$CARD" ] || { echo "mismatch, aborting"; exit 1; }

echo "=== [1] write base Debian image to the init-SD ==="
if [[ "$BASE_IMG" == *.zst ]]; then
  zstd -dc "$BASE_IMG" | dd of="$CARD" bs=16M conv=fsync status=progress
elif [[ "$BASE_IMG" == *.xz ]]; then
  xzcat "$BASE_IMG" | dd of="$CARD" bs=16M conv=fsync status=progress
else
  dd if="$BASE_IMG" of="$CARD" bs=16M conv=fsync status=progress
fi
sync; partprobe "$CARD" || true

echo "=== [2] grow the card rootfs to use the whole init-SD ==="
# (so there's room for the golden image payload). p2 assumed rootfs.
sgdisk -e "$CARD" || true
growpart "$CARD" 2 || true
e2fsck -fy "${CARD}p2" || true
resize2fs "${CARD}p2" || true

echo "=== [3] mount the card rootfs + seed the installer payload ==="
MNT="$(mktemp -d)"
mount "${CARD}p2" "$MNT"

install -d "$MNT/opt/pcat-install/lcd"
install -Dm755 "$HERE/../installer/init-sd-install.sh" "$MNT/opt/pcat-install/init-sd-install.sh"
install -Dm755 "$HERE/../lcd/pcat_lcd.py"              "$MNT/opt/pcat-install/lcd/pcat_lcd.py"
install -Dm644 "$HERE/../installer/pcat-installer.service" \
  "$MNT/etc/systemd/system/pcat-installer.service"
ln -sf ../pcat-installer.service \
  "$MNT/etc/systemd/system/multi-user.target.wants/pcat-installer.service"

# the golden image payload
cp "$GOLDEN_DIR/golden.img.zst"        "$MNT/opt/pcat-install/golden.img.zst"
cp "$GOLDEN_DIR/golden.img.zst.sha256" "$MNT/opt/pcat-install/golden.img.zst.sha256"
cp "$GOLDEN_DIR/.img-size"             "$MNT/opt/pcat-install/.img-size"
[ "$BACKUP_OPENWRT" = "1" ] && touch "$MNT/opt/pcat-install/.backup-openwrt"

# CRITICAL: the init-SD must NOT run the eMMC firstboot-personalise unit — that
# belongs to the TARGET, not the installer card. Remove it if the base image
# happened to carry it.
rm -f "$MNT/etc/systemd/system/multi-user.target.wants/pcat-firstboot.service" || true
rm -f "$MNT/etc/systemd/system/pcat-firstboot.service" || true

# make sure python3 + spidev are present for the LCD painter
if [ ! -e "$MNT/usr/bin/python3" ]; then
  echo "WARNING: base image has no python3 — LCD progress will be skipped."
  echo "         (install python3 python3-spidev python3-libgpiod into the base first)"
fi

sync
umount "$MNT"; rmdir "$MNT"

echo "=== init-SD ready on $CARD ==="
echo "Plug it into the Photonicat, power on, wait for POWER-OFF, pull the card."
