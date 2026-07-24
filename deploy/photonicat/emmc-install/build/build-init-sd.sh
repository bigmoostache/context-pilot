#!/usr/bin/env bash
# build-init-sd.sh — assemble the "init-SD": a bootable Debian card that, on
# first boot, auto-flashes the golden image onto the Photonicat's eMMC, then
# powers off.
#
# WHERE THIS RUNS: a Linux host with the target init-SD inserted (a small card,
# NOT the 500 GB prod-SD). NOT the Mac Mini. Natural host: asterix.
#
# STRATEGY: the init-SD IS a Debian system; the simplest proven one for this
# board is asterix's own image, reused with a DIFFERENT payload — the installer
# unit + golden image + LCD painter under /opt/pcat-install. It must NOT carry
# the firstboot-emmc unit (that runs on the eMMC target).
#
# HARDENING (v2, T656):
#   * REFUSES to write a non-removable / mounted / obviously-too-small target
#     (the v1 could nuke the wrong disk if the operator fat-fingered $CARD).
#   * space-checks the card holds the golden payload before it starts.
#   * INSTALLS the runtime deps the flasher + painter need (zstd, sgdisk,
#     cloud-guest-utils, python3-spidev, python3-libgpiod) into the image via
#     chroot, so a missing tool can't silently disable the flash or the LCD in
#     the field — the single biggest "works-on-my-image" gap.
set -euo pipefail

BASE_IMG="${BASE_IMG:?set BASE_IMG=/path/to/base-debian-pcat.img (a card image that boots this board)}"
CARD="${CARD:?set CARD=/dev/sdX or /dev/mmcblkN (the init-SD — WILL BE ERASED)}"
GOLDEN_DIR="${GOLDEN_DIR:?set GOLDEN_DIR=dir with golden.img.zst + .sha256 + .img-size}"
BACKUP_OPENWRT="${BACKUP_OPENWRT:-1}"
ROOT_PARTNO="${ROOT_PARTNO:-2}"
HERE="$(cd "$(dirname "$0")" && pwd)"

for t in dd lsblk findmnt partprobe growpart e2fsck resize2fs; do
  command -v "$t" >/dev/null 2>&1 || { echo "MISSING TOOL: $t"; exit 1; }
done

# ── SAFETY: the card must NOT be the running root, NOT mounted, NOT the eMMC ──
# HARDWARE NOTE (verified live on asterix, T656): the `removable` flag is NOT a
# reliable "is this a safe SD" signal on RK-class boards — an SD in the native
# mmc slot reports removable=0, identical to the internal eMMC. So we do NOT
# hard-gate on `removable` (that false-refuses a legitimate native-slot init-SD);
# instead we use the same discriminator the on-device installer uses:
#   * REFUSE the running-root disk (never wipe the OS we booted from);
#   * REFUSE the eMMC — /sys/.../device/type == "MMC" (an SD reports "SD");
#   * REFUSE any mounted disk (torn-write / wrong-target risk).
# A USB card reader still reports removable=1 (the normal PC case); a native
# mmc-slot SD reports removable=0 but type "SD" — allowed, with a warning so the
# operator notices. ALLOW_NONREMOVABLE=1 remains an override for exotic readers.
CARD_NAME="$(basename "$CARD")"
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null || true)"
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1 || true)"
[ "/dev/$ROOT_DISK" = "$CARD" ] && { echo "REFUSE: $CARD is the running root disk"; exit 1; }
CARD_TYPE="$(cat "/sys/block/$CARD_NAME/device/type" 2>/dev/null || echo '')"
[ "$CARD_TYPE" = "MMC" ] && { echo "REFUSE: $CARD is the internal eMMC (device/type=MMC) — never flash the eMMC as an init-SD"; exit 1; }
REM="$(cat "/sys/block/$CARD_NAME/removable" 2>/dev/null || echo 0)"
if [ "$REM" != "1" ] && [ "$CARD_TYPE" != "SD" ]; then
  echo "REFUSE: $CARD is not removable (removable=$REM) and not an SD card (device/type='$CARD_TYPE')."
  echo "This looks like an internal disk. Override with ALLOW_NONREMOVABLE=1 only if certain."
  [ "${ALLOW_NONREMOVABLE:-0}" = "1" ] || exit 1
fi
[ "$REM" != "1" ] && echo "NOTE: $CARD reports removable=0 but device/type='$CARD_TYPE' (native-slot SD) — proceeding."
if lsblk -nro MOUNTPOINT "$CARD" 2>/dev/null | grep -q .; then
  echo "REFUSE: $CARD has mounted partitions — unmount first:"; lsblk "$CARD"; exit 1
fi
[ -s "$GOLDEN_DIR/golden.img.zst" ] || { echo "no golden.img.zst in $GOLDEN_DIR"; exit 1; }

