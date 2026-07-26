#!/usr/bin/env bash
# build-init-sd.sh — assemble the "init-SD": a bootable Armbian card that, on
# boot, flashes a pristine Armbian image onto the Photonicat's eMMC, injects the
# operator's root SSH key, and powers off.
#
# WHERE THIS RUNS: a Linux host with the target init-SD inserted (a small card,
# NOT the prod-SD). The card WILL BE ERASED.
#
# STRATEGY (simple test case): the init-SD IS an Armbian system (it boots this
# board and has the tools). It carries a COPY of the same Armbian image as the
# flash payload under /opt/pcat-install, plus the installer + its oneshot unit +
# the operator root pubkey. No golden image, no clone, no offline surgery.
#
# Usage — pass the variables AFTER sudo: sudo defaults to env_reset, so a
# `VAR=… sudo ./script` form silently drops them and the script aborts on the
# ${VAR:?} guards.
#   sudo ARMBIAN_IMG=/path/to/Armbian_photonicat2_trixie.img.xz \
#        CARD=/dev/sdX \
#        PUBKEY=/path/to/operator_root.pub \
#        ./build/build-init-sd.sh
#
#   ARMBIAN_IMG : Armbian image for THIS board — used BOTH to boot the SD AND as
#                 the eMMC flash payload. May be .img.xz, .img.zst, or raw .img.
#   TARGET_IMG  : (optional) a DIFFERENT image to flash onto the eMMC; defaults
#                 to ARMBIAN_IMG.
#   ULA_PREFIX  : (optional) the fleet ULA /48 — see the default below.
#   AUTO_POWEROFF=1 : (optional) power the board off when the flash is done. Off
#                 by default so the LCD keeps showing the box's IPv6 address until
#                 a human presses the power button. Turn it on for unattended
#                 fleet flashing, where nobody is watching the panel.
set -euo pipefail

ARMBIAN_IMG="${ARMBIAN_IMG:?set ARMBIAN_IMG=/path/to/Armbian_photonicat2_*.img.xz}"
TARGET_IMG="${TARGET_IMG:-$ARMBIAN_IMG}"     # image dd'd onto the eMMC in the field
CARD="${CARD:?set CARD=/dev/sdX (the init-SD — WILL BE ERASED)}"
PUBKEY="${PUBKEY:?set PUBKEY=/path/to/operator_root.pub}"

# ── the fleet ULA /48 — A PRODUCT CONSTANT, NOT A PER-BOX VALUE ───────────────
# Every box gets an address in this prefix, with the hardware serial as the
# interface-id, so an installed box is reachable at a KNOWN address with no DHCP,
# no router and no discovery step (RFC 4193; see emmc-install/README.md).
#
# The 40 random bits were drawn ONCE. Do NOT regenerate them per box or per
# build: boxes and the control node must share the prefix to talk to each other,
# and an inventory written against the old prefix would silently stop resolving.
# Keep it in sync with `box_ula_prefix` in deploy/ansible/site.yml.
ULA_PREFIX="${ULA_PREFIX:-fd59:ec78:2da4}"
case "$ULA_PREFIX" in
  fd??:*:*) ;;   # RFC 4193 locally-assigned ULA (fc00::/7 with L=1 ⇒ always fd)
  *) echo "REFUSE: ULA_PREFIX '$ULA_PREFIX' is not an fd..:..:.. /48"; exit 1 ;;
esac
HERE="$(cd "$(dirname "$0")" && pwd)"

for t in dd xz lsblk findmnt blockdev sha256sum partprobe mktemp; do
  command -v "$t" >/dev/null 2>&1 || { echo "MISSING TOOL: $t"; exit 1; }
done
command -v growpart >/dev/null 2>&1 || echo "NOTE: growpart absent — SD rootfs will not be grown; the card must already have room for the payload"
[ -r "$ARMBIAN_IMG" ] || { echo "ARMBIAN_IMG $ARMBIAN_IMG not readable"; exit 1; }
[ -r "$TARGET_IMG" ]  || { echo "TARGET_IMG $TARGET_IMG not readable"; exit 1; }
[ -r "$PUBKEY" ]      || { echo "PUBKEY $PUBKEY not readable"; exit 1; }
[ -b "$CARD" ]        || { echo "CARD $CARD not a block device"; exit 1; }

