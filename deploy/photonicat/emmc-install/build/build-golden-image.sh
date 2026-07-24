#!/usr/bin/env bash
# build-golden-image.sh — clone asterix's Debian SD into a shrunk, seeded
# "golden" eMMC image ready to dd onto any Photonicat 2's eMMC.
#
# WHERE THIS RUNS: a Linux host with losetup/e2fsck/resize2fs/sgdisk/zstd. NOT
# the Mac Mini (no reader, macOS lacks these). asterix is the natural host.
#
# ── CRITICAL SAFETY NOTES (v2 hardening, T656) ──────────────────────────────
# 1. DO NOT clone a MOUNTED, live filesystem. `dd if=/dev/mmcblk1` while its
#    rootfs is mounted rw yields a torn image (in-flight writes, dirty journal).
#    Clone from a source that is NOT the running root: image the SD from a
#    SECOND machine, or boot asterix off the eMMC/another medium first. This
#    script REFUSES to image a disk that carries the current root mount.
# 2. The scratch dir ($OUT) MUST be on a DIFFERENT disk from $SRC_DISK — writing
#    the growing image file onto the very filesystem you are block-imaging makes
#    the image contain (a partial copy of) itself = corruption. Enforced below.
# 3. Preserving the partition PARTUUID: boot.scr resolves root dynamically from
#    p2's PARTUUID, so a self-consistent new GUID still boots — but we preserve
#    the original GUID anyway (sgdisk -u) to avoid surprising anything that
#    references it.
#
# PRODUCES (in $OUT): golden.img, golden.img.zst, golden.img.zst.sha256,
# .img-size. SEEDS firstboot unit + operator ssh key (rootfs) + the tailscale
# key onto the BOOT partition (so it is not shadowed by the /boot mount at
# runtime — the v1 shadow bug).
set -euo pipefail

SRC_DISK="${SRC_DISK:-/dev/mmcblk1}"        # source Debian SD (whole disk)
OUT="${OUT:-/mnt/scratch/pcat-golden}"      # scratch on a DIFFERENT disk, ~40 GB
PUBKEY="${PUBKEY:?set PUBKEY=/path/to/authorized_key.pub}"
TS_AUTHKEY="${TS_AUTHKEY:-}"                 # reusable tagged tailscale key (opt)
BOOT_PARTNO="${BOOT_PARTNO:-1}"             # boot partition number (label 'boot')
ROOT_PARTNO="${ROOT_PARTNO:-2}"            # rootfs partition number
SHRINK_SLACK_MB="${SHRINK_SLACK_MB:-1500}"
HERE="$(cd "$(dirname "$0")" && pwd)"

# ── Model A self-provision (optional): bake the Ansible tree + a runtime + the
# provision unit into the golden image so each box installs Context Pilot itself
# on first boot. PROVISION_VARS points at a pcat-provision.yml (admin email +
# provider keys + release tag); it is written to the BOOT partition and shredded
# on the box after a successful install. Unset PROVISION_VARS to skip entirely
# (image comes up as a bare Debian box, provisioning done later by hand).
ANSIBLE_DIR="${ANSIBLE_DIR:-$HERE/../../../ansible}"   # deploy/ansible tree
PROVISION_VARS="${PROVISION_VARS:-}"                    # path to pcat-provision.yml
INSTALL_ANSIBLE="${INSTALL_ANSIBLE:-1}"                # chroot-install ansible into image

# ── preflight: required tools ───────────────────────────────────────────────
for t in dd losetup e2fsck resize2fs dumpe2fs sgdisk zstd sha256sum truncate partx; do
  command -v "$t" >/dev/null 2>&1 || { echo "MISSING TOOL: $t"; exit 1; }
done
[ -b "$SRC_DISK" ] || { echo "SRC_DISK $SRC_DISK not a block device"; exit 1; }
[ -r "$PUBKEY" ]  || { echo "PUBKEY $PUBKEY not readable"; exit 1; }

# ── SAFETY 1: refuse to image the disk that carries the running root ─────────
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null || true)"
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1 || true)"
if [ -n "$ROOT_DISK" ] && [ "/dev/$ROOT_DISK" = "$SRC_DISK" ]; then
  echo "REFUSING: $SRC_DISK carries the live root ($ROOT_SRC)."
  echo "Clone from a second machine, or boot off another medium first."
  exit 1
