#!/bin/sh
# pcat-ula — assign this box's fleet ULA: one /64 per on-board Ethernet port.
#
# WHY THIS EXISTS: a freshly-installed box has no name of ours yet and no
# predictable IPv4 (it takes whatever the client's DHCP hands out), so the
# operator has to HUNT for it before Ansible can even connect. A ULA (RFC 4193)
# whose interface-id is the hardware serial is known *before the box has ever
# booted*: the /48 is a product constant, the interface-id is on the label. No
# DHCP, no router, no mDNS, no cooperation from the client's network — a ULA
# simply exists on the link.
#
# WHO CALLS THIS — both, deliberately:
#   * pcat-ula.service      at boot, so the address exists as early as possible;
#   * NM dispatcher 50-pcat-ula on every interface `up`, because NetworkManager
#     takes ownership of the addresses of a connection it manages and drops the
#     ones it did not add itself.
# `ip addr replace` is idempotent, so being called twice is a no-op.
#
# ONE /64 PER PORT: the Photonicat 2 has two Ethernet ports. The SAME address on
# both would collide (duplicate address detection) the moment a technician plugs
# them into the same switch, so the subnet-id carries the port index instead:
# end0 -> :1:, end1 -> :2:. Same interface-id on every port, no conflict.
#
# NEVER FATAL: every failure path exits 0 with a journal line. A box that misses
# its ULA is still reachable over DHCP/link-local — losing the convenience must
# not cost a boot.
set -u

CONF=/etc/default/pcat-ula
WAIT_SECS=20        # udev may not have created the ports yet when we run at boot
NETWORKD_DIR=/etc/systemd/network
OURS_PREFIX=05-pcat-ula-      # sorts ahead of the distro's 10-* files (first match wins)

log() { echo "pcat-ula: $*" >&2; }

# ── declarative half: hand the address to systemd-networkd ───────────────────
# MEASURED on RK3576 / systemd 257: `networkctl reconfigure <if>` DROPS an address
# networkd did not configure itself, so `ip addr replace` alone is not durable — a
# DHCP renew or a carrier bounce can take the ULA away. (`ManageForeignAddresses`
# does not exist in 257; it is silently ignored.) The address must be DECLARED.
#
# The catch: only the FIRST matching .network file applies, and the distro's file
# matches every port at once (netplan ships `[Match] Name=e*`), so a drop-in on it
# could not carry a per-port address. Hence one file per interface, sorted ahead of
# the distro's, inheriting its settings VERBATIM so DHCP and friends keep working.
#
# We deliberately do NOT reconfigure the link here: the imperative assignment above
# already made the address live, and reconfiguring would bounce a link that an
# Ansible run may be using. The declaration takes over at the next boot/reconfigure.
networkd_source_file() {   # $1=iface → the .network to inherit from ('' if none)
  eff="$(networkctl status "$1" 2>/dev/null \
         | sed -n 's/^[[:space:]]*Network File:[[:space:]]*//p' | head -1)"
  case "$eff" in
    # Already ours: read the source recorded inside, never regenerate from our own
    # output (that would slowly drift away from the distro's settings).
    *"/$OURS_PREFIX"*) sed -n 's/^# pcat-ula-source: //p' "$eff" 2>/dev/null | head -1 ;;
    /*)                printf '%s\n' "$eff" ;;
    *)                 printf '' ;;
  esac
}

networkd_declare() {   # $1=iface  $2=addr/prefixlen
  [ -d /run/systemd/system ] || return 0
  command -v networkctl >/dev/null 2>&1 || return 0
  systemctl is-active --quiet systemd-networkd 2>/dev/null || return 0

  src="$(networkd_source_file "$1")"
  out="$NETWORKD_DIR/$OURS_PREFIX$1.network"
  tmp="$out.new"
  mkdir -p "$NETWORKD_DIR" || return 0
  {
    echo "# Managed by pcat-ula — DO NOT EDIT, regenerated at every boot."
    echo "# Declares this port's fleet ULA so systemd-networkd OWNS it: an address"
    echo "# networkd did not configure is dropped on reconfigure (measured)."
    if [ -n "$src" ] && [ -r "$src" ]; then
      echo "# pcat-ula-source: $src"
      echo "# pcat-ula-source-sha256: $(sha256sum <"$src" 2>/dev/null | cut -d' ' -f1)"
    fi
    echo "[Match]"
    echo "Name=$1"
    if [ -n "$src" ] && [ -r "$src" ]; then
      # Everything the distro configured, minus its [Match] (ours is per-port).
      awk '/^\[/ { skip = ($0 == "[Match]") } !skip' "$src"
    else
      # Nothing to inherit — keep the port self-sufficient rather than half-configured.
      printf '[Network]\nDHCP=yes\nLinkLocalAddressing=ipv6\n'
    fi
    echo "[Network]"
    echo "Address=$2"
  } >"$tmp" 2>/dev/null || { rm -f "$tmp"; return 0; }

  if [ -f "$out" ] && cmp -s "$tmp" "$out"; then
    rm -f "$tmp"                      # unchanged: no write, no reload
    return 0
  fi
  mv -f "$tmp" "$out" || { rm -f "$tmp"; return 0; }
  log "$1 declared to networkd in $out"
  NETWORKD_CHANGED=1
}

[ -r "$CONF" ] || { log "no $CONF — nothing to assign"; exit 0; }
# shellcheck source=/dev/null
. "$CONF"
: "${PCAT_ULA_PREFIX:=}"
: "${PCAT_ULA_IID:=}"
if [ -z "$PCAT_ULA_PREFIX" ] || [ -z "$PCAT_ULA_IID" ]; then
  log "prefix or interface-id empty in $CONF — nothing to assign"
  exit 0
fi
command -v ip >/dev/null 2>&1 || { log "iproute2 missing"; exit 0; }

# On-board Ethernet ONLY. The allowlist excludes enx* (USB NIC, MAC-derived
# name), wwan*/usb* (the 5G modem and tethering), wlan*, and every virtual
# interface: the ULA belongs on the ports a technician can physically plug into.
is_wired() {
  case "$1" in
    enx*)                          return 1 ;;
    eth*|end*|eno*|enp*|ens*|enP*) ;;
    *)                             return 1 ;;
  esac
  [ -e "/sys/class/net/$1/device" ]   || return 1   # no backing device = virtual
  [ -e "/sys/class/net/$1/wireless" ] && return 1
  [ "$(cat "/sys/class/net/$1/type" 2>/dev/null)" = "1" ] || return 1  # ARPHRD_ETHER
  return 0
}

