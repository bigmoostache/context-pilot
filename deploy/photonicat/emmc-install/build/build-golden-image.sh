#!/usr/bin/env bash
# build-golden-image.sh — clone asterix's live Debian SD into a shrunk, seeded
# "golden" eMMC image ready to dd onto any Photonicat 2's eMMC.
#
# WHERE THIS RUNS: on a Linux host that can see BOTH the source SD (asterix's
# card, or asterix itself imaging its own disk) AND enough scratch space (~40 GB).
# NOT the Mac Mini (no card reader here, and macOS lacks losetup/resize2fs). The
# natural host is asterix itself: it can `dd` its own /dev/mmcblk1, shrink a copy,
# and seed it — everything below is Linux-only (losetup, e2fsck, resize2fs,
# sgdisk, zstd).
#
# WHAT IT PRODUCES (in $OUT):
#   golden.img            — shrunk (~20 GB) full-disk image: GPT + p1 boot + p2 root
#   golden.img.zst        — zstd-compressed (~4-6 GB) for the init-SD
#   golden.img.zst.sha256 — sha256 of the DECOMPRESSED golden.img (read-back verify)
#   .img-size             — decompressed byte count (installer progress + verify)
#
# WHAT IT SEEDS INTO THE IMAGE before shrinking:
#   * /usr/local/sbin/firstboot-emmc.sh + pcat-firstboot.service (enabled)
#   * ~/.ssh/authorized_keys           — the operator's public key (key-only login)
#   * /boot/pcat-ts-authkey            — the reusable tagged tailscale key (secret,
#                                        shredded by firstboot after enrol)
#
# This is a PLAN script: it is written to be run by hand once the source card is
# reachable. It is deliberately verbose + guarded; read the ENV block and set the
# three inputs before running.
set -euo pipefail

# ── inputs (override via env) ───────────────────────────────────────────────
SRC_DISK="${SRC_DISK:-/dev/mmcblk1}"        # asterix's Debian SD (whole disk)
OUT="${OUT:-/mnt/scratch/pcat-golden}"      # scratch dir with ~40 GB free
PUBKEY="${PUBKEY:?set PUBKEY=/path/to/authorized_key.pub}"   # operator ssh pubkey
TS_AUTHKEY="${TS_AUTHKEY:-}"                 # reusable tagged tailscale key (optional)
SHRINK_SLACK_MB="${SHRINK_SLACK_MB:-1500}"   # free MB to leave above used data
HERE="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$OUT"
RAW="$OUT/golden-raw.img"
IMG="$OUT/golden.img"

echo "=== [1] full-disk image of $SRC_DISK ==="
# Image the entire disk so the GPT + bootloader gap + both partitions travel
# together. The eMMC boot.scr is self-locating (reads its own p2 PARTUUID at
# boot), so no on-image PARTUUID surgery is needed.
dd if="$SRC_DISK" of="$RAW" bs=16M conv=fsync status=progress

echo "=== [2] loop-mount + fsck the root partition ==="
LOOP="$(losetup --show -fP "$RAW")"
trap 'losetup -d "$LOOP" 2>/dev/null || true' EXIT
ROOTP="${LOOP}p2"
e2fsck -fy "$ROOTP" || true

echo "=== [3] shrink root fs to (used + ${SHRINK_SLACK_MB}MB slack) ==="
# resize2fs -M shrinks to minimum; then grow back by the slack so the box has a
# little headroom before firstboot's growpart expands to the full 58 GB eMMC.
MINBLK="$(resize2fs -P "$ROOTP" 2>/dev/null | awk -F': ' '/Estimated/{print $2}')"
BLKSZ="$(dumpe2fs -h "$ROOTP" 2>/dev/null | awk -F': *' '/Block size/{print $2}')"
SLACK_BLK=$(( SHRINK_SLACK_MB * 1024 * 1024 / BLKSZ ))
TARGET_BLK=$(( MINBLK + SLACK_BLK ))
echo "min=$MINBLK blocks, target=$TARGET_BLK blocks (blk=$BLKSZ)"
resize2fs "$ROOTP" "${TARGET_BLK}"
e2fsck -fy "$ROOTP" || true