fi
# refuse if any partition of SRC_DISK is currently mounted (torn-image risk)
if lsblk -nro MOUNTPOINT "$SRC_DISK" 2>/dev/null | grep -q .; then
  echo "REFUSING: $SRC_DISK has mounted partitions. Unmount them first."
  lsblk "$SRC_DISK"
  exit 1
fi

mkdir -p "$OUT"
# ── SAFETY 2: $OUT must be on a different disk than SRC_DISK ─────────────────
OUT_SRC="$(findmnt -no SOURCE --target "$OUT" 2>/dev/null || true)"
OUT_DISK="$(lsblk -no pkname "$OUT_SRC" 2>/dev/null | head -1 || true)"
if [ -n "$OUT_DISK" ] && [ "/dev/$OUT_DISK" = "$SRC_DISK" ]; then
  echo "REFUSING: scratch $OUT is on $SRC_DISK — use a different disk."
  exit 1
fi

RAW="$OUT/golden-raw.img"
IMG="$OUT/golden.img"

echo "=== [1] full-disk image of $SRC_DISK ==="
dd if="$SRC_DISK" of="$RAW" bs=16M conv=fsync status=progress

echo "=== [2] loop-mount + fsck the root partition ==="
LOOP="$(losetup --show -fP "$RAW")"
cleanup() { losetup -d "$LOOP" 2>/dev/null || true; }
trap cleanup EXIT
# wait for the partition nodes losetup -P creates
for _ in $(seq 1 10); do [ -e "${LOOP}p${ROOT_PARTNO}" ] && break; sleep 1; done
ROOTP="${LOOP}p${ROOT_PARTNO}"
[ -e "$ROOTP" ] || { echo "loop partition $ROOTP never appeared"; exit 1; }
# capture the ORIGINAL partition GUID before we touch the table
ORIG_GUID="$(sgdisk -i "$ROOT_PARTNO" "$LOOP" 2>/dev/null | awk -F': ' '/Partition unique GUID/{print $2}' | tr -d ' ')"
echo "original p${ROOT_PARTNO} GUID = ${ORIG_GUID:-<none>}"
e2fsck -fy "$ROOTP" || true

echo "=== [3] shrink root fs to (used + ${SHRINK_SLACK_MB}MB slack) ==="
MINBLK="$(resize2fs -P "$ROOTP" 2>/dev/null | awk -F':' '/[Ee]stimated minimum/{gsub(/ /,"",$2); print $2}')"
BLKSZ="$(dumpe2fs -h "$ROOTP" 2>/dev/null | awk -F':' '/Block size/{gsub(/ /,"",$2); print $2}')"
[ -n "$MINBLK" ] && [ -n "$BLKSZ" ] || { echo "could not read fs geometry"; exit 1; }
SLACK_BLK=$(( SHRINK_SLACK_MB * 1024 * 1024 / BLKSZ ))
TARGET_BLK=$(( MINBLK + SLACK_BLK ))
echo "min=$MINBLK target=$TARGET_BLK blocks (blk=$BLKSZ)"
resize2fs "$ROOTP" "${TARGET_BLK}"
e2fsck -fy "$ROOTP" || true

echo "=== [4] seed firstboot + ssh key (rootfs) + tailscale key (BOOT part) ==="
MNT="$(mktemp -d)"
mount "$ROOTP" "$MNT"
install -Dm755 "$HERE/../firstboot/firstboot-emmc.sh" "$MNT/usr/local/sbin/firstboot-emmc.sh"
install -Dm644 "$HERE/../firstboot/pcat-firstboot.service" \
  "$MNT/etc/systemd/system/pcat-firstboot.service"
ln -sf ../pcat-firstboot.service \
  "$MNT/etc/systemd/system/multi-user.target.wants/pcat-firstboot.service"

# Stamp the build epoch (rootfs, NOT the autofs /boot) so firstboot can floor a
# lagging RTC forward to at least build time before the tailscale TLS login —
# fixes the "clock in 2025 → x509 not-yet-valid → enrol skipped" failure (T658).
date +%s >"$MNT/etc/pcat-build-epoch"

