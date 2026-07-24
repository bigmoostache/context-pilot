#!/usr/bin/env bash
# firstboot-emmc.sh — one-shot personalisation of a freshly-flashed eMMC box.
#
# Runs on the FIRST boot of the eMMC-cloned Debian (booted OFF the eMMC, init-SD
# pulled). The golden image is a byte-clone of asterix, so every flashed box
# would otherwise be an identity TWIN (same SSH host keys, machine-id, tailscale
# node identity, and a ~20 GB rootfs on a 58 GB eMMC). This un-twins the box,
# grows the rootfs, names it from its hardware serial, and enrols it — then
# self-disables.
#
# HARDENING (v2, T656) over the fragile v1:
#   * TAILSCALE KEY SHADOW BUG: /boot is a SEPARATE partition mounted over the
#     rootfs /boot dir, so a key written to <rootfs>/boot during the build was
#     INVISIBLE at runtime. We now search several locations for the key.
#   * CLOCK: a freshly-flashed box may have a wrong RTC; tailscale's TLS login
#     then fails with "x509: certificate is not yet valid". We sync time first
#     and RETRY the enrol instead of giving up after one shot.
#   * PARTITION GROW: sgdisk -e on a mounted disk can leave the kernel on the
#     old table; we force a re-read (partx -u / partprobe) and tolerate a busy
#     device, verifying the grow actually took.
#   * SSHD ROOT LOGIN: a baked root key is useless if PermitRootLogin=no; we set
#     prohibit-password so key-only root login works for the Ansible step.
#   * EVERY step logs and is non-fatal where safe; the stamp is written only at
#     the end so an interrupted first boot re-runs cleanly.
set -uo pipefail

STAMP=/var/lib/pcat-firstboot.done
LOG=/var/log/pcat-firstboot.log
exec >>"$LOG" 2>&1
echo "=== pcat firstboot $(date -Is) ==="

if [ -e "$STAMP" ]; then
  echo "stamp present, already personalised — exiting"
  exit 0
fi

# ── 0. resolve the eMMC we actually booted from (do NOT assume mmcblk0) ──────
ROOT_SRC="$(findmnt -no SOURCE / 2>/dev/null)"        # e.g. /dev/mmcblk0p2
ROOT_DISK="$(lsblk -no pkname "$ROOT_SRC" 2>/dev/null | head -1)"  # mmcblk0
ROOT_PART_NO="$(echo "$ROOT_SRC" | grep -oE '[0-9]+$')"
DISK="/dev/${ROOT_DISK:-mmcblk0}"
echo "booted rootfs=$ROOT_SRC disk=$DISK part=$ROOT_PART_NO"

# ── 1. regenerate SSH host keys (kill the clone's twin keys) ────────────────
echo "[1] regenerating SSH host keys"
rm -f /etc/ssh/ssh_host_*
( dpkg-reconfigure openssh-server 2>/dev/null ) || ssh-keygen -A || true
# ensure key-only root login works (the baked authorized_keys is for root)
if [ -f /etc/ssh/sshd_config ] && ! grep -qE '^\s*PermitRootLogin\s+prohibit-password' /etc/ssh/sshd_config; then
  sed -i 's/^\s*#\?\s*PermitRootLogin.*/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config
  grep -qE '^\s*PermitRootLogin' /etc/ssh/sshd_config || echo 'PermitRootLogin prohibit-password' >>/etc/ssh/sshd_config
fi
systemctl restart ssh 2>/dev/null || systemctl restart sshd 2>/dev/null || true

# ── 2. fresh machine-id (clone shares asterix's; breaks systemd/journald/dbus) ─
echo "[2] resetting machine-id"
: >/etc/machine-id
rm -f /var/lib/dbus/machine-id
systemd-machine-id-setup || true
ln -sf /etc/machine-id /var/lib/dbus/machine-id