echo "=== [4] seed firstboot + ssh key + tailscale key into the image ==="
MNT="$(mktemp -d)"
mount "$ROOTP" "$MNT"
install -Dm755 "$HERE/../firstboot/firstboot-emmc.sh" "$MNT/usr/local/sbin/firstboot-emmc.sh"
install -Dm644 "$HERE/../firstboot/pcat-firstboot.service" \
  "$MNT/etc/systemd/system/pcat-firstboot.service"
# enable the unit (symlink into multi-user.target.wants)
ln -sf ../pcat-firstboot.service \
  "$MNT/etc/systemd/system/multi-user.target.wants/pcat-firstboot.service"
# operator ssh key → root (adjust to a non-root user if the image has one)
install -Dm600 /dev/null "$MNT/root/.ssh/authorized_keys"
cat "$PUBKEY" >>"$MNT/root/.ssh/authorized_keys"
# clear the clone's identity NOW too (belt + braces; firstboot also does it)
: >"$MNT/etc/machine-id" || true
rm -f "$MNT/etc/ssh/ssh_host_"* || true
rm -rf "$MNT/var/lib/tailscale/"* || true
# tailscale key onto the BOOT partition (/boot lives on p1 for this board layout;
# if /boot is on the rootfs instead, this still lands at <root>/boot).
if [ -n "$TS_AUTHKEY" ]; then
  mkdir -p "$MNT/boot"
  printf '%s' "$TS_AUTHKEY" >"$MNT/boot/pcat-ts-authkey"
  chmod 600 "$MNT/boot/pcat-ts-authkey"
fi
sync
umount "$MNT"; rmdir "$MNT"

echo "=== [5] shrink the PARTITION + GPT to match the smaller fs ==="
# move p2's end down to just past the fs, then relocate the backup GPT header.
FS_BYTES=$(( TARGET_BLK * BLKSZ ))
P2_START="$(partx -g -o START "$ROOTP" 2>/dev/null | tr -d ' ')"
# recreate p2 with the smaller size via sgdisk (start kept, new end computed)
NEW_END=$(( P2_START + FS_BYTES / 512 ))
sgdisk -d 2 "$LOOP"
sgdisk -n 2:"$P2_START":"$NEW_END" -c 2:rootfs "$LOOP"
sgdisk -e "$LOOP" || true
losetup -d "$LOOP"; trap - EXIT

echo "=== [6] truncate the raw image to the end of p2 + copy to golden.img ==="
TRUNC_BYTES=$(( (NEW_END + 34) * 512 ))    # +34 sectors for the secondary GPT
truncate -s "$TRUNC_BYTES" "$RAW"
cp --sparse=always "$RAW" "$IMG"

echo "=== [7] checksum + compress ==="
IMG_BYTES="$(stat -c%s "$IMG")"
echo "$IMG_BYTES" >"$OUT/.img-size"
sha256sum "$IMG" | cut -d' ' -f1 >"$OUT/golden.img.zst.sha256.tmp"
# the installer verifies the DECOMPRESSED image, so the sha is of golden.img
sha256sum "$IMG" | awk '{print $1"  golden.img"}' >"$OUT/golden.img.zst.sha256"
zstd -3 -T0 -f "$IMG" -o "$OUT/golden.img.zst"
rm -f "$OUT/golden.img.zst.sha256.tmp" "$RAW"

echo "=== golden image ready ==="
echo "  image : $IMG  ($IMG_BYTES bytes)"
echo "  zst   : $OUT/golden.img.zst"
echo "  sha   : $OUT/golden.img.zst.sha256"
echo "Next: build-init-sd.sh writes an installer card carrying golden.img.zst."