# ── Model A self-provision payload (optional): bake the Ansible tree + wrapper +
# unit so the box installs Context Pilot itself on first boot. Skipped entirely
# when PROVISION_VARS is unset AND the ansible tree is absent (bare-Debian image).
if [ -n "$PROVISION_VARS" ] || [ -d "$ANSIBLE_DIR" ]; then
  if [ -d "$ANSIBLE_DIR" ]; then
    echo "seeding self-provision payload from $ANSIBLE_DIR"
    install -d "$MNT/opt/pcat-provision"
    # the whole deploy/ansible tree (playbook + tasks + templates) — NOT secret
    cp -a "$ANSIBLE_DIR/." "$MNT/opt/pcat-provision/ansible/"
    # strip any operator-local secret files that may sit in the tree (*.local.yml)
    find "$MNT/opt/pcat-provision/ansible" -name '*.local.yml' -delete 2>/dev/null || true
    install -Dm755 "$HERE/../installer/pcat-provision.sh" \
      "$MNT/opt/pcat-provision/pcat-provision.sh"
    install -Dm644 "$HERE/../installer/pcat-provision.service" \
      "$MNT/etc/systemd/system/pcat-provision.service"
    ln -sf ../pcat-provision.service \
      "$MNT/etc/systemd/system/multi-user.target.wants/pcat-provision.service"
    # the painter lives under /opt/pcat-install on the init-SD, but the eMMC clone
    # has no init-SD payload — ship the painter here too so the LCD state works.
    install -Dm755 "$HERE/../lcd/pcat_lcd.py" "$MNT/opt/pcat-install/lcd/pcat_lcd.py"
    # chroot-install the ansible runtime so `ansible-playbook -c local` exists on
    # the box (missing = the provision unit fails loud, box stays reachable).
    if [ "$INSTALL_ANSIBLE" = "1" ] && command -v chroot >/dev/null 2>&1; then
      for d in dev proc sys; do mount --bind "/$d" "$MNT/$d" 2>/dev/null || true; done
      cp /etc/resolv.conf "$MNT/etc/resolv.conf" 2>/dev/null || true
      chroot "$MNT" /bin/bash -c '
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -y &&
        apt-get install -y --no-install-recommends ansible git curl ca-certificates
      ' || echo "WARNING: ansible install in chroot failed — provisioning will no-op until installed"
      for d in dev proc sys; do umount "$MNT/$d" 2>/dev/null || true; done
    else
      echo "NOTE: skipping ansible chroot-install (INSTALL_ANSIBLE=$INSTALL_ANSIBLE) — ensure the base image has ansible"
    fi
  else
    echo "WARNING: PROVISION_VARS set but ANSIBLE_DIR $ANSIBLE_DIR missing — no payload seeded"
  fi
fi

# operator ssh key -> root, and allow key-only root login
install -Dm600 /dev/null "$MNT/root/.ssh/authorized_keys"
cat "$PUBKEY" >>"$MNT/root/.ssh/authorized_keys"
if [ -f "$MNT/etc/ssh/sshd_config" ]; then
  sed -i 's/^\s*#\?\s*PermitRootLogin.*/PermitRootLogin prohibit-password/' "$MNT/etc/ssh/sshd_config" || true
fi
# clear clone identity now (belt+braces; firstboot repeats)
: >"$MNT/etc/machine-id" || true
rm -f "$MNT/etc/ssh/ssh_host_"* || true
rm -rf "$MNT/var/lib/tailscale/"* || true
umount "$MNT"