# ── SAFETY: the card must NOT be the running root, the eMMC, or mounted ──────
CARD_NAME="$(basename "$CARD")"
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null || true)"
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1 || true)"
[ "/dev/$ROOT_DISK" = "$CARD" ] && { echo "REFUSE: $CARD is the running root disk"; exit 1; }
CARD_TYPE="$(cat "/sys/block/$CARD_NAME/device/type" 2>/dev/null || echo '')"
[ "$CARD_TYPE" = "MMC" ] && { echo "REFUSE: $CARD is the internal eMMC (device/type=MMC)"; exit 1; }
if lsblk -nro MOUNTPOINT "$CARD" 2>/dev/null | grep -q .; then
  echo "REFUSE: $CARD has mounted partitions — unmount first:"; lsblk "$CARD"
  # A desktop automounter (GNOME/udisks) re-mounts the card on every udev event,
  # so this fires routinely and the fix is always the same — spell it out, and
  # chain it in front of the run so there is no window for a re-mount.
  echo
  echo "Unmount and re-run in one go:"
  for p in $(lsblk -nro NAME,MOUNTPOINT "$CARD" 2>/dev/null | awk 'NF>1{print $1}'); do
    echo "  udisksctl unmount -b /dev/$p ;\\"
  done
  echo "  sudo ARMBIAN_IMG=$ARMBIAN_IMG CARD=$CARD PUBKEY=$PUBKEY $0"
  exit 1
fi

echo "=== SAFETY CHECK ==="
echo "About to ERASE $CARD and write $ARMBIAN_IMG."
lsblk "$CARD" || true
read -rp "Type the card device path again to confirm: " CONFIRM
[ "$CONFIRM" = "$CARD" ] || { echo "mismatch, aborting"; exit 1; }

# Detect the compression by MAGIC BYTES, not the filename — an image whose
# extension was stripped (e.g. "Trixie_current_minimal" that is really .img.xz)
# would otherwise be dd'd as raw = a card full of compressed bytes that boots
# nothing, and it fails SILENTLY. Content detection removes that trap.
detect_fmt() {   # $1=file → echoes xz|zst|raw
  local magic
  magic="$(od -An -tx1 -N6 "$1" 2>/dev/null | tr -d ' \n')"
  case "$magic" in
    fd377a585a00*) echo xz ;;
    28b52ffd*)     echo zst ;;
    *)             echo raw ;;
  esac
}
decompress_to_dd() {   # $1=src image  → stream decompressed to stdout
  case "$(detect_fmt "$1")" in
    xz)  xz -dc "$1" ;;
    zst) zstd -dc "$1" ;;
    raw) cat "$1" ;;
  esac
}

echo "=== [1] write Armbian image to the init-SD ==="
decompress_to_dd "$ARMBIAN_IMG" | dd of="$CARD" bs=16M conv=fsync status=progress iflag=fullblock
sync; partprobe "$CARD" || true; sleep 2

echo "=== [2] grow the SD rootfs so it can hold the flash payload ==="
# Find the Armbian rootfs partition on the SD by content, then grow it.
CARD_ROOT=""; CARD_ROOT_NO=""
MNT="$(mktemp -d)"
# never leave the card mounted on any exit (a leftover mount also makes the next
# run's "REFUSE mounted" safety fire).
cleanup() { mountpoint -q "$MNT" && umount "$MNT" 2>/dev/null; [ -d "$MNT" ] && rmdir "$MNT" 2>/dev/null || true; }
trap cleanup EXIT
for p in "${CARD}"p* "${CARD}"[0-9]*; do
  [ -b "$p" ] || continue
  if mount "$p" "$MNT" 2>/dev/null; then
    if [ -f "$MNT/etc/os-release" ] || [ -d "$MNT/etc/ssh" ]; then
      CARD_ROOT="$p"; CARD_ROOT_NO="$(echo "$p" | grep -oE '[0-9]+$')"
      umount "$MNT" 2>/dev/null || true; break
    fi
    umount "$MNT" 2>/dev/null || true
  fi
