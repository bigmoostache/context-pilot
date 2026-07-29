#!/bin/sh
# cp-regdom — push the configured Wi-Fi regulatory country code into the kernel.
#
# WHY THIS EXISTS (design-network-uplink §9, landmine 1): out of the box the
# regulatory domain is the world default `00`, under which `iw phy phy0 info`
# reports EVERY 5 GHz channel — and 2.4 GHz channels 12/13 — as `(no IR)`, no
# initiating radiation. An AP cannot start on a `no IR` channel, so a country
# code is a functional prerequisite for the access point, not a nicety.
# Measured on phy0: `iw reg set FR` takes the `no IR` count from 89 to 0 and
# channel 36 from 20 dBm/unusable to 23 dBm/usable.
#
# Landmine 12: `iw reg get` flags both radios `(self-managed)`, which reads as
# "your country code will be ignored". It is not so — ath11k forwards the user
# hint to firmware and honours it. Do not re-derive the opposite from the flag.
#
# WHO CALLS THIS: cp-regdom.service at boot (before NetworkManager brings any
# radio up), and the backend applier on every AP apply — the country must be in
# force BEFORE `cp-ap` is activated, or the AP silently fails to beacon.
#
# NEVER FATAL: every failure path exits 0 with a journal line. A box that misses
# its regulatory domain still routes, still serves the cockpit, and still has a
# working ethernet uplink — only the AP is affected, and the applier refuses to
# enable it without a country anyway (FR-NET-14).
set -u

CONF=/etc/default/cp-network
# Default matches the orchestrator's agents dir under cp_root (HOME is
# {{ cp_root }}/home in context-pilot.service, and the registry appends
# .context-pilot/agents). Overridable through $CONF so a relocated cp_root or a
# test box does not need this script patched.
CP_NETWORK_STATE=/opt/context-pilot/home/.context-pilot/agents/.network.json

log() { echo "cp-regdom: $*" >&2; }

if [ -r "$CONF" ]; then
  # shellcheck source=/dev/null
  . "$CONF"
fi

command -v iw >/dev/null 2>&1 || { log "iw not installed — nothing to do"; exit 0; }

if [ ! -r "$CP_NETWORK_STATE" ]; then
  log "no state file at $CP_NETWORK_STATE — leaving the regulatory domain alone"
  exit 0
fi

# Deliberately sed, not jq: jq is not a dependency of this appliance and this
# runs at boot before anything optional is guaranteed present. The state file is
# written by serde_json::to_vec_pretty, so the country is always on its own line
# as `"country": "FR"`. A malformed or absent value simply yields the empty
# string and we no-op.
country=$(sed -n 's/.*"country"[[:space:]]*:[[:space:]]*"\([A-Za-z][A-Za-z]\)".*/\1/p' \
  "$CP_NETWORK_STATE" 2>/dev/null | head -1 | tr '[:lower:]' '[:upper:]')

if [ -z "$country" ]; then
  log "no country code configured — leaving the regulatory domain alone (5 GHz stays no-IR)"
  exit 0
fi

current=$(iw reg get 2>/dev/null | sed -n 's/^country \([A-Z][A-Z]\):.*/\1/p' | head -1)
if [ "$current" = "$country" ]; then
  log "regulatory domain already $country"
  exit 0
fi

if iw reg set "$country" 2>/dev/null; then
  log "regulatory domain set to $country (was ${current:-00})"
else
  log "iw reg set $country FAILED — 5 GHz may stay unusable"
fi
exit 0