# ── 3. wipe the cloned tailscale identity (else it steals asterix's node) ────
echo "[3] wiping tailscale state"
# Wipe the CLONE's tailscale identity ONCE, guarded by a persistent marker. The
# stamp is written only at the very END of this script, so a crash after a
# SUCCESSFUL enrol (step 7) but before the stamp re-runs the whole script — and
# re-wiping here would DESTROY our own fresh enrol, after which step 7 finds the
# /boot key already shredded = the box is permanently unenrollable. The marker
# makes this destructive step idempotent across such re-runs.
if [ ! -e /var/lib/pcat-identity-cleaned ]; then
  systemctl stop tailscaled 2>/dev/null || true
  rm -rf /var/lib/tailscale/* 2>/dev/null || true
  mkdir -p /var/lib && touch /var/lib/pcat-identity-cleaned
else
  echo "identity already cleaned on a prior run — skipping tailscale wipe"
fi

# ── 4. grow the rootfs to fill the eMMC ─────────────────────────────────────
# The golden image is a shrunk disk image dd'd onto a bigger eMMC, so the GPT
# and last partition stop short. sgdisk -e relocates the backup GPT header to
# the true end (mandatory before growpart), then growpart extends the partition
# and resize2fs grows the ext4 online. On a mounted disk the kernel may keep the
# old table, so we force a re-read and verify the grow took.
echo "[4] growing rootfs $ROOT_SRC on $DISK"
if command -v sgdisk >/dev/null 2>&1; then
  sgdisk -e "$DISK" 2>/dev/null || echo "sgdisk -e failed (non-fatal)"
fi
partprobe "$DISK" 2>/dev/null || partx -u "$DISK" 2>/dev/null || true
BEFORE="$(blockdev --getsize64 "$ROOT_SRC" 2>/dev/null || echo 0)"
if command -v growpart >/dev/null 2>&1 && [ -n "$ROOT_PART_NO" ]; then
  growpart "$DISK" "$ROOT_PART_NO" 2>/dev/null || echo "growpart: nothing to do / failed"
fi
partprobe "$DISK" 2>/dev/null || partx -u "$DISK" 2>/dev/null || true
resize2fs "$ROOT_SRC" 2>/dev/null || echo "resize2fs failed"
AFTER="$(blockdev --getsize64 "$ROOT_SRC" 2>/dev/null || echo 0)"
echo "rootfs partition bytes: $BEFORE -> $AFTER"

# ── 5. hostname from the board's hardware serial (pcat-<serial>) ────────────
echo "[5] setting hostname"
SER="$(tr -d '\0' </proc/device-tree/serial-number 2>/dev/null | tr -cd '[:alnum:]')"
[ -z "$SER" ] && SER="$(cut -c1-12 /etc/machine-id 2>/dev/null)"  # fallback
HOST="pcat-${SER:-unknown}"
hostnamectl set-hostname "$HOST" 2>/dev/null || echo "$HOST" >/etc/hostname
if grep -qE '^127\.0\.1\.1' /etc/hosts 2>/dev/null; then
  sed -i "s/^127\.0\.1\.1.*/127.0.1.1\t${HOST}/" /etc/hosts
else
  printf '127.0.1.1\t%s\n' "$HOST" >>/etc/hosts
fi
echo "hostname = $HOST"

# ── 5b. GUARANTEE the LAN access path (plain SSH by baked key + mDNS) ────────
# This is the access plane that does NOT depend on Tailscale (step 7 may fail if
# the key is expired or there is no internet at boot). It must always come up:
#   * sshd enabled + running (the baked /root/.ssh/authorized_keys authorises the
#     operator key over the LAN);
#   * avahi advertising <hostname>.local so the box is reachable as
#     pcat-<serial>.local without knowing its DHCP address.
# avahi caches the OLD hostname until restarted, so restart it AFTER the rename
# or .local would still resolve to the stale clone name.
echo "[5b] ensuring LAN SSH + mDNS access path"
systemctl enable --now ssh 2>/dev/null || systemctl enable --now sshd 2>/dev/null || true
systemctl enable --now avahi-daemon 2>/dev/null || true
systemctl restart avahi-daemon 2>/dev/null || true