# ── SAFETY: card must be big enough for the golden payload + base ────────────
CARD_BYTES="$(blockdev --getsize64 "$CARD" 2>/dev/null || echo 0)"
GOLD_BYTES="$(stat -c%s "$GOLDEN_DIR/golden.img.zst" 2>/dev/null || echo 0)"
if [ "$CARD_BYTES" -gt 0 ] && [ "$CARD_BYTES" -lt $(( GOLD_BYTES + 4*1024*1024*1024 )) ]; then
  echo "REFUSE: card ($CARD_BYTES B) too small for golden ($GOLD_BYTES B) + base + slack"; exit 1
fi

echo "=== SAFETY CHECK ==="
echo "About to ERASE $CARD (removable=$REM, $CARD_BYTES bytes) and write $BASE_IMG."
lsblk "$CARD" || true
read -rp "Type the card device path again to confirm: " CONFIRM
[ "$CONFIRM" = "$CARD" ] || { echo "mismatch, aborting"; exit 1; }

echo "=== [1] write base Debian image to the init-SD ==="
case "$BASE_IMG" in
  *.zst) zstd -dc "$BASE_IMG" | dd of="$CARD" bs=16M conv=fsync status=progress ;;
  *.xz)  xzcat "$BASE_IMG"    | dd of="$CARD" bs=16M conv=fsync status=progress ;;
  *)     dd if="$BASE_IMG" of="$CARD" bs=16M conv=fsync status=progress ;;
esac
sync; partprobe "$CARD" || true

echo "=== [2] grow the card rootfs to use the whole init-SD ==="
sgdisk -e "$CARD" 2>/dev/null || true
partprobe "$CARD" || true
growpart "$CARD" "$ROOT_PARTNO" || true
partprobe "$CARD" || true
CARDP="$(lsblk -nro NAME "$CARD" | sed -n "$((ROOT_PARTNO+1))p")"
CARDP="/dev/$CARDP"
e2fsck -fy "$CARDP" || true
resize2fs "$CARDP" || true

echo "=== [3] mount + seed the installer payload ==="
MNT="$(mktemp -d)"
mount "$CARDP" "$MNT"
install -d "$MNT/opt/pcat-install/lcd"
install -Dm755 "$HERE/../installer/init-sd-install.sh" "$MNT/opt/pcat-install/init-sd-install.sh"
install -Dm755 "$HERE/../lcd/pcat_lcd.py"              "$MNT/opt/pcat-install/lcd/pcat_lcd.py"
install -Dm644 "$HERE/../installer/pcat-installer.service" \
  "$MNT/etc/systemd/system/pcat-installer.service"
ln -sf ../pcat-installer.service \
  "$MNT/etc/systemd/system/multi-user.target.wants/pcat-installer.service"
cp "$GOLDEN_DIR/golden.img.zst"        "$MNT/opt/pcat-install/golden.img.zst"
cp "$GOLDEN_DIR/golden.img.zst.sha256" "$MNT/opt/pcat-install/golden.img.zst.sha256"
cp "$GOLDEN_DIR/.img-size"             "$MNT/opt/pcat-install/.img-size"
[ "$BACKUP_OPENWRT" = "1" ] && touch "$MNT/opt/pcat-install/.backup-openwrt"

# the init-SD must NOT run the eMMC firstboot unit (target-only)
rm -f "$MNT/etc/systemd/system/multi-user.target.wants/pcat-firstboot.service" || true
rm -f "$MNT/etc/systemd/system/pcat-firstboot.service" || true

# MASK the vendor LCD dashboard on the init-SD. It exports gpio122/121 via
# LEGACY sysfs at boot, which makes our installer's painter fail with EBUSY
# ("Device or resource busy") when it tries to claim the same lines via
# libgpiod (VERIFIED live on asterix, T656). Masking frees the LCD for the
# installer. The eMMC target clone keeps the vendor dashboard — only the
# transient init-SD environment gives it up.
ln -sf /dev/null "$MNT/etc/systemd/system/pcat2_mini_display.service" || true

echo "=== [4] install runtime deps into the image (chroot) ==="
# The flasher needs zstd + sgdisk + growpart; the painter needs python3 + spidev
# + libgpiod. Missing any = silent field failure, so bake them in now. Best-
# effort: needs qemu-user-static for cross-arch chroot if the host isn't arm64.
if command -v chroot >/dev/null 2>&1; then
  for d in dev proc sys; do mount --bind "/$d" "$MNT/$d" 2>/dev/null || true; done
  cp /etc/resolv.conf "$MNT/etc/resolv.conf" 2>/dev/null || true
  chroot "$MNT" /bin/bash -c '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -y &&
    apt-get install -y --no-install-recommends \
      zstd gdisk cloud-guest-utils e2fsprogs \
      python3 python3-spidev python3-libgpiod curl ca-certificates
  ' || echo "WARNING: dep install in chroot failed — verify tools exist in BASE_IMG"
  for d in dev proc sys; do umount "$MNT/$d" 2>/dev/null || true; done
else
  echo "WARNING: no chroot — ensure BASE_IMG already has zstd gdisk cloud-guest-utils python3-spidev python3-libgpiod"
fi

sync
umount "$MNT"; rmdir "$MNT"
echo "=== init-SD ready on $CARD ==="
echo "Plug into the Photonicat, power on, wait for POWER-OFF, pull the card."
