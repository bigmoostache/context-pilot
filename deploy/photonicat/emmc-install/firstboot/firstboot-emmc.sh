#!/usr/bin/env bash
# firstboot-emmc.sh — one-shot personalisation of a freshly-flashed eMMC box.
#
# Runs on the FIRST boot of the eMMC-cloned Debian (the box booted OFF the eMMC,
# init-SD already pulled). The golden image is a byte-clone of asterix, so every
# flashed box would otherwise be an identity TWIN (same SSH host keys, same
# machine-id, same tailscale node identity, same 20 GB rootfs on a 58 GB eMMC).
# This script un-twins the box, grows the rootfs to fill the eMMC, names it from
# its hardware serial, and enrols it into the tailnet — then disables itself so
# it never runs again.
#
# Installed by build-golden-image.sh as /usr/local/sbin/firstboot-emmc.sh, driven
# by the paired pcat-firstboot.service (WantedBy multi-user.target). A stamp file
# /var/lib/pcat-firstboot.done guards re-entry even if the unit is somehow
# re-enabled.
#
# The tailscale auth key is NOT baked into git — it is read at RUNTIME from
# /boot/pcat-ts-authkey (dropped onto the image by build-golden-image.sh from a
# key minted at build time). Absent key ⇒ tailscale enrol is skipped, not fatal.
set -uo pipefail

STAMP=/var/lib/pcat-firstboot.done
LOG=/var/log/pcat-firstboot.log
exec >>"$LOG" 2>&1
echo "=== pcat firstboot $(date -Is) ==="

if [ -e "$STAMP" ]; then
  echo "stamp present, already personalised — exiting"
  exit 0
fi

# ── 1. regenerate SSH host keys (kill the clone's twin keys) ────────────────
echo "[1] regenerating SSH host keys"
rm -f /etc/ssh/ssh_host_*
dpkg-reconfigure openssh-server || ssh-keygen -A
systemctl restart ssh 2>/dev/null || systemctl restart sshd 2>/dev/null || true

# ── 2. fresh machine-id (clone shares asterix's; breaks systemd/journald/dbus) ─
echo "[2] resetting machine-id"
: >/etc/machine-id
rm -f /var/lib/dbus/machine-id
systemd-machine-id-setup
ln -sf /etc/machine-id /var/lib/dbus/machine-id

# ── 3. wipe the cloned tailscale identity (else it steals asterix's node) ────
echo "[3] wiping tailscale state"
systemctl stop tailscaled 2>/dev/null || true
rm -rf /var/lib/tailscale/*

# ── 4. grow the rootfs to fill the eMMC ─────────────────────────────────────
# The golden image is a ~20 GB disk image dd'd onto a 58 GB eMMC, so the GPT and
# the last partition stop short. sgdisk -e relocates the backup GPT header to the
# true end of the eMMC (mandatory after writing a smaller image onto a bigger
# disk — growpart refuses otherwise), then growpart extends p2 and resize2fs
# grows the ext4 online.
echo "[4] growing rootfs on /dev/mmcblk0p2"
sgdisk -e /dev/mmcblk0 || echo "sgdisk -e failed (non-fatal, continuing)"
partprobe /dev/mmcblk0 || true
growpart /dev/mmcblk0 2 || echo "growpart failed (maybe already full)"
resize2fs /dev/mmcblk0p2 || echo "resize2fs failed"

# ── 5. hostname from the board's hardware serial (pcat-<serial>) ────────────
# device-tree serial-number is the RK3568's per-board unique id, so pcat-<serial>
# is collision-free across a fleet without a central allocator.
echo "[5] setting hostname"
SER="$(tr -d '\0' </proc/device-tree/serial-number 2>/dev/null)"
[ -z "$SER" ] && SER="$(cat /etc/machine-id | cut -c1-12)"  # fallback
HOST="pcat-${SER}"
hostnamectl set-hostname "$HOST" 2>/dev/null || echo "$HOST" >/etc/hostname
# keep /etc/hosts coherent so sudo doesn't stall on name resolution
if grep -qE '^127\.0\.1\.1' /etc/hosts; then
  sed -i "s/^127\.0\.1\.1.*/127.0.1.1\t${HOST}/" /etc/hosts
else
  echo -e "127.0.1.1\t${HOST}" >>/etc/hosts
fi
echo "hostname = $HOST"

# ── 6. enrol into Tailscale with the reusable tagged key (if present) ────────
echo "[6] tailscale enrol"
KEYFILE=/boot/pcat-ts-authkey
if [ -s "$KEYFILE" ]; then
  systemctl enable --now tailscaled
  sleep 2
  tailscale up \
    --authkey="$(tr -d '[:space:]' <"$KEYFILE")" \
    --ssh \
    --hostname="$HOST" \
    --accept-dns=false \
    && echo "tailscale up OK as $HOST" \
    || echo "tailscale up FAILED (check key validity)"
  # the key is single-image-reusable but still a secret at rest — shred it now
  # that this box is enrolled, so a pulled eMMC can't leak the fleet key.
  shred -u "$KEYFILE" 2>/dev/null || rm -f "$KEYFILE"
else
  echo "no /boot/pcat-ts-authkey — skipping tailscale enrol"
fi

# ── 7. stamp + self-disable ─────────────────────────────────────────────────
echo "[7] stamping done + disabling unit"
mkdir -p "$(dirname "$STAMP")"
date -Is >"$STAMP"
systemctl disable pcat-firstboot.service 2>/dev/null || true
echo "=== firstboot complete: $HOST ==="
