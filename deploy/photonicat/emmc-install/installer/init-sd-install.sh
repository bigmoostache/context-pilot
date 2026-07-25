#!/usr/bin/env bash
# init-sd-install.sh — zero-touch eMMC flasher that runs ON the init-SD.
#
# SIMPLE TEST CASE (the minimal slice we validate first):
#   1. detect the eMMC (never the booted SD),
#   2. dd a pristine Armbian image onto it,
#   3. verify the written region against the recorded sha256,
#   4. inject ONLY the operator's root SSH key (so Ansible can connect),
#   5. power off.
# Everything else (custom user, hostname, tailscale, Context Pilot) is Ansible's
# job on a later boot. No golden image, no offline block-surgery, no LCD.
#
# Power-off IS the done-signal: board dark = safe to pull the init-SD, then boot
# the eMMC and let Ansible take over.
#
# Payload on the init-SD (under /opt/pcat-install, dropped by build-init-sd.sh):
#   armbian.img.xz     — xz-compressed pristine Armbian image for THIS board
#   armbian.sha256     — sha256 of the DECOMPRESSED image (read-back verify)
#   armbian.img-size   — decompressed byte count
#   root.pub           — operator root public key (the ONLY thing we inject)
#   lcd/pcat_lcd.py    — GC9307 front-panel progress painter (best-effort)
#   debs/*.deb         — python3-spidev + python3-libgpiod (offline LCD deps)
set -uo pipefail

BASE=/opt/pcat-install
IMG_XZ="$BASE/armbian.img.xz"
SHA="$BASE/armbian.sha256"          # sha256 of the DECOMPRESSED image
SIZEF="$BASE/armbian.img-size"      # decompressed byte count
PUBKEY="$BASE/root.pub"
LCD="$BASE/lcd/pcat_lcd.py"         # GC9307 progress painter
STAMP=/run/pcat-install.done        # tmpfs — only guards a within-boot double-run
LOG=/var/log/pcat-install.log
exec >>"$LOG" 2>&1
echo "=== pcat init-SD install $(date -Is) ==="

# LCD paint is BEST-EFFORT: a missing/broken painter (no spidev/gpiod on the
# image, wrong GC9307 mapping on Armbian, EBUSY, …) must NEVER abort the flash.
# Power-off stays the authoritative done-signal even with a dark panel.
lcd() { [ -f "$LCD" ] && python3 "$LCD" "$@" >/dev/null 2>&1 || true; }

fail() {
  echo "FAIL: $*"
  lcd error "$*"
  sync
  # Leave the board ON on failure so the eMMC is not left in an unknown
  # half-state silently powered off, and the log survives for inspection.
  exit 1
}

# ── preflight: every tool the flash + inject depends on ─────────────────────
for tool in dd xz sha256sum lsblk findmnt blockdev mount umount; do
  command -v "$tool" >/dev/null 2>&1 || fail "MISSING TOOL $tool"
done
command -v partprobe >/dev/null 2>&1 || command -v partx >/dev/null 2>&1 \
  || fail "MISSING TOOL partprobe/partx"

# ── best-effort: install the LCD deps carried as offline .deb ────────────────
# The Armbian minimal image lacks python3-spidev/python3-libgpiod; build-init-sd
# seeds them under debs/. dpkg-install here so the painter works. Absent or failed
# = no LCD, never fatal (power-off stays the done-signal), so this is not gated by
# fail(); it runs before the first lcd() call so progress paints from the start.
if ls "$BASE/debs/"*.deb >/dev/null 2>&1; then
  dpkg -i "$BASE/debs/"*.deb >/dev/null 2>&1 && echo "LCD deps installed" || echo "LCD deps install failed (progress screen skipped)"
fi

# ── identify the booted (init-SD) device so we NEVER flash ourselves ────────
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null)"
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1)"
[ -n "$ROOT_DISK" ] || fail "CANT FIND BOOT DISK"
echo "booted from /dev/$ROOT_DISK ($ROOT_SRC)"

# ── detect the eMMC and NEVER the SD we booted from ─────────────────────────
# The `removable` flag is USELESS on this board — both the eMMC and a native-slot
# SD report removable=0. Canonical eMMC discriminators:
#   1. /sys/block/mmcblkN/device/type == "MMC"  (an SD reports "SD")  ← primary
#   2. a hardware-boot sibling /sys/block/mmcblkNboot0 exists (eMMC-only) ← backup
# Refuse to guess if more than one candidate qualifies; never trust the number.
detect_emmc() {
  local d name dtype found=""
  for d in /sys/block/mmcblk*; do
    [ -e "$d" ] || continue
    name="$(basename "$d")"
    [ "$name" = "$ROOT_DISK" ] && continue
    case "$name" in *boot*|*rpmb*) continue;; esac
    dtype="$(cat "$d/device/type" 2>/dev/null || echo '')"
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
[ "$EMMC_NAME" = "$ROOT_DISK" ] && fail "REFUSE FLASH BOOT DISK"
echo "eMMC target = $TARGET"

