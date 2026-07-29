#!/bin/sh
# cp-regdom — push the configured Wi-Fi regulatory country code into the kernel.
#
# WHY THIS EXISTS: out of the box the regulatory domain is the world default
# `00`, under which `iw phy phy0 info` reports EVERY 5 GHz channel — and 2.4 GHz
# channels 12/13 — as `(no IR)`, no initiating radiation. An AP cannot start on a
# `no IR` channel, so a country code is a functional prerequisite for the access
# point, not a nicety. MEASURED on phy0 of this hardware: `iw reg set FR` takes
# the `no IR` count from 89 to 0, and channel 36 from 20 dBm/unusable to
# 23 dBm/usable. Those two numbers are the whole justification for this file.
#
# THE `(self-managed)` TRAP — do not re-derive the opposite from the flag.
# `iw reg get` flags BOTH radios `(self-managed)`, which reads as "the firmware
# owns the domain, your country code will be ignored". Read that way, this script
# looks pointless. It is not so: measured, ath11k (phy0) forwards the user hint to
# firmware and honours it — that is where the 89 → 0 above comes from. Self-managed
# means the phy may also have opinions of its own, not that yours are discarded.
# phy1 (aic8800) genuinely does stay at `00` forever, which is why the check below
# reads the `global` block and never "the first country line in the output".
#
# WHO CALLS THIS — two entry points, one implementation. There is exactly one
# place in this appliance that talks to `iw reg set`, and it is this file.
#
#   1. `cp-regdom` (no arguments) — cp-regdom.service at boot, before
#      NetworkManager brings any radio up. Nothing else is running that early, so
#      the country is read straight out of .network.json with `sed`.
#
#   2. `cp-regdom <CC>` — the backend applier on every AP apply, passing the
#      country as a single argument. The country must be in force BEFORE `cp-ap`
#      is activated or the AP silently fails to beacon, and at that moment the
#      applier already holds the value in memory: making it re-read the file it
#      just wrote would be a second source of truth for no gain. An argument, if
#      present, therefore WINS over the file and the file is not consulted.
#
# CLI CONTRACT: at most one argument, a two-letter ISO-3166 country code, case
# insensitive. Anything else — three letters, digits, an empty string — is a
# no-op with a journal line; garbage is never handed to `iw`. Extra arguments are
# ignored.
#
# NEVER FATAL: every path exits 0, including every failure path, each with a
# journal line. THE EXIT STATUS IS NOT A SUCCESS SIGNAL and no caller may gate on
# it: a box that misses its regulatory domain still routes, still serves the
# cockpit and still has a working ethernet uplink — only the AP is affected, and
# the applier refuses to enable the AP without a country in the first place, so a
# country that failed to take is the narrower of the two problems. An applier that
# failed an apply over this would trade a degraded AP for a rollback of the whole
# network document.
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

if [ "$#" -gt 0 ]; then
  # ── Entry point 2: the applier told us the country ────────────────────────
  # It arrives already validated by the backend (two alphabetic characters), and
  # it is re-checked here anyway: this script is on $PATH as root and the check
  # costs one `case`. Rejecting rather than sanitising is deliberate — a code we
  # had to repair is a code we do not understand, and `iw reg set` would take
  # something plausible-looking and leave the radio somewhere nobody intended.
  case "$1" in
    [[:alpha:]][[:alpha:]])
      country=$(printf '%s' "$1" | tr '[:lower:]' '[:upper:]')
      ;;
    *)
      log "ignoring '$1': not a two-letter country code — leaving the regulatory domain alone"
      exit 0
      ;;
  esac
else
  # ── Entry point 1: boot, read it out of the state file ────────────────────
  if [ ! -r "$CP_NETWORK_STATE" ]; then
    log "no state file at $CP_NETWORK_STATE — leaving the regulatory domain alone"
    exit 0
  fi

  # Deliberately sed, not jq: jq is not a dependency of this appliance and this
  # runs at boot before anything optional is guaranteed present.
  #
  # ANCHORED TO A WHOLE LINE, both ends. The state file is written by
  # `serde_json::to_vec_pretty`, which puts every key alone on its own line as
  # `  "country": "FR",` — that is the assumption this pattern pins, and it is
  # the reason anchoring is safe. Unanchored, `.*"country".*` matched the string
  # anywhere on any line, so a `"country"` appearing inside some other VALUE (an
  # SSID, an APN) would be picked up by `head -1` and silently win over the real
  # field. There is exactly one `"country"` key in the document today (ap.country);
  # if a second one is ever added, this becomes first-wins again and needs
  # narrowing to the `ap` object — hence the note.
  country=$(sed -n \
    's/^[[:space:]]*"country"[[:space:]]*:[[:space:]]*"\([A-Za-z][A-Za-z]\)"[[:space:]]*,\{0,1\}[[:space:]]*$/\1/p' \
    "$CP_NETWORK_STATE" 2>/dev/null | head -1 | tr '[:lower:]' '[:upper:]')

  if [ -z "$country" ]; then
    log "no country code configured — leaving the regulatory domain alone (5 GHz stays no-IR)"
    exit 0
  fi
fi

# Read the GLOBAL domain specifically, not the first `country` line in the file.
# `iw reg get` prints one block per regulatory authority — `global`, then one per
# self-managed phy — and on this hardware they legitimately disagree: measured,
# phy#0 (ath11k) adopts the hint while phy#1 (aic8800) stays at `00` forever. A
# naive "first country line" read picks whichever phy happens to be listed first
# and reports a domain we never set.
global_domain() {
  iw reg get 2>/dev/null | awk '
    /^global$/            { g = 1; next }
    g && /^country /      { c = $2; sub(/:.*/, "", c); print c; exit }'
}

before=$(global_domain)

# Issued unconditionally, even when the global domain already matches: `iw reg
# set` is a cheap netlink hint, and a self-managed phy that came up after the
# last call (firmware reload, a radio rfkill-cycled) would otherwise never
# receive it. The applier calls this immediately before bringing `cp-ap` up, and
# an AP that starts under a stale `00` does not beacon at all.
if ! iw reg set "$country" 2>/dev/null; then
  log "iw reg set $country FAILED — 5 GHz may stay unusable"
  exit 0
fi

if [ "$before" = "$country" ]; then
  log "regulatory domain already $country (hint re-issued)"
else
  log "regulatory domain set to $country (was ${before:-unknown})"
fi
exit 0
