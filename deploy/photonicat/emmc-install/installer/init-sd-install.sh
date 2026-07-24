#!/usr/bin/env bash
# init-sd-install.sh — the zero-touch eMMC flasher that runs ON the init-SD.
#
# The technician plugs the init-SD, powers the Photonicat, and walks away. This
# script (driven by pcat-installer.service on the init-SD's first boot) writes
# the baked golden image onto the eMMC, verifies it, paints progress on the LCD,
# and powers the board off. Power-off IS the done-signal: board dark = safe to
# pull the init-SD. The technician then inserts the prod-SD and powers on, and
# the box comes up on eMMC Debian (firstboot-emmc.sh personalises it there).
#
# Layout on the init-SD (all under /opt/pcat-install, dropped by build-init-sd.sh):
#   golden.img.zst   — zstd-compressed golden eMMC image (~4-6 GB)
#   golden.img.zst.sha256 — checksum of the DECOMPRESSED image for read-back verify
#   lcd/pcat_lcd.py  — the GC9307 progress painter
#
# Safety: a stamp file on the init-SD (/opt/pcat-install/.installed) guards
# re-entry, so leaving the init-SD in across the post-install reboot never
# re-flashes. And the SD is boot-priority, so the ONLY thing that stops a re-flash
# loop is either pulling the SD (intended) or this stamp (belt + braces).
set -uo pipefail

BASE=/opt/pcat-install
IMG_ZST="$BASE/golden.img.zst"
SHA="$BASE/golden.img.zst.sha256"       # sha256 of the DECOMPRESSED image
LCD="$BASE/lcd/pcat_lcd.py"
STAMP="$BASE/.installed"
TARGET=/dev/mmcblk0                       # the eMMC
LOG=/var/log/pcat-install.log
exec >>"$LOG" 2>&1
echo "=== pcat init-SD install $(date -Is) ==="

lcd() { python3 "$LCD" "$@" 2>/dev/null || true; }
fail() {
  echo "FAIL: $*"
  lcd error "$*"
  sleep 10
  # leave the board ON on failure so the technician sees the error screen.
  exit 1
}

# ── guard: already flashed this card? ───────────────────────────────────────
if [ -e "$STAMP" ]; then
  echo "stamp present — already flashed, powering off"
  lcd done
  sleep 3
  poweroff
  exit 0
fi

# ── sanity: image + checksum + target present ───────────────────────────────
[ -s "$IMG_ZST" ] || fail "GOLDEN IMG MISSING"
[ -s "$SHA" ]     || fail "CHECKSUM MISSING"
[ -b "$TARGET" ]  || fail "NO EMMC"

# ── OPTIONAL: back up the factory OpenWrt before overwrite (D2) ──────────────
# Controlled by a flag file so build-init-sd.sh can size the card accordingly.
if [ -e "$BASE/.backup-openwrt" ]; then
  echo "backing up factory eMMC to init-SD"
  lcd flashing 0
  dd if="$TARGET" bs=16M conv=fsync 2>/dev/null | zstd -3 -o "$BASE/openwrt-factory.img.zst" \
    && echo "backup OK" || echo "backup FAILED (continuing anyway)"
fi

# ── flash: decompress the golden image straight onto the eMMC ───────────────
# Progress is estimated from the decompressed image size (recorded at build time
# in .img-size) vs bytes written, sampled by a background watcher that repaints
# the LCD every few seconds. dd conv=fsync forces the data to physical media.
echo "flashing golden image to $TARGET"
lcd flashing 1

IMG_BYTES=0
[ -e "$BASE/.img-size" ] && IMG_BYTES="$(cat "$BASE/.img-size")"

# background progress painter: poll dd's output offset via /proc, repaint LCD.
(
  while :; do
    sleep 4
    # find the dd writing to the eMMC and read its file position
    ddpid="$(pgrep -f "dd of=$TARGET" | head -1)"
    [ -z "$ddpid" ] && break
    pos="$(awk '/^pos:/{print $2}' "/proc/$ddpid/fdinfo/1" 2>/dev/null)"
    if [ -n "$pos" ] && [ "$IMG_BYTES" -gt 0 ]; then
      pct=$(( pos * 100 / IMG_BYTES ))
      [ "$pct" -gt 99 ] && pct=99
      lcd flashing "$pct"
    fi
  done
) &
WATCH=$!

zstd -dc "$IMG_ZST" | dd of="$TARGET" bs=16M conv=fsync iflag=fullblock \
  || fail "DD WRITE FAILED"
kill "$WATCH" 2>/dev/null || true
sync
echo "flash written, syncing"

# ── verify: sha256 the written region, compare to the recorded image hash ────
lcd verify
WANT="$(cut -d' ' -f1 <"$SHA")"
echo "verifying $IMG_BYTES bytes against $WANT"
GOT="$(dd if="$TARGET" bs=16M count=$(( (IMG_BYTES + 16*1024*1024 - 1) / (16*1024*1024) )) 2>/dev/null \
        | head -c "$IMG_BYTES" | sha256sum | cut -d' ' -f1)"
if [ "$GOT" != "$WANT" ]; then
  fail "VERIFY MISMATCH"
fi
echo "verify OK"

# ── done: stamp, paint DONE, power off (= safe-to-pull signal) ──────────────
date -Is >"$STAMP"
sync
lcd done
echo "=== install complete, powering off ==="
sleep 4
poweroff
