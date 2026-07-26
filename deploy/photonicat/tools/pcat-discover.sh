#!/usr/bin/env bash
# pcat-discover.sh — list the Photonicat boxes reachable on THIS network segment,
# by their fleet ULA, and print inventory lines for them.
#
# You do not need this to reach a box whose serial you know: its address is
# <prefix>:1:<serial-as-4-hex-groups>, full stop. This is for the other case —
# "what is plugged into this switch?" — and for checking that a freshly-installed
# box actually came up.
#
# HOW IT WORKS: every IPv6 host answers the all-nodes multicast address ff02::1 on
# its link. Sourcing that ping from an address inside our own /64 makes the boxes
# reply from THEIR address in that /64 (same-prefix source selection), so the
# replies are self-filtering: anything in our prefix is one of ours. A /64 cannot
# be swept (2^64 addresses), so multicast is the only enumeration that works.
#
# Usage:
#   ./pcat-discover.sh [-i IFACE] [-p PREFIX] [-k SSH_KEY] [--inventory]
#     -i  interface facing the boxes (default: the one with the IPv6/IPv4 default route)
#     -p  fleet ULA /48 (default: the product constant below)
#     -k  ssh key used to identify boxes (default: ssh's own defaults)
#     --inventory  print ready-to-paste inventory.ini lines instead of a table
set -uo pipefail

PREFIX="${ULA_PREFIX:-fd59:ec78:2da4}"    # keep in sync with build-init-sd.sh / site.yml
SUBNET=1                                   # port 1 = the :1: /64
IFACE=""
SSH_KEY=""
INVENTORY=0

while [ $# -gt 0 ]; do
  case "$1" in
    -i) IFACE="${2:?}"; shift 2 ;;
    -p) PREFIX="${2:?}"; shift 2 ;;
    -k) SSH_KEY="${2:?}"; shift 2 ;;
    --inventory) INVENTORY=1; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

command -v ip >/dev/null 2>&1 || { echo "iproute2 required" >&2; exit 1; }

if [ -z "$IFACE" ]; then
  IFACE="$(ip -6 route show default 2>/dev/null | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1); exit}')"
  [ -n "$IFACE" ] || IFACE="$(ip -4 route show default 2>/dev/null | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1); exit}')"
fi
[ -n "$IFACE" ] || { echo "could not guess the interface — pass -i IFACE" >&2; exit 1; }

SRC="$PREFIX:$SUBNET::1"

# ── the control node needs an address in the boxes' /64 to talk to them ───────
# A ULA is on-link only: without a local address in the prefix there is no source
# address to reach it from, and the ping would fail with EINVAL.
if ! ip -6 addr show dev "$IFACE" 2>/dev/null | grep -qi "inet6 $SRC/"; then
  echo "adding $SRC/64 to $IFACE (control-node address in the fleet prefix)"
  if [ "$(id -u)" = "0" ]; then
    ip -6 addr add "$SRC/64" dev "$IFACE"
  else
    sudo ip -6 addr add "$SRC/64" dev "$IFACE"
  fi || { echo "could not add $SRC/64 — run as root or add it by hand" >&2; exit 1; }
fi

echo "probing $IFACE for boxes in $PREFIX:$SUBNET::/64 …" >&2
# -c3 rather than one shot: multicast replies are unordered and a box that just
# booted may miss the first solicitation.
FOUND="$(ping -6 -c3 -W2 -I "$SRC" ff02::1 2>/dev/null \
         | grep -oE "$PREFIX:$SUBNET:[0-9a-f:]+" | sort -u)"

if [ -z "$FOUND" ]; then
  echo "no box answered in $PREFIX:$SUBNET::/64." >&2
  echo "Fallback — every IPv6 host on this link, ours or not:" >&2
  ping -6 -c2 -W2 -I "$IFACE" ff02::1 2>/dev/null \
    | grep -oE 'fe80::[0-9a-f:]+' | sort -u | sed 's/^/  /' >&2
  echo "(a box with an unreadable serial gets no ULA — identify it by trying the root key)" >&2
  exit 1
fi

SSH_OPTS=(-o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=5)
[ -n "$SSH_KEY" ] && SSH_OPTS+=(-i "$SSH_KEY")

[ "$INVENTORY" = "0" ] && printf '%-40s %-22s %s\n' ADDRESS HOSTNAME SERIAL

for addr in $FOUND; do
  # The interface-id IS the serial, so we can name the box without touching it;
  # we still ask over SSH, because that also proves the operator key works.
  iid="${addr#"$PREFIX:$SUBNET:"}"
  serial="$(printf '%s' "$iid" | tr -d ':')"
  info="$(ssh "${SSH_OPTS[@]}" "root@$addr" \
            'hostname; tr -d "\0" </proc/device-tree/serial-number 2>/dev/null' 2>/dev/null)"
  host="$(printf '%s\n' "$info" | sed -n 1p)"
  live_serial="$(printf '%s\n' "$info" | sed -n 2p)"
  [ -n "$host" ] || host="(no ssh)"
  [ -n "$live_serial" ] && serial="$live_serial"
  if [ "$INVENTORY" = "1" ]; then
    printf '%s ansible_host=%s ansible_user=root\n' \
      "$([ "$host" = "(no ssh)" ] && echo "pcat-$serial" || echo "$host")" "$addr"
  else
    printf '%-40s %-22s %s\n' "$addr" "$host" "$serial"
  fi
done