done
[ -n "$CARD_ROOT" ] || { echo "could not find Armbian rootfs on $CARD"; rmdir "$MNT"; exit 1; }
echo "SD rootfs = $CARD_ROOT (partition $CARD_ROOT_NO)"
if command -v growpart >/dev/null 2>&1; then
  sgdisk -e "$CARD" 2>/dev/null || true
  partprobe "$CARD" 2>/dev/null || true; udevadm settle 2>/dev/null || true
  growpart "$CARD" "$CARD_ROOT_NO" || echo "growpart: nothing to do"
  # growpart rewrites the partition table, so the kernel DROPS and recreates the
  # partition node via udev. e2fsck/resize2fs must WAIT for it to reappear — else
  # they run in the gap, hit "No such file or directory", the fs is never grown,
  # and seeding the payload later fails with ENOSPC (the bug we just hit).
  partprobe "$CARD" 2>/dev/null || true; udevadm settle 2>/dev/null || true
  for _ in $(seq 1 20); do [ -b "$CARD_ROOT" ] && break; sleep 1; done
  [ -b "$CARD_ROOT" ] || { echo "FATAL: $CARD_ROOT never reappeared after growpart"; exit 1; }
  e2fsck -fy "$CARD_ROOT" || true
  resize2fs "$CARD_ROOT" || { echo "FATAL: resize2fs failed — SD fs would be too small for the payload"; exit 1; }
else
  echo "FATAL: growpart absent — cannot grow the SD to hold the payload"; exit 1
fi

echo "=== [3] compute decompressed sha256 + size of the flash payload ==="
# The installer verifies the eMMC read-back against the DECOMPRESSED image, so we
# record its sha256 + byte count here (two passes — build-time convenience).
DEC_SIZE="$(decompress_to_dd "$TARGET_IMG" | wc -c)"
DEC_SHA="$(decompress_to_dd "$TARGET_IMG" | sha256sum | cut -d' ' -f1)"
echo "decompressed size=$DEC_SIZE sha256=$DEC_SHA"

echo "=== [4] mount + seed the installer payload ==="
mount "$CARD_ROOT" "$MNT"
install -d "$MNT/opt/pcat-install"
install -Dm755 "$HERE/../installer/init-sd-install.sh" "$MNT/opt/pcat-install/init-sd-install.sh"
install -Dm755 "$HERE/../lcd/pcat_lcd.py"              "$MNT/opt/pcat-install/lcd/pcat_lcd.py"
install -Dm644 "$HERE/../installer/pcat-installer.service" \
  "$MNT/etc/systemd/system/pcat-installer.service"
ln -sf ../pcat-installer.service \
  "$MNT/etc/systemd/system/multi-user.target.wants/pcat-installer.service"

# Free the LCD for our painter: mask any vendor dashboard service that grabs the
# GPIO/SPI lines (it would make the painter fail EBUSY). Harmless if the Armbian
# image has no such service (likely — this was a vendor-image thing).
ln -sf /dev/null "$MNT/etc/systemd/system/pcat2_mini_display.service" 2>/dev/null || true

# the eMMC flash payload: carry the image as .xz (installer decompresses on dd).
# Detect by content, not extension (see detect_fmt) so a mis-named .xz is copied
# as-is rather than double-compressed into an unbootable payload.
case "$(detect_fmt "$TARGET_IMG")" in
  xz)  cp "$TARGET_IMG" "$MNT/opt/pcat-install/armbian.img.xz" ;;
  zst) echo "recompressing .zst payload to .xz"; zstd -dc "$TARGET_IMG" | xz -T0 -3 >"$MNT/opt/pcat-install/armbian.img.xz" ;;
  raw) echo "compressing raw payload to .xz"; xz -T0 -3 -c "$TARGET_IMG" >"$MNT/opt/pcat-install/armbian.img.xz" ;;
esac
printf '%s\n' "$DEC_SHA"  >"$MNT/opt/pcat-install/armbian.sha256"
printf '%s\n' "$DEC_SIZE" >"$MNT/opt/pcat-install/armbian.img-size"
install -m600 "$PUBKEY" "$MNT/opt/pcat-install/root.pub"