# ── within-boot double-run guard (stamp in /run, gone next power-on) ─────────
if [ -e "$STAMP" ]; then
  echo "stamp present this boot — already flashed, powering off"
  lcd done
  sync; sleep 2; poweroff; exit 0
fi

# ── sanity: payload present ─────────────────────────────────────────────────
[ -s "$IMG_XZ" ]  || fail "ARMBIAN IMG MISSING"
[ -s "$SHA" ]     || fail "CHECKSUM MISSING"
[ -s "$SIZEF" ]   || fail "IMG-SIZE MISSING"
[ -s "$PUBKEY" ]  || fail "ROOT PUBKEY MISSING"
IMG_BYTES="$(cat "$SIZEF")"
[ "$IMG_BYTES" -gt 0 ] 2>/dev/null || fail "BAD IMG-SIZE"

# ── target must be at least as large as the decompressed image ──────────────
TGT_BYTES="$(blockdev --getsize64 "$TARGET" 2>/dev/null || echo 0)"
if [ "$TGT_BYTES" -gt 0 ] && [ "$TGT_BYTES" -lt "$IMG_BYTES" ]; then
  fail "EMMC SMALLER THAN IMAGE ($TGT_BYTES < $IMG_BYTES)"
fi

# ── flash: decompress the Armbian image straight onto the eMMC ──────────────
echo "flashing Armbian image to $TARGET ($IMG_BYTES bytes decompressed)"
lcd flashing 0

# Background writer, then a progress watcher. The watcher reads the REAL output
# offset from dd's of= fd (resolved via /proc/PID/fd → $TARGET), NOT stdout, so
# the bar tracks bytes actually written. $! after a pipeline is the last stage
# (dd) in bash — exactly the PID whose fdinfo we want.
xz -dc "$IMG_XZ" | dd of="$TARGET" bs=16M conv=fsync iflag=fullblock &
DDPID=$!

(
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
[ "$GOT" = "$WANT" ] || fail "VERIFY MISMATCH (got=$GOT want=$WANT)"
echo "verify OK"

# ── re-read the freshly-written partition table ─────────────────────────────
partprobe "$TARGET" 2>/dev/null || partx -u "$TARGET" 2>/dev/null || true
sleep 2

# ── inject ONLY the root SSH key (the bootstrap Ansible needs) ───────────────
# Find the Armbian rootfs partition on the eMMC by content (an ext4 partition
# carrying /etc/ssh) rather than assuming a partition number. Mount rw, drop the
# key, allow key-only root login, and DISABLE Armbian's interactive first-login
# gate so headless root SSH works on the first boot (see note below).
echo "injecting root SSH key onto the eMMC rootfs"
MNT="$(mktemp -d)"
ROOTFS=""
for p in "${TARGET}p"* "${TARGET}"[0-9]*; do
  [ -b "$p" ] || continue
  if mount -o rw "$p" "$MNT" 2>/dev/null; then
    if [ -f "$MNT/etc/ssh/sshd_config" ] || [ -f "$MNT/etc/os-release" ]; then
      ROOTFS="$p"; break
    fi
    umount "$MNT" 2>/dev/null || true
  fi
done
[ -n "$ROOTFS" ] || { rmdir "$MNT" 2>/dev/null || true; fail "COULD NOT FIND ARMBIAN ROOTFS ON $TARGET"; }
echo "rootfs partition = $ROOTFS"

install -d -m700 "$MNT/root/.ssh"
install -m600 "$PUBKEY" "$MNT/root/.ssh/authorized_keys"
# uid/gid 0 (we run as root, so created files are already root-owned)

# allow key-only root login
if [ -f "$MNT/etc/ssh/sshd_config" ]; then
  sed -i 's/^\s*#\?\s*PermitRootLogin.*/PermitRootLogin prohibit-password/' "$MNT/etc/ssh/sshd_config" || true
  grep -qE '^\s*PermitRootLogin' "$MNT/etc/ssh/sshd_config" \
    || echo 'PermitRootLogin prohibit-password' >>"$MNT/etc/ssh/sshd_config"
fi

# ARMBIAN GOTCHA: Armbian gates the FIRST login behind an interactive account-
# setup (`/root/.not_logged_in_yet` → armbian-firstlogin), which can block
# headless root SSH until a human sets a password + creates a user. We remove it
# so key-based root SSH works on the first boot. This does NOT disable
# armbian-firstrun (rootfs resize + SSH host-key regen) — that is a separate
# unit and still runs. VERIFY this behavior on the actual Armbian image.
rm -f "$MNT/root/.not_logged_in_yet" 2>/dev/null || true

sync
umount "$MNT" 2>/dev/null || true
rmdir "$MNT" 2>/dev/null || true
echo "root key injected"

# ── done: stamp, paint DONE, power off (= safe-to-pull signal) ──────────────
date -Is >"$STAMP"
sync
lcd done
echo "=== install complete, powering off ==="
sleep 3
poweroff