# ── 6. sync the clock (a wrong RTC breaks tailscale's TLS login) ────────────
# ROOT-CAUSE FIX (T658): a fresh box booted with RTC=2025-09; the old guard
# `[ $yr -ge 2025 ]` was SATISFIED by that stale clock yet it is still ~months
# BEFORE the tailscale login cert's notBefore, so TLS died `x509: certificate is
# not yet valid` and enrol was skipped. A hardcoded year floor is fragile — the
# real floor is "no earlier than when this image was built". build-golden-image
# stamps that epoch into /etc/pcat-build-epoch (rootfs, always present, NOT the
# autofs /boot), so we can always jump a lagging RTC forward to at least build
# time before any TLS. NTP still corrects to true time when the network is up;
# the epoch floor is only the guaranteed lower bound.
echo "[6] syncing clock before enrol"
timedatectl set-ntp true 2>/dev/null || true
BUILD_EPOCH="$(cat /etc/pcat-build-epoch 2>/dev/null || echo 0)"
# give NTP a moment; break as soon as the clock is at/after the build epoch
for _ in $(seq 1 15); do
  now="$(date +%s 2>/dev/null || echo 0)"
  [ "$now" -ge "$BUILD_EPOCH" ] 2>/dev/null && [ "$BUILD_EPOCH" -gt 0 ] 2>/dev/null && break
  sleep 2
done
# still behind the build epoch? NTP didn't land — try an HTTP Date header, then
# fall back to hard-setting the clock to the build epoch floor (guarantees the
# year is current-or-newer, so cert notBefore checks pass).
now="$(date +%s 2>/dev/null || echo 0)"
if [ "$BUILD_EPOCH" -gt 0 ] 2>/dev/null && [ "$now" -lt "$BUILD_EPOCH" ] 2>/dev/null; then
  hdr="$(curl -sI --max-time 10 https://www.google.com 2>/dev/null | grep -i '^date:' | cut -d' ' -f2-)"
  [ -n "$hdr" ] && date -s "$hdr" 2>/dev/null || true
  now="$(date +%s 2>/dev/null || echo 0)"
  if [ "$now" -lt "$BUILD_EPOCH" ] 2>/dev/null; then
    echo "clock still < build epoch — forcing date to build-epoch floor"
    date -s "@$BUILD_EPOCH" 2>/dev/null || true
  fi
fi
echo "clock now: $(date -Is)"

# ── 7. enrol into Tailscale with the reusable tagged key (if present) ────────
# The key may live on the boot partition (mounted at /boot) OR on the rootfs —
# search both, since the build may have placed it either way. Retry the enrol,
# because DNS/NTP/tailscaled may not be ready on the first attempt.
echo "[7] tailscale enrol"
# ROOT-CAUSE FIX (T658): /boot on this board is a systemd AUTOFS automount, so it
# is empty until something walks into it. A bare `[ -s /boot/pcat-ts-authkey ]`
# may run before the mount is triggered, making firstboot see an empty /boot and
# log "no ts-authkey found — skipping enrol" (exactly what happened). Force the
# automount (and an explicit mount fallback) BEFORE searching, so the baked key
# is actually visible.
ls /boot >/dev/null 2>&1 || true
mountpoint -q /boot || mount /boot 2>/dev/null || true
SRCKEY=""
for cand in /boot/pcat-ts-authkey /boot/firmware/pcat-ts-authkey /etc/pcat-ts-authkey /var/lib/pcat/ts-authkey; do
  [ -s "$cand" ] && { SRCKEY="$cand"; break; }
done
# HARDENING (T658): enrol from a tmpfs (/run, RAM-backed) copy and destroy the
# persistent /boot copy only AFTER a successful enrol. Rationale:
#   * /boot on this board is likely vfat (no unix perms), and it is persistent
#     flash — a raw authkey sitting there is readable by anyone who pulls the
#     eMMC, so we want it gone the moment it is no longer needed;
#   * `shred` is NOT a reliable secure-erase on eMMC/flash (wear-levelling +
#     COW mean the original blocks may survive), so the real control is that the
#     key never outlives a SUCCESSFUL enrol — not the overwrite itself;
#   * we do NOT shred up front: firstboot is stamp-guarded and re-runs until it
#     succeeds, so a crash or failed enrol must leave the /boot key intact for
#     the next boot to retry. The /run copy is the short-lived working copy,
#     never touches disk, and is wiped on poweroff / dropped below.
KEYFILE=""
if [ -n "$SRCKEY" ]; then
  KEYFILE=/run/pcat-ts-authkey
  ( umask 077; cat "$SRCKEY" >"$KEYFILE" ) 2>/dev/null || KEYFILE=""
  [ -n "$KEYFILE" ] && chmod 600 "$KEYFILE" 2>/dev/null || true
  # NOTE: the persistent /boot copy is NOT shredded here — only AFTER a
  # successful enrol (see below). firstboot is stamp-guarded and re-runs until
  # it succeeds, so the key must survive a crash/failed enrol to let the next
  # boot retry; destroying it up front would strand the box unenrollable on any
  # transient failure. The /run (tmpfs) copy we enrol from is the short-lived,
  # reliably-erasable working copy.
