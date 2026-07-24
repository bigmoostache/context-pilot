#!/usr/bin/env bash
# pcat-provision.sh — install Context Pilot onto the freshly-booted eMMC box by
# running the existing Ansible play (deploy/ansible/site.yml) LOCALLY, so the box
# provisions itself with no external control node.
#
# WHERE THIS RUNS: on the eMMC Debian, on first boot, AFTER firstboot-emmc.sh has
# un-twinned + resized + tailscale-enrolled the box. Driven by
# pcat-provision.service (ordered after pcat-firstboot + network-online).
#
# WHY A SEPARATE PHASE (not folded into firstboot): this step is slow and
# network-heavy — it downloads the release bundle + Caddy and COMPILES the
# GC9307 display driver from source (tasks/display.yml). Keeping it out of
# firstboot lets first boot stay lean (identity + access come up fast) and gives
# provisioning its own generous timeout + its own LCD state, so a slow or failed
# install never wedges the box's boot or its access planes.
#
# MODEL A (self-provision) secrets lifecycle — mirrors the tailscale key:
#   * the operator drops /boot/pcat-provision.yml at build time (admin email +
#     cp_provider_keys + release tag + install_display), passed to ansible as
#     `-e @<file>` (it is YAML, NOT a shell env file);
#   * we run the play with `-c local -i 'localhost,'` so localhost == this box:
#     tasks/fetch.yml's delegate_to:localhost downloads the artifact straight
#     onto the box, deploy/keys/seed/start/display all act on the box itself;
#   * on success we SHRED the secrets file so a pulled eMMC can't leak the
#     provider API keys, then stamp + self-disable.
#
# The GitHub release + Caddy are PUBLIC, so no GitHub token is ever needed — the
# only secrets in the file are the provider API keys + the admin email.
set -uo pipefail

BASE=/opt/pcat-provision
PLAYBOOK="$BASE/ansible/site.yml"
LCD=/opt/pcat-install/lcd/pcat_lcd.py     # painter shipped by the init-SD image
STAMP=/var/lib/pcat-provision.done
LOG=/var/log/pcat-provision.log
exec >>"$LOG" 2>&1
echo "=== pcat provision $(date -Is) ==="

# LCD paint is best-effort — a broken painter must never abort provisioning.
lcd() { [ -x "$LCD" ] || [ -f "$LCD" ] && python3 "$LCD" "$@" 2>/dev/null || true; }

fail() {
  echo "PROVISION FAIL: $*"
  lcd error "$*"
  # Leave the box ON and reachable (firstboot already brought up SSH + tailscale)
  # so the operator can inspect $LOG and re-run. Do NOT stamp — a fixed re-run
  # then retries cleanly.
  exit 1
}

# ── already provisioned? (stamp survives reboots — provisioning is once-per-box)
if [ -e "$STAMP" ]; then
  echo "stamp present — already provisioned, exiting"
  exit 0
fi

# ── locate ansible + the playbook ───────────────────────────────────────────
command -v ansible-playbook >/dev/null 2>&1 || fail "NO ANSIBLE"
[ -f "$PLAYBOOK" ] || fail "NO PLAYBOOK"

# ── locate the secrets file (same multi-path search as the tailscale key) ────
VARS=""
for cand in /boot/pcat-provision.yml /boot/firmware/pcat-provision.yml \
            /etc/pcat-provision.yml /var/lib/pcat/provision.yml; do
  [ -s "$cand" ] && { VARS="$cand"; break; }
done
[ -n "$VARS" ] || fail "NO PROVISION VARS (searched /boot, /etc, /var/lib/pcat)"
echo "using vars file $VARS"

# ── run the play against ourselves ──────────────────────────────────────────
# -c local: no SSH, act on this host. -i 'localhost,': single-host inventory.
# The play logs in "as root" (become:false) — we already are root here.
lcd provisioning
echo "running ansible-playbook -c local against localhost"
if ansible-playbook -c local -i 'localhost,' "$PLAYBOOK" -e @"$VARS"; then
  echo "ansible play OK"
else
  fail "ANSIBLE PLAY FAILED"
fi

# ── shred the secrets now the box is provisioned ────────────────────────────
echo "shredding $VARS"
shred -u "$VARS" 2>/dev/null || rm -f "$VARS" || true

# ── stamp + self-disable ────────────────────────────────────────────────────
echo "stamping done + disabling unit"
mkdir -p "$(dirname "$STAMP")"
date -Is >"$STAMP"
systemctl disable pcat-provision.service 2>/dev/null || true
lcd done
echo "=== provision complete ==="