list_wired() {
  for path in /sys/class/net/*; do
    [ -e "$path" ] || continue
    ifn=${path##*/}
    is_wired "$ifn" && echo "$ifn"
  done
}

# Wait for at least one port to appear rather than assuming udev has settled.
waited=0
while [ -z "$(list_wired)" ] && [ "$waited" -lt "$WAIT_SECS" ]; do
  sleep 1
  waited=$((waited + 1))
done

found=0
NETWORKD_CHANGED=0
for ifn in $(list_wired); do
  # Subnet-id from the port's OWN trailing index, not from enumeration order, so
  # the mapping survives a renamed or absent sibling. Hex, because the subnet-id
  # is a hex group in the address (end0 -> 1 … end9 -> a).
  digits=${ifn##*[!0-9]}
  [ -n "$digits" ] || digits=0
  subnet=$(printf '%x' $((digits + 1)))
  addr="$PCAT_ULA_PREFIX:$subnet:$PCAT_ULA_IID/64"
  # Two halves, both needed: `replace` makes the address live RIGHT NOW (including
  # before any Ansible run has ever happened), the declaration makes it durable.
  if ip -6 addr replace "$addr" dev "$ifn" 2>/dev/null; then
    log "$ifn <- $addr"
    found=$((found + 1))
  else
    log "$ifn <- $addr FAILED"
  fi
  networkd_declare "$ifn" "$addr"
done

if [ "$NETWORKD_CHANGED" = "1" ] && command -v networkctl >/dev/null 2>&1; then
  # Only when a file actually changed — i.e. the first run on a box, or a prefix
  # change. MEASURED: `networkctl reload` does NOT merely re-read the files, it
  # reconfigures every link whose matching file changed. That is safe here and was
  # verified live: the DHCPv4 lease is renewed to the same address and the ULA is
  # already in the file being applied, so neither an SSH session over IPv4 nor one
  # over the ULA is lost. Skipping the reload would only defer the takeover to the
  # next boot, where the file is read from the start anyway.
  networkctl reload >/dev/null 2>&1 || true
fi

[ "$found" -gt 0 ] || log "no on-board Ethernet port found (waited ${waited}s)"
exit 0