# the ULA identity kit: the /48 (as a build input, not a literal buried in the
# installer) + the assigner the installer copies onto the eMMC rootfs. The
# per-box interface-id is NOT here — the installer derives it from the board's own
# serial at flash time, so one init-SD provisions the whole fleet.
printf '%s\n' "$ULA_PREFIX" >"$MNT/opt/pcat-install/ula-prefix"
install -Dm755 "$HERE/../ula/pcat-ula.sh"      "$MNT/opt/pcat-install/ula/pcat-ula.sh"
install -Dm644 "$HERE/../ula/pcat-ula.service" "$MNT/opt/pcat-install/ula/pcat-ula.service"
install -Dm755 "$HERE/../ula/nm-dispatcher-50-pcat-ula" \
  "$MNT/opt/pcat-install/ula/nm-dispatcher-50-pcat-ula"
echo "  ULA prefix: $ULA_PREFIX (interface-id derived per box from its serial)"

# Completion behaviour. Absent file = manual power-off (the default): the board
# stays on with the address on the LCD. Only written when explicitly asked for.
if [ "${AUTO_POWEROFF:-0}" = "1" ]; then
  printf '1\n' >"$MNT/opt/pcat-install/auto-poweroff"
  echo "  completion: AUTO-POWEROFF (unattended; board goes dark when done)"
else
  rm -f "$MNT/opt/pcat-install/auto-poweroff"
  echo "  completion: MANUAL power-off (board stays on showing the address)"
fi

# The init-SD is a transient installer environment: don't make its own Armbian
# first-login gate wait on anything (the oneshot runs regardless, but this keeps
# the boot clean and fast).
rm -f "$MNT/root/.not_logged_in_yet" 2>/dev/null || true

echo "=== [5] seed the LCD deps as offline .deb (dpkg-installed on the pcat) ==="
# The LCD painter needs python3-spidev + python3-libgpiod, which the Armbian
# minimal image lacks. We do NOT chroot-apt them (cross-arch DNS under qemu fails
# with getaddrinfo EBUSY): instead we carry the arm64 .deb on the SD and the
# installer dpkg-installs them offline on the pcat before painting. These are the
# ONLY missing deps — xz and libgpiod2 are already in the base image (apt showed
# "2 newly installed"). Best-effort: no debs / a failed install = no LCD, and the
# flash still works (power-off stays the done-signal).
# Refresh deps/ from the Debian pool with build/fetch-lcd-deps.sh when versions move.
if ls "$HERE/../deps/"*.deb >/dev/null 2>&1; then
  install -d "$MNT/opt/pcat-install/debs"
  cp "$HERE/../deps/"*.deb "$MNT/opt/pcat-install/debs/"
  echo "  seeded LCD debs:"; (cd "$HERE/../deps" && for f in *.deb; do echo "    $f"; done)
else
  echo "  NOTE: no .deb in deps/ — LCD progress will be skipped (flash still works)."
fi
# xz is REQUIRED by the installer and already ships in the Armbian base — verify.
[ -x "$MNT/usr/bin/xz" ] && echo "  xz: present in image" \
  || echo "  WARNING: xz missing in image — the installer will FAIL on the pcat."

sync
umount "$MNT" 2>/dev/null || true
rmdir "$MNT" 2>/dev/null || true
echo "=== init-SD ready on $CARD ==="
echo "Plug into the Photonicat and power on. When the LCD shows DONE, note the"
echo "address, PRESS THE POWER BUTTON, wait for the board to go dark, THEN pull"
echo "the card (it is the running rootfs while the board is on)."
echo "The DONE screen shows the box's IPv6 interface-id (= its serial); its"
echo "address is ${ULA_PREFIX}:1:<that id>. Connect with:"
echo "  sudo ip -6 addr add ${ULA_PREFIX}:1::1/64 dev <your-iface>   # once per control node"
echo "  ssh -i <your key> root@${ULA_PREFIX}:1:<interface-id>        # then Ansible"