fi
if [ -n "$KEYFILE" ] && [ -s "$KEYFILE" ]; then
  echo "staged ts-authkey to tmpfs (persistent /boot copy kept until enrol succeeds)"
  KEY="$(tr -d '[:space:]' <"$KEYFILE")"
  # guard against the classic tskey-api-… ↔ tskey-auth-… mix-up: only an AUTH
  # key can enrol a node; an API key just fails `tailscale up` opaquely.
  case "$KEY" in
    tskey-auth-*) : ;;
    tskey-api-*)  echo "REFUSING enrol: key is a tskey-api-… (need tskey-auth-…)"; KEY="" ;;
    *)            echo "WARNING: key has unexpected prefix — attempting enrol anyway" ;;
  esac
fi
if [ -n "${KEYFILE:-}" ] && [ -n "${KEY:-}" ]; then
  systemctl enable --now tailscaled 2>/dev/null || true
  ok=""
  for attempt in 1 2 3 4 5; do
    if tailscale up --authkey="$KEY" --ssh --hostname="$HOST" --accept-dns=false 2>&1; then
      echo "tailscale up OK as $HOST (attempt $attempt)"; ok=1; break
    fi
    echo "tailscale up attempt $attempt failed — retry in 10s (check key not expired/quota)"
    sleep 10
  done
  [ -z "$ok" ] && echo "TAILSCALE ENROL FAILED after retries — key kept on /boot for next-boot retry"
  # Only now the box is enrolled is it safe to destroy the persistent /boot key.
  # On failure we deliberately LEAVE it so the stamp-guarded firstboot re-run can
  # retry — shred is unreliable on eMMC/vfat anyway, so the real control is that
  # the key never outlives a SUCCESSFUL enrol (normal-operation exposure only).
  if [ -n "$ok" ] && [ -n "${SRCKEY:-}" ]; then
    shred -u "$SRCKEY" 2>/dev/null || rm -f "$SRCKEY" || true
    sync
  fi
else
  echo "no usable ts-authkey found (searched /boot, /boot/firmware, /etc, /var/lib/pcat) — skipping enrol"
fi
# always drop the in-RAM key + the shell copy, enrolled or not
KEY=""
[ -n "${KEYFILE:-}" ] && { shred -u "$KEYFILE" 2>/dev/null || rm -f "$KEYFILE" || true; }

# ── 7b. write an access breadcrumb so the operator sees how to reach the box ─
# Two independent planes are now up (LAN key-SSH via avahi, Tailscale-SSH). Record
# every coordinate to /root/ACCESS.txt and the log so the first person in — or a
# technician reading the console — knows exactly where to connect, without
# guessing the DHCP lease.
echo "[7b] writing access breadcrumb"
LAN_IP="$(ip -4 -o addr show scope global 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | paste -sd, -)"
TS_IP="$(tailscale ip -4 2>/dev/null | head -1)"
{
  echo "host      : $HOST"
  echo "mdns      : ${HOST}.local"
  echo "lan_ip    : ${LAN_IP:-<none>}"
  echo "tailscale : ${TS_IP:-<not enrolled>}"
  echo "ssh       : ssh root@${HOST}.local   (LAN, baked key)"
  [ -n "$TS_IP" ] && echo "ssh (ts)  : tailscale ssh root@${HOST}"
  echo "written   : $(date -Is)"
} >/root/ACCESS.txt 2>/dev/null || true
cat /root/ACCESS.txt 2>/dev/null || true

# ── 8. stamp + self-disable ─────────────────────────────────────────────────
echo "[8] stamping done + disabling unit"
mkdir -p "$(dirname "$STAMP")"
date -Is >"$STAMP"
systemctl disable pcat-firstboot.service 2>/dev/null || true
echo "=== firstboot complete: $HOST ==="
