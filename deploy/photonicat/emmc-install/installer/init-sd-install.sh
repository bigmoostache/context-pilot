#!/usr/bin/env bash
# init-sd-install.sh — the zero-touch eMMC flasher that runs ON the init-SD.
#
# The technician plugs the init-SD, powers the Photonicat, and walks away. This
# script (driven by pcat-installer.service on the init-SD's first boot) writes
# the baked golden image onto the eMMC, verifies it, paints progress on the LCD,
# and powers the board off. Power-off IS the done-signal: board dark = safe to
# pull the init-SD.
#
# HARDENING (v2, T656): the v1 assumed /dev/mmcblk0 was the eMMC and fd 1 was
# dd's output — both wrong/dangerous. This version:
#   * DETECTS the eMMC by its non-removable attribute and REFUSES to write the
#     booted (init-SD) device or any removable device — the single most
#     dangerous bug was dd'ing the wrong disk when kernel probe order swaps
#     mmcblk0/mmcblk1 between boots.
#   * tracks REAL flash progress from the of= fd (not stdout), with a startup
#     wait so the watcher never exits before dd opens the device.
#   * space-guards the optional OpenWrt backup so it can't fill the init-SD and
#     wedge the subsequent flash.
#   * treats EVERY external tool as possibly-absent (preflight) and every step
#     as fail-loud (LCD ERROR + board stays on) rather than fail-silent.
#
# Layout on the init-SD (all under /opt/pcat-install, dropped by build-init-sd.sh):
#   golden.img.zst        — zstd-compressed golden eMMC image
#   golden.img.zst.sha256 — sha256 of the DECOMPRESSED image (read-back verify)
#   .img-size             — decompressed byte count
#   lcd/pcat_lcd.py       — the GC9307 progress painter
set -uo pipefail

BASE=/opt/pcat-install
IMG_ZST="$BASE/golden.img.zst"
SHA="$BASE/golden.img.zst.sha256"       # sha256 of the DECOMPRESSED image
LCD="$BASE/lcd/pcat_lcd.py"
STAMP="$BASE/.installed"
LOG=/var/log/pcat-install.log
exec >>"$LOG" 2>&1
echo "=== pcat init-SD install $(date -Is) ==="

# LCD paint is best-effort — a broken painter must NEVER abort a flash.
lcd() { python3 "$LCD" "$@" 2>/dev/null || true; }

fail() {
  echo "FAIL: $*"
  lcd error "$*"
  sync
  # Leave the board ON on failure so the technician sees the error screen and
  # the eMMC is NOT left in an unknown half-state silently powered off.
  exit 1
}

# ── preflight: every external tool the flash depends on must exist ──────────
# A missing zstd/dd/sha256sum mid-flash is the classic silent-failure; catch it
# up front while we can still show a clean error.
for tool in dd zstd sha256sum lsblk findmnt; do
  command -v "$tool" >/dev/null 2>&1 || fail "MISSING TOOL $tool"
done

# ── identify the booted (init-SD) device so we NEVER flash ourselves ────────
# findmnt gives the source partition of /, e.g. /dev/mmcblk1p2; strip the
# partition suffix to get the whole-disk node (mmcblk1). Kernel probe order is
# NOT stable across boots, so we must resolve this dynamically, not assume.
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null)"
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1)"
[ -n "$ROOT_DISK" ] || fail "CANT FIND BOOT DISK"
echo "booted from /dev/$ROOT_DISK ($ROOT_SRC)"

# ── detect the eMMC and NEVER the SD we booted from ─────────────────────────
# PROBED LIVE on asterix (T656): the `removable` flag is USELESS here — BOTH the
# eMMC (mmcblk0) and the SD (mmcblk1) report removable=0. The real, canonical
# discriminators for an eMMC are:
#   1. /sys/block/mmcblkN/device/type == "MMC"  (the SD reports "SD")  ← primary
#   2. a hardware-boot sibling /sys/block/mmcblkNboot0 exists (eMMC-only) ← backup
# We take a candidate only if it is NOT the booted device and matches at least
# one signal, and we REFUSE to guess if more than one qualifies. The mmcblkN
# NUMBER is deliberately never trusted (kernel probe order isn't stable).
detect_emmc() {
  local d name dtype found=""
  for d in /sys/block/mmcblk*; do
    [ -e "$d" ] || continue
    name="$(basename "$d")"
    [ "$name" = "$ROOT_DISK" ] && continue          # never the booted device
    # skip the boot0/boot1/rpmb hardware partitions, keep the main device node
    case "$name" in *boot*|*rpmb*) continue;; esac
    dtype="$(cat "$d/device/type" 2>/dev/null || echo '')"
    # primary: type MMC == eMMC; backup: presence of a boot0 hardware partition
    if [ "$dtype" = "MMC" ] || [ -e "/sys/block/${name}boot0" ]; then
      if [ -n "$found" ]; then
        echo "AMBIGUOUS: both /dev/$found and /dev/$name look like eMMC" >&2
        return 1
      fi
      found="$name"
    fi
  done
  [ -n "$found" ] || return 1
  echo "$found"
}