# The tailscale key must survive the /boot mount at runtime, so write it onto
# the BOOT partition itself (label 'boot'), NOT the rootfs /boot dir (which the
# boot partition mounts over, hiding anything beneath — the v1 shadow bug).
# The tailscale key AND the self-provision secrets must survive the /boot mount
# at runtime, so write them onto the BOOT partition itself (label 'boot'), NOT
# the rootfs /boot dir (which the boot partition mounts over, hiding anything
# beneath — the v1 shadow bug). Both are SHREDDED on the box after use
# (pcat-ts-authkey after tailscale enrol; pcat-provision.yml after the install).
if [ -n "$TS_AUTHKEY" ] || [ -n "$PROVISION_VARS" ]; then
  # guard the classic tskey-api-… ↔ tskey-auth-… mix-up at BUILD time (fail fast
  # rather than ship an image that silently can't enrol) — T658.
  if [ -n "$TS_AUTHKEY" ]; then
    case "$TS_AUTHKEY" in
      tskey-auth-*) : ;;
      tskey-api-*)  echo "FATAL: TS_AUTHKEY is a tskey-api-… key; enrol needs a tskey-auth-… key"; exit 1 ;;
      *)            echo "WARNING: TS_AUTHKEY has an unexpected prefix — proceeding, but verify it is an auth key" ;;
    esac
  fi
  BOOTP="${LOOP}p${BOOT_PARTNO}"
  if [ -e "$BOOTP" ]; then
    mount "$BOOTP" "$MNT"
    if [ -n "$TS_AUTHKEY" ]; then
      printf '%s' "$TS_AUTHKEY" >"$MNT/pcat-ts-authkey"
      chmod 600 "$MNT/pcat-ts-authkey"
      # ASSERT the key actually landed on the boot partition — the whole enrol
      # depends on it, and a silent miss here is exactly what broke T658.
      [ -s "$MNT/pcat-ts-authkey" ] || { echo "FATAL: pcat-ts-authkey did not land on boot partition $BOOTP"; umount "$MNT"; exit 1; }
      echo "tailscale key written + verified on boot partition p${BOOT_PARTNO}"
    fi
    if [ -n "$PROVISION_VARS" ]; then
      if [ -r "$PROVISION_VARS" ]; then
        install -m600 "$PROVISION_VARS" "$MNT/pcat-provision.yml"
        [ -s "$MNT/pcat-provision.yml" ] || { echo "FATAL: pcat-provision.yml did not land on boot partition $BOOTP"; umount "$MNT"; exit 1; }
        echo "provision secrets written + verified on boot partition p${BOOT_PARTNO}"
      else
        echo "WARNING: PROVISION_VARS $PROVISION_VARS not readable — secrets NOT seeded"
      fi
    fi
    umount "$MNT"
  else
    echo "FATAL: boot partition $BOOTP not found — key/secrets cannot be seeded"; exit 1
  fi
fi
rmdir "$MNT"

echo "=== [5] shrink the PARTITION + GPT to match the smaller fs ==="
FS_BYTES=$(( TARGET_BLK * BLKSZ ))
P2_START="$(partx -g -o START -n "$ROOT_PARTNO" "$LOOP" 2>/dev/null | tr -dc '0-9')"
[ -n "$P2_START" ] || { echo "could not read p${ROOT_PARTNO} start sector"; exit 1; }
NEW_END=$(( P2_START + FS_BYTES / 512 - 1 ))
echo "p${ROOT_PARTNO}: start=$P2_START new_end=$NEW_END"
sgdisk -d "$ROOT_PARTNO" "$LOOP"
sgdisk -n "${ROOT_PARTNO}:${P2_START}:${NEW_END}" -t "${ROOT_PARTNO}:8300" -c "${ROOT_PARTNO}:rootfs" "$LOOP"
# restore the ORIGINAL partition GUID so nothing that references it breaks
[ -n "$ORIG_GUID" ] && sgdisk -u "${ROOT_PARTNO}:${ORIG_GUID}" "$LOOP"
sgdisk -e "$LOOP" || true
losetup -d "$LOOP"; trap - EXIT

echo "=== [6] truncate raw image to end of p${ROOT_PARTNO} (+secondary GPT) ==="
TRUNC_BYTES=$(( (NEW_END + 34) * 512 ))
truncate -s "$TRUNC_BYTES" "$RAW"
cp --sparse=always "$RAW" "$IMG"

echo "=== [7] checksum + compress ==="
IMG_BYTES="$(stat -c%s "$IMG")"
echo "$IMG_BYTES" >"$OUT/.img-size"
sha256sum "$IMG" | awk '{print $1"  golden.img"}' >"$OUT/golden.img.zst.sha256"
zstd -3 -T0 -f "$IMG" -o "$OUT/golden.img.zst"
rm -f "$RAW"

echo "=== golden image ready ==="
echo "  image : $IMG ($IMG_BYTES bytes)"
echo "  zst   : $OUT/golden.img.zst"
echo "  sha   : $OUT/golden.img.zst.sha256"
echo "Next: build-init-sd.sh writes an installer card carrying golden.img.zst."