EMMC_NAME="$(detect_emmc)" || fail "CANT SAFELY IDENTIFY EMMC"
TARGET="/dev/$EMMC_NAME"
[ -b "$TARGET" ] || fail "NO EMMC BLOCK DEV"
# Absolute belt-and-braces: refuse if target somehow equals the boot disk.
[ "$EMMC_NAME" = "$ROOT_DISK" ] && fail "REFUSE FLASH BOOT DISK"
echo "eMMC target = $TARGET"

# ── guard: already flashed this card? ───────────────────────────────────────
if [ -e "$STAMP" ]; then
  echo "stamp present — already flashed, powering off"
  lcd done
  sleep 3
  poweroff
  exit 0
fi

# ── sanity: image + checksum + size present and non-empty ───────────────────
[ -s "$IMG_ZST" ] || fail "GOLDEN IMG MISSING"
[ -s "$SHA" ]     || fail "CHECKSUM MISSING"
[ -s "$BASE/.img-size" ] || fail "IMG-SIZE MISSING"
IMG_BYTES="$(cat "$BASE/.img-size")"
[ "$IMG_BYTES" -gt 0 ] 2>/dev/null || fail "BAD IMG-SIZE"

# ── target must be at least as large as the decompressed image ──────────────
TGT_BYTES="$(blockdev --getsize64 "$TARGET" 2>/dev/null || echo 0)"
if [ "$TGT_BYTES" -gt 0 ] && [ "$TGT_BYTES" -lt "$IMG_BYTES" ]; then
  fail "EMMC SMALLER THAN IMAGE"
fi

# ── OPTIONAL: back up the factory OpenWrt before overwrite (D2) ──────────────
# Space-guarded: a backup that fills the init-SD would wedge the flash below.
# We need free space on $BASE ≥ the eMMC size (worst case, incompressible).
if [ -e "$BASE/.backup-openwrt" ]; then
  FREE_KB="$(df -Pk "$BASE" | awk 'NR==2{print $4}')"
  NEED_KB=$(( TGT_BYTES / 1024 / 2 ))   # assume ≥2:1 zstd on a squashfs image
  if [ "${FREE_KB:-0}" -lt "$NEED_KB" ]; then
    echo "SKIP backup: only ${FREE_KB}KB free, need ~${NEED_KB}KB — flash takes priority"
  else
    echo "backing up factory eMMC to init-SD"
    lcd flashing 0
    if dd if="$TARGET" bs=16M conv=fsync 2>/dev/null | zstd -3 -o "$BASE/openwrt-factory.img.zst"; then
      echo "backup OK"
    else
      echo "backup FAILED — removing partial, continuing"
      rm -f "$BASE/openwrt-factory.img.zst"
    fi
  fi
fi

# ── flash: decompress the golden image straight onto the eMMC ───────────────
echo "flashing golden image to $TARGET ($IMG_BYTES bytes)"
lcd flashing 1

# Start the writer, then the progress watcher. The watcher reads the REAL
# output-file offset from the of= fd (found by resolving /proc/PID/fd symlinks
# to $TARGET), NOT fd 1 (dd's stdout, which the v1 bug read). It waits for the
# fd to appear so it never exits before dd opens the device.
zstd -dc "$IMG_ZST" | dd of="$TARGET" bs=16M conv=fsync iflag=fullblock &
DDPID=$!

(
  # wait up to 20s for dd to open the target fd
  ddfd=""
  for _ in $(seq 1 20); do
    for l in /proc/"$DDPID"/fd/*; do
      [ -e "$l" ] || continue
      if [ "$(readlink -f "$l" 2>/dev/null)" = "$TARGET" ]; then ddfd="$l"; break; fi
    done
    [ -n "$ddfd" ] && break
    sleep 1
  done
  [ -n "$ddfd" ] || exit 0
  fdinfo="/proc/$DDPID/fdinfo/$(basename "$ddfd")"
  while kill -0 "$DDPID" 2>/dev/null; do
    pos="$(awk '/^pos:/{print $2}' "$fdinfo" 2>/dev/null)"
    if [ -n "$pos" ]; then
      pct=$(( pos * 100 / IMG_BYTES ))
      [ "$pct" -gt 99 ] && pct=99
      [ "$pct" -lt 0 ] && pct=0
      lcd flashing "$pct"
    fi
    sleep 4
  done
) &
WATCH=$!

# Wait for the writer and capture its real exit status (not the subshell's).
if ! wait "$DDPID"; then
  kill "$WATCH" 2>/dev/null || true
  fail "DD WRITE FAILED"
fi
kill "$WATCH" 2>/dev/null || true
sync
echo "flash written, syncing"

# ── verify: sha256 the written region, compare to the recorded image hash ────
lcd verify
WANT="$(cut -d' ' -f1 <"$SHA")"
echo "verifying $IMG_BYTES bytes against $WANT"
GOT="$(dd if="$TARGET" bs=1M iflag=fullblock count=$(( (IMG_BYTES + 1048575) / 1048576 )) 2>/dev/null \
        | head -c "$IMG_BYTES" | sha256sum | cut -d' ' -f1)"
if [ "$GOT" != "$WANT" ]; then
  echo "GOT=$GOT WANT=$WANT"
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
