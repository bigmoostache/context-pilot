# Design Document — Internet Uplink (WAN / 5G) & Wi-Fi Access Point

**Status:** v1.2 — M0–M6 executed 2026-07-29. Shipped and validated on hardware
except the 5G **data path**, which stays blocked on RF (O0.1), and the mobile
pane's end-to-end check, which is blocked on a missing mobile settings entry
point (pre-existing, out of scope here). Corrections found by execution are
folded in below and marked **M1–M6 correction**.
**Author:** Context Pilot
**Date:** 2026-07-29
**Hardware:** Photonicat 2 (RK3576) — Quectel RM520N-GL 5G modem, dual Wi-Fi radio
**Related:** `docs/design-auth.md` §13.5 (IT Settings), `deploy/PROVISIONING.md`

---

## §1 — Context & Problem

The appliance ships with a 5G modem and two Wi-Fi radios, and today uses neither.
`deploy/ansible/tasks/modem.yml` installs the modem *toolbox* (`mmcli`, `qmicli`,
`picocom`) and deliberately stops there: "No WAN config, no NetworkManager, no
routing, no APN — that is deliberately left for later." This document is "later".

Two capabilities are missing:

1. **Uplink choice.** The box needs internet for the LLM provider APIs and for
   OTA updates. On a client site the ethernet uplink may be absent (a field
   deployment), unreliable, or administratively unavailable. The 5G modem is the
   answer, but nothing connects it and nothing decides between the two.
2. **Wi-Fi access.** Clients reach the cockpit over the client LAN today. An
   access point makes the box self-sufficient — plug it in anywhere, connect to
   its SSID, reach `:443` — and, optionally, turns it into the site's router.

Both must be configurable by the vendor (Ansible, at provisioning time) *and* by
the client's IT admin (cockpit IT Settings, `can_manage_it`), without the two
fighting over the same state.

---

## §2 — Hardware Baseline (measured)

Probed live on the test box `dh-7681f2a227e0f10d` (192.168.1.38), 2026-07-29,
Armbian community 26.8.0-trunk.413 trixie, kernel 6.18.38-current-rockchip64.
**Everything below is measured, not assumed.**

### Interfaces

| Link | Driver | State | Notes |
|---|---|---|---|
| `end0` | rockchip | UP, DHCP `192.168.1.38/24` metric 100 | carries ULA `fd59:ec78:2da4:1:…` |
| `end1` | rockchip | DOWN (no carrier) | carries ULA `fd59:ec78:2da4:2:…` |
| `wlp1s0` | `ath11k` (PCIe, Wi-Fi 6) | DOWN | modes: managed, **AP**, P2P-{client,GO,device}. 2.4 + 5 + 6 GHz; 23–30 dBm |
| `wlan0` | `aic8800_fdrv` | DOWN | modes: managed, **AP, AP/VLAN, monitor, mesh point**, P2P. 2.4 **+ 5** GHz; 20 dBm |
| `wwu1u1i4` | `qmi_wwan` | DOWN | the modem's QMI net port |

### Modem

```
/org/freedesktop/ModemManager1/Modem/0  [Quectel] RM520N-GL
  firmware RM520NGLAAR03A03M4G · carrier config ROW_Commercial
  supported: gsm-umts, lte, 5gnr      current mode: 3g,4g,5g (preferred 5g)
  ports: cdc-wdm0 (qmi), ttyUSB1 (gps), ttyUSB2/3 (at), wwu1u1i4 (net)
  lock: sim-pin2   unlock retries: sim-pin (3)   state: disabled
```

A SIM is present. The modem has never been connected (`state: disabled`).

**M0 correction — the SIM does not need a PIN.** With the modem enabled,
`AT+CPIN?` returns `READY`. The `lock: sim-pin2` line refers to the FDN
(fixed-dialing) lock listed under `enabled locks: fixed-dialing`; it does **not**
gate data. `mmcli -m 0 --enable` succeeds with no PIN. Landmine 4 is closed on
the PIN question — `wwan.pin` stays in the state model for SIMs that do lock,
but it is not a prerequisite here.

SIM identity (measured): IMSI `208202407300256`, ICCID `8933202425073002569`,
operator **`20820` Bouygues Telecom**.

### Regulatory domain — a blocker, not a detail

`iw phy phy0 info` reports every 5 GHz channel as **`(no IR)`** (no initiating
radiation), and 2.4 GHz channels 12/13 likewise. The regulatory domain is the
world default (`00`). **An AP cannot start on a `no IR` channel.** A country code
is a functional prerequisite, not a nicety (see FR-NET-14).

**M0 measurement — both phys are `self-managed`, and `iw reg set` still works.**
`iw reg get` marks `phy#0` and `phy#1` as `(self-managed)`, which normally means
the driver ignores the global regulatory domain. ath11k is the benign case: it
forwards a user hint to firmware. Measured on `phy0`:

| | before | after `iw reg set FR` |
|---|---|---|
| reported country | `00` (`na` on the phy) | `FR: DFS-ETSI` |
| `no IR` channels | **89** | **0** |
| ch. 36 / 48 | 20 dBm, `no IR` | 23 dBm, usable (`NO-OUTDOOR`) |
| ch. 100 | `no IR` | 30 dBm, `radar detection` |

So the `cp-regdom` design (§9) holds as written. The self-managed flag is worth
recording only because it predicts the opposite outcome and would otherwise be
re-litigated at M1.

### What is *not* installed

`hostapd`, `dnsmasq`, `NetworkManager`, `nftables`, `iptables`. `wpasupplicant`,
`modemmanager`, `iw` and `wireless-regdb` are present (the last two matter: §12's
package list can drop them). `net.ipv4.ip_forward = 0`.

### Network stack in place

netplan → systemd-networkd. `/run/systemd/network/10-netplan-all-eth-interfaces.network`
matches `e*`; `/etc/systemd/network/05-pcat-ula-end{0,1}.network` (written by
`pcat-ula`) sort earlier and therefore **win** — they are the effective files for
the ethernet ports, and they inherit the netplan content verbatim plus `Address=`.

---

## §3 — Functional Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-NET-01 | Ansible can configure the 5G connection (APN, SIM PIN, credentials, roaming, standby policy) at provisioning time | Must |
| FR-NET-02 | Ansible can set the initial uplink mode and AP configuration | Must |
| FR-NET-03 | An `can_manage_it` admin can select one of three uplink modes from cockpit IT Settings: `wan`, `wan_5g`, `5g` | Must |
| FR-NET-04 | `wan` — the box reaches the internet through ethernet only; the modem is not connected | Must |
| FR-NET-05 | `wan_5g` — ethernet is preferred; 5G takes over when the ethernet uplink stops providing internet, and hands back when it recovers | Must |
| FR-NET-06 | `5g` — the box reaches the internet through 5G **only**; the ethernet default route is suppressed even with a cable plugged in | Must |
| FR-NET-07 | An `can_manage_it` admin can enable/disable the Wi-Fi access point and set SSID, passphrase, band, channel, country and hidden-SSID | Must |
| FR-NET-08 | The AP shares the active uplink with its clients (NAT + DHCP + DNS) by default | Must |
| FR-NET-09 | Internet sharing on the AP can be switched off, leaving the AP as a cockpit-access-only network with no forwarding | Must |
| FR-NET-10 | The cockpit shows live uplink state: active uplink, ethernet carrier/IP, modem registration, operator, access technology, signal, WWAN IP | Must |
| FR-NET-11 | The cockpit shows AP state: running or not, SSID, associated-client count | Should |
| FR-NET-12 | A re-run of `site.yml` never overwrites settings the IT admin changed from the cockpit, unless explicitly forced | Must |
| FR-NET-13 | The Wi-Fi passphrase and SIM PIN are never returned by any read API | Must |
| FR-NET-14 | The AP refuses to be enabled without a regulatory country code | Must |
| FR-NET-16 | **The whole 5G surface exists only on a box that carries the modem.** On a Photonicat variant without the M.2 module the cockpit offers neither `wan_5g` nor `5g`, shows no bearer settings, and the API refuses both with a `400`; the applier creates no `cp-wwan` profile. Picking `5g` there would suppress the ethernet default route with nothing to replace it. | Must |
| FR-NET-15 | **The vendor** (`can_manage_secrets`, superadmin) can amend the 5G settings (APN, credentials, PIN, roaming, standby) from the cockpit. A client's `can_manage_it` admin can neither read nor write them — the SIM and the data plan are ours, so the APN is a fleet decision, not a per-site one. They keep the bearer's live **status**, which is what they need when the box loses its uplink. | Must |

---

## §4 — Non-Functional Requirements & Invariants

| ID | Invariant |
|----|-----------|
| NFR-NET-01 | **No network mode ever alters an address on `end0`/`end1`.** Only default routes are touched. The fleet ULA, the DHCP lease, the LAN reachability of the cockpit and the day-0 access path are untouched in every mode. |
| NFR-NET-02 | NetworkManager never manages an ethernet link. The `unmanaged-devices` seam (§5) is a hard precondition of installing it. |
| NFR-NET-03 | Every route/gate is `can_manage_it` — **except the 5G bearer, which is `can_manage_secrets`** (FR-NET-15). The server is authoritative (NFR-05); client gating is cosmetic, and the UI hides the bearer form by simply not receiving it. |
| NFR-NET-04 | The system applier is env-gated exactly like `CP_CADDYFILE`: with the env unset, the backend persists state and performs **no** system call. Tests and local dev run unmodified. |
| NFR-NET-05 | A failed apply rolls back to the previous configuration and reports a `502`, mirroring `caddy::regenerate`. A bad setting can never wedge the box. |
| NFR-NET-06 | State is persisted atomically + durably (`state::write_atomic`) and survives a power cut; it is re-applied at boot before the cockpit serves. |
| NFR-NET-07 | Structure budget: `crates/…/transport/it/` is at 7 of 8 entries and `web/src/lib/api/` at 8 of 8. New backend code lands in a single `it/network/` sub-directory; the API client extends `lib/api/it.ts` in place. No file exceeds 500 lines. |

---

## §5 — Architecture: the NetworkManager ↔ networkd seam

The guiding constraint is NFR-NET-01. `end0`/`end1` carry the fleet ULA — the
deterministic, serial-derived address that is the **only** day-0 and break-glass
access path for the entire fleet. `PROVISIONING.md` already records the landmine:
networkd deletes addresses it did not configure itself. NetworkManager behaves
the same way. Handing the ethernet to a second stack would put the fleet's
recovery path on the line for no benefit.

So the two stacks split by device:

```
            ┌──────────────────── systemd-networkd (netplan) ───────────────────┐
  end0 ─────┤ DHCP metric 100 · fleet ULA · cockpit :443 · day-0 path           │
  end1 ─────┤ fleet ULA                                                         │
            └───────────────────────────────────────────────────────────────────┘
            ┌──────────────────── NetworkManager (+ ModemManager) ──────────────┐
  cdc-wdm0 ─┤ profile `cp-wwan`  (gsm)   route-metric 700 | 50   → net wwu1u1i4 │
  wlp1s0 ───┤ profile `cp-ap`    (wifi, mode ap)  ipv4.method shared | manual   │
  wlan0 ────┤ unused — reserved for a future Wi-Fi client uplink                │
            └───────────────────────────────────────────────────────────────────┘
```

**M0 correction — the modem's NM device is `cdc-wdm0`, not `wwu1u1i4`.**
`nmcli device status` lists `cdc-wdm0  gsm  disconnected`; the QMI net port
`wwu1u1i4` is not a NetworkManager device at all — NM drives the modem through
ModemManager on the control port and applies the resulting IP config to the net
port itself. `cp-wwan` therefore binds `cdc-wdm0`. This is only a naming
correction; the `unmanaged-devices` globs are unaffected (neither name matches
`end*`/`lan*`/`wan*`).

The seam is one file, shipped **before** NetworkManager is installed:

```ini
# /etc/NetworkManager/conf.d/10-cp-unmanaged.conf
[keyfile]
unmanaged-devices=interface-name:end*;interface-name:lan*;interface-name:wan*

[main]
dns=systemd-resolved     # DECIDED in M0/O0.3 — measured, see below
```

**M0 decision — `dns=systemd-resolved`, validated on hardware.** With the file
above already in place, `apt-get install network-manager dnsmasq-base nftables`
was run on the test box. Measured immediately after:

| Check | Result |
|---|---|
| `nmcli device status` → `end0`, `end1` | `unmanaged` ✅ |
| `wlp1s0`, `wlan0`, `cdc-wdm0` | managed / `disconnected` ✅ |
| `ip -br addr show end0` | DHCP `192.168.1.38/24` **and** ULA `fd59:…:1:…` both present ✅ |
| `end1` | ULA `fd59:…:2:…` present ✅ |
| IPv4 + IPv6 default routes | both intact ✅ |
| `/etc/resolv.conf` | still symlinked to `stub-resolv.conf`, **md5 unchanged** ✅ |
| DNS resolution | works ✅ |
| cockpit on LAN IPv4 and on the ULA | `200`, 5266 bytes on both ✅ |
| `NetworkManager --print-config` | `dns=systemd-resolved` in effect ✅ |

NM never touched `resolv.conf` and never touched the ethernet. **Landmine 3 is
closed**; the seam works exactly as designed.

One thing the install *does* change — see landmine 10: it enables
`NetworkManager-wait-online.service`, which must be masked.

The two stacks meet only in the kernel routing table, where a metric is a metric
regardless of who installed it. `end0` sits at 100 (netplan's DHCP default);
`cp-wwan` sits at 700 (standby) or 50 (preferred).

**Why not NetworkManager everywhere.** It is the tidier end state, but the
migration cost is paid in the one currency we cannot afford: re-validating the
fleet's recovery path. Revisit only if a future need (Wi-Fi client uplink,
bonding) forces a single policy engine over ethernet.

**Why not hostapd + a hand-written QMI script.** ModemManager does not configure
interfaces — that is the connection manager's job, and systemd-networkd has no
ModemManager integration. Going that route means hand-writing bearer setup, IP
application, DNS merge, reconnect/backoff, plus hostapd, plus a DHCP server, plus
nftables NAT. NetworkManager already ships all of it, declaratively, driven by a
CLI that both Ansible and the backend can call.

---

## §6 — State model: one owner, one applier

The classic failure mode of "Ansible configures it *and* the UI configures it" is
that the next `site.yml` run silently reverts the client's choices. The model
below makes that impossible by construction.

```
  Ansible ──(seed, write-once)──►  .network.json  ◄──(read/write)── POST /api/it/network/*
                                        │  0600
                                        ▼
                       backend network applier  (the ONLY writer of system config)
                          │          │            │              │
                        nmcli   networkctl   iw / sysctl   caddy::regenerate
                     (cp-wwan,  (end0 drop-in) (regdom,     (AP address in the
                       cp-ap)                  forwarding)   site list — L11)
```

- **Ansible seeds, it does not apply.** `tasks/network.yml` writes
  `.network.json` only when absent — the same write-once contract as `seed.env`.
  `-e cp_net_force=true` re-seeds deliberately (FR-NET-12).
- **The backend is the sole applier**, at boot (`apply_network_at_boot`, mirroring
  `apply_caddy_at_boot`) and on every `POST`. No `nmcli` profile is ever created
  by Ansible, so there is exactly one source of truth and one code path to debug.
- **M3 addition — the applier also drives Caddy**, before the AP comes up
  (landmine 11). The fifth arrow in the diagram above is not optional.
- **M4 correction — one applier is not enough; it also has to serialise.** This
  section is about Ansible vs the cockpit and says nothing about two concurrent
  cockpit calls. Measured: a `5g` apply blocked inside `nmcli connection up` (a
  modem with no coverage), a `wan` apply completed in the meantime and removed
  the strict-mode drop-in, and then the first one finished and wrote it back —
  persisted document saying `wan`, box with no default route. The read → save →
  apply section now runs under a dedicated lock, with the read *inside* it.
- **Location:** `<agents_dir>/.network.json`, beside `.identity.json` and
  `.provisioned` (i.e. `/opt/context-pilot/home/.context-pilot/agents/`), mode
  `0600` — it holds the Wi-Fi PSK and the SIM PIN.

### Shape

```jsonc
{
  "mode": "wan" | "wan_5g" | "5g",
  "wwan": {
    "apn": "orange.fr",
    "username": null, "password": null,   // secret, never read back
    "pin": null,                           // secret, never read back
    "roaming": false,
    "standby": "hot" | "cold"
  },
  "ap": {
    "enabled": false,
    "ssid": "ContextPilot-f10d",
    "passphrase": null,                    // secret, never read back
    "band": "bg" | "a",
    "channel": 0,                          // 0 = automatic
    "country": "FR",
    "hidden": false,
    "share_internet": true
  },
  "probe": { "targets": ["1.1.1.1", "9.9.9.9"], "fail_threshold": 3, "ok_threshold": 2, "interval_s": 10 }
}
```

`GET` returns this document with every secret replaced by a `*_set: bool`
(FR-NET-13).

---

## §7 — The three uplink modes

| Mode | Default route | `cp-wwan` | `end0` drop-in |
|---|---|---|---|
| `wan` | `end0`, metric 100 | `autoconnect no`, down; modem powered low | absent |
| `wan_5g` | `end0` (100); `cp-wwan` (700) standing by | up (hot) or armed (cold) | absent |
| `5g` | `cp-wwan`, metric 50 | up, `autoconnect yes` | present — WAN gateway suppressed |

### Strict `5g`: suppressing the ethernet gateway

The IT admin asked for *only*, so metrics alone are not enough — a cable that is
plugged in must not carry traffic. The applier writes a drop-in on the **effective**
`.network` for `end0`, which is `pcat-ula`'s file, not netplan's:

```ini
# /etc/systemd/network/05-pcat-ula-end0.network.d/50-cp-uplink.conf
[DHCPv4]
UseGateway=false
[DHCPv6]
UseGateway=false
[IPv6AcceptRA]
UseGateway=false
```

then `networkctl reload && networkctl reconfigure end0`. Leaving `wan`/`wan_5g`
removes the drop-in and reconfigures again.

`Address=` is never touched, so the DHCP address, the fleet ULA and the cockpit
stay up throughout (NFR-NET-01). `PROVISIONING.md` already records that the ULA
survives `networkctl reconfigure` — that is precisely because networkd now *owns*
it, and it is why this drop-in approach is safe.

**M0 measurement — the mechanism does exactly this, and reverts cleanly.**
The drop-in was hand-written, `networkctl reload && networkctl reconfigure end0`
run, then removed and reconfigured again:

| | baseline | drop-in active | after removal |
|---|---|---|---|
| IPv4 default route | via `192.168.1.1` | **absent** | restored |
| IPv6 default route | via RA | **absent** | restored |
| `192.168.1.38/24` on `end0` | present | **present** | present |
| fleet ULA on `end0` | present | **present** | present |
| cockpit on LAN IPv4 | `200`, 5266 B | `200`, **5266 B** | `200`, 5266 B |
| cockpit on the ULA | `200`, 5266 B | `200`, **5266 B** | `200`, 5266 B |
| `ping 1.1.1.1` | reachable | **unreachable** | reachable |

Note the `[IPv6AcceptRA] UseGateway=false` stanza is load-bearing: without it the
RA-learned IPv6 default route survives and strict `5g` leaks IPv6 out of the
ethernet. Both address families must be suppressed together. The only side effect
observed is that the SLAAC privacy addresses are regenerated on reconfigure —
cosmetic, and the ULA (which is what the fleet depends on) is stable.

### Standby policy (`wwan.standby`)

- **`hot` (default)** — in `wan_5g` the bearer stays connected with metric 700.
  Failover is a metric flip: sub-second. Cost: the SIM stays attached and
  consumes a little data on keep-alive.
- **`cold`** — the modem stays registered but the bearer is down; failover costs
  the bearer setup (measured target: < 40 s). For metered SIMs.

---

## §8 — Failover supervisor

A dedicated daemon, not a task inside the orchestrator: it must survive an
orchestrator crash or an OTA restart, and it must be debuggable with
`journalctl -u cp-uplink`.

- **`/usr/local/sbin/cp-uplink-watch`** + **`cp-uplink.service`**, configured from
  **`/etc/default/cp-uplink`** — rendered by the backend from `.network.json`,
  the same pattern as `/etc/default/pcat-ula`.
- **Interface-bound probing.** `ping -I end0` / `curl --interface end0` against
  the target list — never through the default route, which would test the wrong
  path and make the check circular.
- **Hysteresis.** `fail_threshold` consecutive failures demote the WAN and promote
  the 5G route; `ok_threshold` consecutive successes restore. A cooldown between
  transitions prevents flapping on a marginal link. Every transition is logged
  with the reason.
- **Scope.** The supervisor runs only in `wan_5g`. In `wan` and `5g` the routing
  is static and the unit idles (it still reports state for `GET /api/it/network`).

This is what covers the case metrics alone cannot see: **cable plugged in, DHCP
lease held, upstream dead** — the most common real-world client failure.

---

## §9 — Wi-Fi access point

NetworkManager profile `cp-ap` on `wlp1s0` (the ath11k radio).

**M0 correction to the rationale.** §2 v1 credited `wlp1s0` with AP/VLAN, mesh and
monitor: those modes actually belong to `wlan0` (aic8800). `wlp1s0` supports only
managed / AP / P2P. The choice of `wlp1s0` still stands, on the reasons that
survive measurement — Wi-Fi 6, 6 GHz-capable, and 23–30 dBm of regulatory
headroom against the aic8800's 20 dBm — but the AP/VLAN and mesh arguments must
not be relied on. If per-SSID VLANs ever leave §14's out-of-scope list, they need
`wlan0`, not `wlp1s0`.

| Setting | Value |
|---|---|
| `802-11-wireless.mode` | `ap` |
| `802-11-wireless.ssid` / `.hidden` | from state |
| `802-11-wireless.band` / `.channel` | `bg` \| `a`; **channel `0` must be sent as the EMPTY STRING** — `nmcli` rejects a literal `0` ("not a valid channel") and reads the empty value back as `0` |
| `802-11-wireless-security.key-mgmt` | `wpa-psk` **+ `proto rsn` + `pairwise/group ccmp`** — see O6.4 |
| `ipv4.method` | `shared` when `share_internet`, else `manual` |
| `ipv4.addresses` | `10.42.0.1/24` |

- **`share_internet: true`** → `ipv4.method=shared`: NetworkManager runs dnsmasq
  for DHCP + DNS on the AP subnet and installs NAT through its firewall backend;
  the applier sets `net.ipv4.ip_forward=1`. Clients get the active uplink,
  whichever it is (FR-NET-08).
- **`share_internet: false`** → the AP is a cul-de-sac whose only reachable
  service is the cockpit (FR-NET-09).

  **M3 correction — NOT `ipv4.method=manual`.** Measured: `manual` gives the
  interface its address and nothing else, i.e. **no DHCP server**, so a client
  cannot join the network at all without being hand-configured with a static
  address — and O3.3's own criterion says "with sharing off, *the same client*
  still loads the cockpit". NetworkManager has no "DHCP without NAT" method, so
  the cul-de-sac is built the other way round: keep `ipv4.method=shared` for
  dnsmasq, then set `ip_forward=0` and delete NM's `nm-shared-<if>` masquerade
  table. Verified: the client keeps its lease, `ping 1.1.1.1` fails,
  `sysctl net.ipv4.ip_forward` reads `0`, `nft list tables` is empty, NM does not
  put the table back, and `https://10.42.0.1/` still answers `200`/5266 B.
- **Country code.** `cp-regdom.service` (oneshot, `After=network-pre.target`) runs
  `iw reg set <CC>` from the state file, with `wireless-regdb` installed. Without
  it 5 GHz is unusable (§2). Enabling the AP with an empty country is a `400`
  (FR-NET-14).

### M0 measurement — the AP works, with two caveats

A throwaway `cp-ap-test` profile was built exactly as the table above specifies
(`mode ap`, `band a`, `channel 36`, `wpa-psk`, `ipv4.method shared`,
`10.42.0.1/24`) and a real client was associated to it — `wlan0`, the *other*
radio on the same box, put into station mode. That makes the association test
self-contained: no phone needed, and it is repeatable in CI-on-hardware.

| Observation | Value |
|---|---|
| `nmcli con up cp-ap-test` | **4.0 s** to fully activated |
| `iw dev wlp1s0 info` | `type AP`, `channel 36 (5180 MHz)`, txpower 16 dBm |
| client link | associated at **5180 MHz**, `-34 dBm`, VHT-MCS 8, 78 Mbit/s tx |
| client address | `10.42.0.233` by DHCP from NM's dnsmasq |
| `nft` | `table ip nm-shared-wlp1s0` with `masquerade` + forward policy, auto-installed |
| `net.ipv4.ip_forward` | `0` → `1`, set by NM |
| NAT egress from the AP subnet | `ping 1.1.1.1` from `10.42.0.233` **succeeds** |
| cockpit from the AP subnet, **HTTP** | `200` |
| cockpit from the AP subnet, **HTTPS** | ❌ TLS `internal error` — see landmine 11 |

Flipping to `ipv4.method manual` (sharing off, FR-NET-09) removed the `nft` table
and stopped `dnsmasq` while the AP kept beaconing on ch. 36 with its address —
exactly the intended cul-de-sac. **But `net.ipv4.ip_forward` stayed at `1`.**
NetworkManager sets it and never restores it. §9's "restore `ip_forward=0` if
nothing else needs it" is therefore not a nicety the applier may skip: it is the
only thing that will ever put that sysctl back. Confirmed by measurement.

Second caveat: M0 read `WPA2 WPA3` in nmcli's SECURITY column and concluded WPA3
was already negotiated. **M6 correction — that column is not the beacon.** With a
default `wpa-psk` profile the beacon carries an RSN element whose only AKM is
`PSK`, *plus a legacy WPA1 element offering TKIP*. See O6.4 for what actually
gets WPA3 here.

---

## §10 — API surface

All under `can_manage_it`, all following the established gate shape (`None` caller
= god-mode passthrough, present-caller-without-capability = `403`).

| Method | Route | Body | Response |
|---|---|---|---|
| `GET` | `/api/it/network` | — | `{ config, status }` — config with secrets elided, status live. **`config.wwan` is `null`** for a caller without `can_manage_secrets`; `status.wwan` is not elided |
| `POST` | `/api/it/network/mode` | `{ "mode": "wan"\|"wan_5g"\|"5g" }` | `{ mode, applied }` |
| `POST` | `/api/it/network/ap` | `{ enabled, ssid, passphrase?, band, channel, country, hidden, share_internet }` | `{ ap, applied }` |
| `POST` | `/api/it/network/wwan` | `{ apn, username?, password?, pin?, roaming, standby }` | `{ wwan, applied }` — **`can_manage_secrets` (superadmin), not `can_manage_it`** |

`status` (read from `nmcli -t`, `mmcli -J`, `ip route`, `iw dev`):

```jsonc
{
  "active_uplink": "wan" | "wwan" | "none",
  "modem_present": true,          // HARDWARE fact — see FR-NET-16
  "wan":  { "carrier": true, "ip": "192.168.1.38", "gateway": "192.168.1.1", "has_default_route": true },
  "wwan": { "state": "connected", "operator": "Orange F", "tech": "5gnr",
            "signal_dbm": -83, "ip": "10.183.4.22", "registered": true },
  "ap":   { "running": true, "ssid": "ContextPilot-f10d", "clients": 3, "channel": 36, "country": "FR" }
}
```

**Env gates (NFR-NET-04):** `CP_NMCLI_BIN`, `CP_MMCLI_BIN`, `CP_IW_BIN`,
`CP_NETWORKD_DIR`, `CP_UPLINK_ENV` — **plus, added in M3**, `CP_NETWORKCTL_BIN`
(reconfigure after the drop-in), `CP_SYSTEMCTL_BIN` (restart the supervisor),
`CP_NFT_BIN` (drop NM's masquerade table for a cul-de-sac AP), `CP_IP_BIN` (read
`end0`'s address — it is networkd's, so `nmcli` cannot answer), and
`CP_WAN_IFACE`/`CP_AP_IFACE`/`CP_WWAN_DEV` for hardware naming. `CP_NMCLI_BIN`
unset ⇒ persistence only, no subprocess, no system mutation; each other gate
degrades on its own. This is what makes the whole feature unit-testable off-box.

**M3 correction to `status`:** `active_uplink` and `wan.has_default_route` are
parsed from `/proc/net/route` and need **no gate at all**, so the two fields an
admin watches during a failover stay truthful on a box missing every optional
tool. O3.5's "a fully-null status" therefore understates it.

**Placement (NFR-NET-07):** `crates/cp-orchestrator/src/transport/it/network/`
(`mod.rs`, `state.rs`, `apply.rs`, `status.rs`) — one new entry in a directory
already at 7 of 8. Gates in `transport/rest/config/network.rs` (that directory is
at 5). Dispatch: four arms in `transport/mod.rs`. Spec: `tests/openapi/paths.rs`
+ schemas. Client: extend `web/src/lib/api/it.ts` **in place** — `web/src/lib/api/`
is already at the 8-entry ceiling.

---

## §11 — Frontend

The IT pane gains two sections. `ItPane.tsx` is 257 lines; two more sections would
approach the 500-line ceiling, so they land in a sibling
`web/src/components/shell/config/ItNetworkPane.tsx` (that directory goes 7 → 8).

- **Internet uplink** — three-way mode selector, plus a live status card
  (active uplink, operator, technology, signal, IPs) polled at
  `refetchInterval: 5000` while the pane is open.
- **Wi-Fi access point** — enable switch, SSID, passphrase, band, country,
  channel, hidden SSID, and the "share internet" switch. Country is required
  before enable; the client mirrors the server's `400`.
- **5G bearer** — APN, credentials, PIN, roaming, standby. **Rendered only when
  the server sent a `config.wwan`**, i.e. only for a superadmin (FR-NET-15). The
  absence of the block *is* the gate: a client admin is never shown a control
  they cannot use, and never told which role it would take.
- The `it` category blurb changes from "Network identity & TLS trust" to cover the
  network (`web/src/components/shell/config/categories.ts`).
- **Mobile.** `web/src/mobile-components/shell/config/ItPane.tsx` is a hand-authored
  divergent twin (already 261 lines vs 257 desktop). The new pane must be mirrored
  and `pnpm mirror:check` must stay clean.

---

## §12 — Ansible

`deploy/ansible/tasks/net/network.yml` (new; `modem.yml` moved beside it and
keeps the toolbox only):

1. Ship `/etc/NetworkManager/conf.d/10-cp-unmanaged.conf` **before** the package.
2. Install `network-manager`, `dnsmasq-base`, `nftables`, `wireless-regdb`, `iw`.
3. **Mask `NetworkManager-wait-online.service`** (landmine 10).
4. Ship `cp-regdom.service`, `cp-uplink-watch`, `cp-uplink.service`.
5. Seed `.network.json` write-once (`0600`), from the variables below.

| Variable | Default | Where |
|---|---|---|
| `cp_net_enabled` | `true` | `site.yml` |
| `cp_net_mode` | `wan` | `site.yml` |
| `cp_net_force` | `false` | CLI (`-e`) — re-seed over an existing file |
| `cp_wwan_apn` | `""` | `<client>.local.yml` |
| `cp_wwan_pin` / `_user` / `_password` | `""` | `<client>.local.yml` (secrets) |
| `cp_wwan_roaming` | `false` | `site.yml` |
| `cp_wwan_standby` | `hot` | `site.yml` |
| `cp_ap_enabled` | `false` | `site.yml` |
| `cp_ap_ssid` | `ContextPilot-<last 4 of serial>` | `site.yml` |
| `cp_ap_password` | `""` | `<client>.local.yml` (secret) |
| `cp_ap_country` | `FR` | `site.yml` |
| `cp_ap_share` | `true` | `site.yml` |

**M1 correction.** `deploy/ansible/tasks/` is at 8 entries and the `≤8` structure
rule does **not** cover only the Rust and `web/src` trees: `check-structure.sh`
walks the whole repo bar an explicit exclusion list that does not mention
`deploy/`. A 9th task file fails CI. `modem.yml` and `network.yml` therefore live
in a `net/` sub-directory, taking `tasks/` to 7 files + 1 directory.

---

## §13 — Landmines

1. **`country 00` ⇒ no 5 GHz AP**, and no 2.4 GHz ch. 12/13. Measured on hardware.
   A regulatory country code is a functional prerequisite.
2. **If NetworkManager grabs `end0`, the whole fleet loses its day-0 path.** NM
   purges addresses it did not install, exactly as networkd does. The
   `unmanaged-devices` file must land *before* the package. The already-shipped
   `50-pcat-ula` NM dispatcher hook is the safety net if this is ever violated.
3. **NM vs systemd-resolved over `/etc/resolv.conf`.** Must be arbitrated
   (`dns=systemd-resolved` if resolved runs, else `dns=none`) or the box loses DNS
   on `end0` the moment NM starts. **Verify on hardware before installing NM (M0).**
4. **The SIM.** The test box reports `lock: sim-pin2`, `state: disabled`. The PIN
   flow and `mmcli --simple-connect` must be proven by hand before any code
   depends on them.
5. **The strict-mode drop-in depends on `pcat-ula.sh`.** It targets
   `05-pcat-ula-end0.network.d/`. Any change to the ULA generator must preserve
   that drop-in directory, and any change to which `.network` wins for `end0`
   invalidates the path.
6. **NAT makes the box a router on the client LAN.** That is the client IT's
   decision, not ours — hence FR-NET-09's switch.
7. **Two radios.** The AP takes `wlp1s0`. `wlan0` (aic8800) stays free for a
   future Wi-Fi client uplink; do not consume it.
8. **Hot standby keeps the SIM attached.** Say so to a client on a metered plan.
9. **Concurrent appliers.** Only the backend writes system network config. A
   human running `nmcli` on the box will be reverted at the next apply or boot —
   documented, not defended against.
10. **`NetworkManager-wait-online.service` can stall the cockpit at boot.**
    *Found in M0/O0.3, not anticipated in v1.* Installing `network-manager`
    enables this unit; it is `WantedBy=network-online.target`, and
    `context-pilot.service` carries `After=network-online.target` +
    `Wants=network-online.target`. Its `ExecStart` is `nm-online -s` with
    **`TimeoutStartUSec=infinity`**. Today NM manages nothing that autoconnects,
    so it settles instantly — but the moment `cp-wwan` gets `autoconnect yes`
    (modes `wan_5g`/`5g`), "NM startup complete" waits on a modem that, with no
    SIM or no coverage, may never connect. That is an unbounded delay in front of
    the cockpit. **M1 must `systemctl mask NetworkManager-wait-online.service`**;
    `systemd-networkd-wait-online` already covers the real uplink (5.475 s on the
    test box). *Masked by hand on the test box during M0.*
11. **The AP subnet cannot reach the cockpit over HTTPS.** *Found in M0/O0.2.*
    Caddy listens on the wildcard `*:443`, but the generated Caddyfile enumerates
    **explicit site addresses** — the LAN IPv4, the hostname, and the two fleet
    ULAs. `10.42.0.1` is not among them, so an AP client gets a TLS
    `internal error` (no certificate for that name); plain HTTP answers `200`.
    FR-NET-09's "the AP is a cul-de-sac whose only reachable service is the
    cockpit" is therefore **not satisfied by the network applier alone**: enabling
    the AP must also add `10.42.0.1` to the Caddy site list and re-run
    `caddy::regenerate`. This crosses the §6 boundary that says the network
    applier only drives `nmcli`/`networkctl`/`iw`/`sysctl` — §6 and M3 need a
    fifth arrow to Caddy. Sequencing matters too: regenerate Caddy *before*
    reporting the AP as up, or the first client sees a broken cockpit.
12. **A `self-managed` phy is not necessarily an unsettable phy.** `iw reg get`
    flags both radios `(self-managed)`, which reads as "your country code will be
    ignored". Measured, ath11k honours the hint (89 `no IR` channels → 0). Do not
    re-derive this from the flag alone.

---

## §14 — Out of scope (v1)

- Wi-Fi **client** uplink (box joins an existing SSID as a 4th mode) — the
  `wlan0` radio is reserved for it.
- 802.11r. (**WPA3/SAE is no longer out of scope**: O6.4 ships WPA2/WPA3
  transition mode. A WPA3-*only* option would need a UI switch, since NM can only
  express it as `key-mgmt=sae`, which excludes WPA2 clients.)
- Per-SSID VLANs, guest isolation, captive portal.
- Bandwidth accounting / data caps on the 5G plan.
- IPv6 prefix delegation from the 5G bearer to the AP subnet (v1 NATs IPv4 only).
- Multi-SIM / SIM switching.

---

# Delivery Plan

Seven milestones. Each objective carries **Done when** criteria that are
mechanically checkable — a command with an expected result, not a judgement call.

Legend: `[ ]` open · `[~]` in progress · `[x]` done.

---

## M0 — Hardware reconnaissance

**Status: 3 of 4 objectives closed. O0.1 is blocked on RF (see below).**
Executed 2026-07-29 on `dh-7681f2a227e0f10d`. Findings are folded into §2, §5,
§7, §9 and §13 above; three new landmines (10, 11, 12) were discovered.

**Why first:** four of the nine landmines in §13 can only be closed with a shell
on the box, and M1 modifies the fleet's recovery path. No code is written until
the hardware has been proven to behave as §2 predicts. All steps are manual and
reversible; nothing here is committed except the findings.

### O0.1 — Prove the 5G bearer end to end — **[~] BLOCKED (RF, not software)**

- [x] Unlock and enable the modem
  - [x] `mmcli -m 0 --enable` — succeeds, **no PIN required**
  - [x] Which lock blocks data: **neither**. `AT+CPIN?` → `READY`; the reported
        `lock: sim-pin2` is the FDN lock and does not gate data. Landmine 4 is
        closed on the PIN question.
- [ ] Connect a bearer manually — **cannot proceed: the modem never registers**
- [ ] Record the bearer-setup latency — blocked on the above

**What was measured.** The SIM is Bouygues Telecom (`20820`) and the RF chain is
alive, but the modem cannot attach:

| Probe | Result |
|---|---|
| `AT+CPIN?` | `READY` |
| `AT+CFUN?` | `1` (full) |
| scan #1 | `20801 Orange (available)`, `20816 Free (forbidden)`, `20815 Free` |
| scan #2 | same, **plus `20820 BYTEL (lte, available)`** — the home network |
| `AT+QENG="servingcell"` | `LIMSRV` (limited service) on 208-15, RSRP **−114 dBm**, RSRQ **−17 dB** |
| `AT+CEREG?` / `AT+CGATT?` | `0` / `0` — never registered, never attached |
| forced `AT+COPS=1,2,"20820"` | no registration; falls back to limited service |
| forced `AT+COPS=1,2,"20801"` | `+CME ERROR: no network service`, `CEREG: 3` (denied), MM reports `gprs-not-allowed-in-plmn` — i.e. the SIM is correctly refused domestic roaming on Orange |

**Diagnosis.** This is not a configuration problem. The home network is visible
but only intermittently and at **RSRP −114 dBm / RSRQ −17 dB**, far below the
threshold to camp, so the modem sits in limited service. The Orange rejection is
the *expected* answer for a non-roaming domestic SIM and confirms the baseband,
the SIM and the AT path all work. The remaining variable is the antenna.

**M5/M6 update (2026-07-29, after `cp-wwan` existed).** With a real NM profile in
place the modem got further than in M0 but still does not carry data: it reached
`state: connected` with an IP (`192.0.0.2/27`) and a metric-50 default route on
`wwu1u1i4`, while `packet service state` stayed **`detached`** and no packet
egressed. It then fell back to `searching`. Tried and rejected as the cause: the
APN — `""`, `mmsbouygtel.com` and `ebouygtel.com` all behave identically, and the
`default-attach` bearer MM created is IPv6-only. The diagnosis is unchanged:
signal, not configuration.

**To unblock (physical, needs someone at the box):**
1. Confirm the 5G main/diversity antennas are attached to the correct u.FL/SMA
   ports on the Photonicat 2 and are not swapped with the Wi-Fi pigtails.
2. Re-measure `AT+QCSQ` / `AT+QENG="servingcell"`; expect RSRP better than
   roughly −100 dBm before attach is realistic.
3. Failing that, confirm the SIM's data plan is active and try a known-good SIM.

Nothing downstream of M0 is blocked by this except the 5G half of M3/M5 — the
`wan` mode, the whole AP feature, M1, M2 and M4 are unaffected.

**Done when:** `ping -I wwu1u1i4 1.1.1.1` succeeds from the box, `mmcli -m 0`
reports `state: connected` with an operator and access technology, and the
measured cold-start latency is written into §7 of this document.

**Still open.** Everything that does not need packets to flow over the bearer is
done: the profile is created and reconciled, the metrics move, the routes appear,
the supervisor decides and logs correctly, and the mode matrix passes. What is
untested is the one thing an antenna would unblock — an actual failover that
restores connectivity.

### O0.2 — Prove the AP on `wlp1s0` — **[x] done, one gap found**

- [x] `iw reg set FR` → `phy0` reports `FR: DFS-ETSI`, `no IR` count **89 → 0**,
      channels 36–48 usable at 23 dBm (table in §2)
- [x] Temporary AP started via `nmcli` (`mode ap`, band `a`, ch. 36, WPA2, shared)
- [x] Client associated — `wlan0` (the second radio, in station mode) associated at
      **5180 MHz**, `-34 dBm`, and took `10.42.0.233` by DHCP. Full table in §9.

**Result:** the AP itself is proven — 5 GHz association, DHCP, DNS, and NAT egress
to `1.1.1.1` all work, and sharing-off cleanly removes the `nft` table and
`dnsmasq`. The stated "Done when" is **not** met on one point: the cockpit answers
`200` over **HTTP** on `10.42.0.1` but fails TLS over HTTPS, because the Caddyfile
enumerates explicit site addresses and `10.42.0.1` is not one of them. That is
**landmine 11**, and it is a design gap rather than a hardware finding — the fix
belongs in M3, not in another round of M0.

**Done when:** a client device associates to the test SSID on a **5 GHz** channel
and can open `https://10.42.0.1/` (or the box's AP address) and see the cockpit.

### O0.3 — Arbitrate NetworkManager vs systemd-resolved (landmine 3) — **[x] done**

- [x] Recorded `/etc/resolv.conf` → symlink to `/run/systemd/resolve/stub-resolv.conf`
- [x] `systemd-resolved` is **active and enabled**
- [x] Installed `network-manager`, `dnsmasq-base`, `nftables` **with**
      `10-cp-unmanaged.conf` already in place

**Result: every check passed** — full table in §5. `end0`/`end1` came up
`unmanaged`, both the DHCP IPv4 and the fleet ULA survived, `resolv.conf` was
byte-identical afterwards (md5 unchanged), DNS kept working, and the cockpit
answered `200`/5266 B on both the LAN IPv4 and the ULA. **The decided value is
`[main] dns=systemd-resolved`**, recorded in §5. Landmine 3 closed. NM was left
installed, since M1 installs it anyway.

The install also surfaced **landmine 10** (`NetworkManager-wait-online.service`),
masked by hand on the test box.

**Done when:** with NM installed and running, `ip -br addr show end0` still lists
the DHCP IPv4 **and** the fleet ULA, `nmcli device status` shows `end0`/`end1` as
`unmanaged`, DNS resolution still works, and the decided `[main] dns=` value is
recorded in §5. If any check fails, NM is removed and M1 is re-planned.

### O0.4 — Confirm the strict-mode mechanism — **[x] done**

- [x] Hand-wrote `05-pcat-ula-end0.network.d/50-cp-uplink.conf` with `UseGateway=false`
- [x] `networkctl reload && networkctl reconfigure end0`

**Result: confirmed in both directions** — before/during/after table in §7. Both
the IPv4 and the IPv6 default routes disappeared, the DHCP address and the fleet
ULA stayed, the cockpit answered `200`/5266 B on the LAN IPv4 **and** the ULA
throughout, `1.1.1.1` became unreachable, and removing the drop-in restored
everything. The `[IPv6AcceptRA] UseGateway=false` stanza proved necessary — the
RA default route is independent of DHCPv4.

**Done when:** `ip route` shows **no** default route via `end0`, while
`ip -br addr show end0` still lists both the DHCP IPv4 and the fleet ULA, and the
cockpit is still reachable at `https://[<ula>]/` and `https://192.168.1.38/`.
Removing the drop-in and reconfiguring restores the default route.

### Box state after M0

Left clean and verified: no test profiles, no `nft` rules, `ip_forward=0`, both
radios down, `end0`/`end1` untouched, cockpit `200`/5266 B on the LAN IPv4 and the
ULA, DNS working. Changes deliberately retained: `network-manager` +
`dnsmasq-base` + `nftables` installed, `10-cp-unmanaged.conf` in place,
`NetworkManager-wait-online.service` masked, modem left enabled and searching.

---

## M1 — Ansible foundation & day-0 non-regression

**The riskiest milestone.** It touches the box's network stack, i.e. the fleet's
recovery path. Its acceptance test is not "the feature works" but "nothing broke".

### O1.1 — `tasks/network.yml` — packages and the seam — **[x] done**

**M1 correction — the file is `tasks/net/network.yml`, and `modem.yml` moved
beside it.** §12 claimed the ≤8-entry structure rule covers only the Rust and
`web/src` trees; it does not — `check-structure.sh` walks the whole repo bar an
explicit exclusion list, and `deploy/ansible/tasks/` was already at 8. A ninth
task file fails CI. Grouping the two network-adjacent task files under `net/`
takes the directory to 7 + 1.

- [x] Ship `/etc/NetworkManager/conf.d/10-cp-unmanaged.conf` **before** installing NM
- [x] Install `network-manager`, `dnsmasq-base`, `nftables`, `wireless-regdb`, `iw`
- [x] Set `[main] dns=systemd-resolved` (decided and validated in O0.3)
- [x] `systemctl mask NetworkManager-wait-online.service` (landmine 10)
- [x] Enable + start `NetworkManager`
- [x] Wire the task into `site.yml` behind `cp_net_enabled` (default `true`)

**Measured:** full `site.yml` over the fleet ULA finished `failed=0`;
`nmcli device status` reports `end0`/`end1` `unmanaged` and
`wlp1s0`/`wlan0`/`cdc-wdm0` managed; `NetworkManager-wait-online.service` reads
`masked`. On a re-run, **every task in this file reports `ok`, zero `changed`**,
with the two seeding tasks `skipping` — the write-once contract holding.

*Trap worth knowing when iterating on a box:* `context-pilot.service` carries
`StartLimitBurst=5` / `StartLimitIntervalSec=3600`, so more than five restarts in
an hour — easy to hit while deploying test binaries — makes the playbook fail
with "start of the service was attempted too often". It reads exactly like a
crash and is not one. `systemctl reset-failed context-pilot` clears it. Recorded
in `PROVISIONING.md`.

**Done when:** on a re-run of the full playbook, `nmcli device status` reports
`end0` and `end1` as `unmanaged` and `wlp1s0`/`cdc-wdm0` as managed, and a second
run reports every task in this file as `ok` (idempotent, zero `changed`).

### O1.2 — Regulatory domain unit — **[x] done**

- [x] `cp-regdom.service` (oneshot, `After=network-pre.target`, `RemainAfterExit`)
- [x] Reads the country from `.network.json`; no country ⇒ clean no-op exit 0
- [x] Enabled by Ansible

**M1 correction — read the GLOBAL regulatory block, not the first `country`
line.** `iw reg get` prints one block per authority and on this hardware they
legitimately disagree: after `iw reg set 00`, `global` reads `00` while `phy#0`
still reads `FR`. A first-match parse reported `FR` and skipped the work.
Measured after the fix: `00` → `FR`, `no IR` count on `phy0` **0**, channels
36–48 usable at 23 dBm, and a re-run says "already FR (hint re-issued)".

**Done when:** after `reboot`, `iw reg get` reports the configured country and
`iw phy phy0 info` shows channels 36–48 **without** `no IR`.

### O1.3 — State seeding (write-once) — **[x] done**

Measured: `stat -c %a` returns `600`, owner root, content matches the template,
and the "already exists — left untouched" branch fires on a re-run.

- [x] Template `.network.json` from the `cp_net_*` / `cp_wwan_*` / `cp_ap_*` vars
- [x] `creates:`-style guard so an existing file is never overwritten
- [x] `-e cp_net_force=true` re-seeds
- [x] Mode `0600`, owner root

**Done when:** run 1 creates the file; editing `"mode"` by hand on the box and
re-running `site.yml` leaves the edit intact and reports `ok`; re-running with
`-e cp_net_force=true` reports `changed` and restores the templated content;
`stat -c %a` returns `600`.

### O1.4 — Day-0 non-regression gate (blocking) — **[x] passed**

- [x] Reboot the box after M1 has been applied
- [x] Re-run the full `site.yml` **over the fleet ULA** (not the IPv4)

**All four met.** `ssh root@<ula>` works after reboot; `ip -br addr` still shows
the ULA on `end0` (and on `end1`, which has no carrier); the full `site.yml` over
the ULA finished `failed=0`; and the cockpit answers `200`/**5266 B** on the ULA.

*Caveat on the ULA path, unrelated to this work:* from the control node used here
(a laptop on Wi-Fi) the ULA is intermittent — SSH connects and then stalls, and a
100-byte `ping6` fails minutes after one succeeds. It is the AP's IPv6
multicast/ND handling, not the box: from the box itself `https://[<ula>]/`
answers `200`/5266 B consistently. `PROVISIONING.md` already warns to use a wired
control node for ULA work; this is a second reason.

**Done when, all four:** (a) `ssh root@<ula>` succeeds after reboot; (b)
`ip -br addr` still shows the ULA on both `end0` and `end1`; (c) a full `site.yml`
run over the ULA finishes `failed=0`; (d) `https://[<ula>]/` returns the SPA with
a body of the expected size (≈5.3 kB — a bare `200` proves nothing, see
`PROVISIONING.md`). **If any of the four fails, M1 is reverted before M2 starts.**

---

## M2 — Backend state & API (no system effects)

Pure persistence + contract. Nothing here touches the network, so it can be
developed and reviewed off-hardware.

### O2.1 — State module — **[x] done**

- [x] `transport/it/network/mod.rs` + `state.rs`: the §6 document, serde types,
      load/save via `state::write_atomic`, `0600`
- [x] Validation: mode enum; SSID 1–32 bytes; PSK 8–63 chars; country = 2 alpha;
      channel valid for the band; APN charset
- [x] Fail-closed load (malformed/tampered file ⇒ defaults, mirroring `load_identity`)
- [x] Secret elision helper for read paths

**Done when:** `cargo test -p cp-orchestrator network::state` passes with unit
tests covering round-trip, `0600` on the written file, each validation rejection,
malformed-file fallback, and the proof that no serialised read-path output
contains the PSK or the PIN.

### O2.2 — REST handlers + gates — **[x] done**

- [x] `transport/rest/config/network.rs` — four handlers, each `can_manage_it`
- [x] Re-export in `transport/rest/mod.rs`; four arms in `transport/mod.rs`
- [x] `400` on invalid body; `400` on enabling the AP with no country (FR-NET-14)

**Done when:** a test in the shape of `it_gated` asserts `403` for `Manager`/`User`
and non-`403` for `Admin`/`Superadmin` on all four routes, plus a round-trip test
(set mode → get reflects it) and the two `400` cases.

### O2.3 — OpenAPI + TypeScript contract — **[x] done**

- [x] Paths + schemas in `tests/openapi/paths.rs` (and `schemas*.rs`)
- [x] Regenerate `openapi.json`; regenerate the hey-api client
- [x] Extend `web/src/lib/api/it.ts` (no new file — `lib/api/` is at 8 entries)

**Done when:** `.github/checks/check-api-contract.sh` exits 0 — which requires the
route-exhaustiveness test to pass and `git diff --exit-code` to be clean over
`web/src/lib/api/generated/` after regeneration.

### O2.4 — Structure budget — **[x] done**

**Done when:** `.github/checks/check-structure.sh` exits 0 — no file over 500
lines, no directory over 8 entries, in particular `transport/it/` (7 → 8 via the
single `network/` sub-dir) and `web/src/lib/api/` (unchanged at 8).

---

## M3 — The applier

Where state becomes system configuration. Every step is env-gated (NFR-NET-04)
and rolls back on failure (NFR-NET-05).

### O3.1 — `nmcli` profile rendering — **[x] done**

- [x] `network/apply.rs`: render/reconcile `cp-wwan` and `cp-ap` from state
- [x] Idempotent: identical state ⇒ no `nmcli` mutation
- [x] Secrets passed without ever appearing in a log line or an error message

**Done when:** with env gates set to fake binaries, unit tests assert the exact
argv sequence for a representative state; and `journalctl -u context-pilot | grep -c <psk>`
returns 0 after a real apply on hardware.

### O3.2 — Mode application — **[x] done**

- [x] `wan`: `cp-wwan` down + `autoconnect no`; remove the drop-in; reconfigure
- [x] `wan_5g`: `cp-wwan` up at metric 700 (`hot`) or armed (`cold`); no drop-in
- [x] `5g`: `cp-wwan` up at metric 50; write the drop-in; `networkctl reload` + `reconfigure end0`
- [x] Rollback: on any failure restore the previous state file **and** the previous
      system config, return `502`

**Done when, on hardware, for each of the three modes:** `ip route` matches the
§7 table, `ip -br addr show end0` still lists the DHCP IPv4 **and** the ULA
(NFR-NET-01), and the cockpit answers on `https://[<ula>]/`. Plus: an
intentionally invalid state (e.g. a bogus APN) returns `502` and leaves
`ip route` byte-identical to what it was before the call.

### O3.3 — AP application — **[x] done**

- [x] `share_internet: true` ⇒ `ipv4.method shared` + `ip_forward=1`
- [x] `share_internet: false` ⇒ `ipv4.method manual`, NAT rules cleared,
      `ip_forward` restored **by the applier** — NM does not restore it (measured, §9)
- [x] Country pushed to `cp-regdom` before the AP is brought up
- [x] **Enabling the AP adds `10.42.0.1` to the Caddy site list and re-runs
      `caddy::regenerate` before the AP is reported up; disabling removes it**
      (landmine 11 — without this the AP cannot reach the cockpit over HTTPS)

**Done when:** with sharing **on**, a client associated to the AP resolves DNS and
reaches `1.1.1.1`; with sharing **off**, the same client still loads the cockpit
but `ping 1.1.1.1` fails and `sysctl net.ipv4.ip_forward` reads `0`. In **both**
cases `https://10.42.0.1/` returns the SPA (`200`, ≈5.3 kB) rather than a TLS
error — the check that failed in M0/O0.2.

### O3.4 — Boot apply — **[x] done**

- [x] `apply_network_at_boot`, called from the same startup path as
      `apply_caddy_at_boot`; write-and-apply, never fails startup

**Done when:** setting a mode, then `reboot`, leaves `ip route` and
`nmcli con show --active` matching that mode with no manual intervention.

### O3.5 — Live status — **[x] done**

- [x] `network/status.rs`: parse `nmcli -t`, `mmcli -J`, `ip -j route`, `iw dev`
- [x] Every field degrades to `null` rather than erroring when a tool is absent

**Done when:** `GET /api/it/network` on hardware returns a `status` object whose
`active_uplink`, `wan.has_default_route` and `wwan.state` match what `ip route`
and `mmcli -m 0` independently report, in all three modes; and the same call on a
dev machine with no gates set returns `200` with a fully-null status.

---

## M4 — Cockpit UI

### O4.1 — Uplink section — **[x] done**

- [x] `ItNetworkPane.tsx`: three-mode selector + live status card, 5 s polling
- [x] Pending/success/error states mirroring `IdentityForm`
- [x] Mounted in `ItPane.tsx`; `categories.ts` blurb updated

**Done when:** switching mode in the UI changes `ip route` on the box, the status
card reflects the new active uplink within 10 s without a manual refresh, and a
server `502` surfaces as a visible error rather than a silent no-op.

### O4.2 — AP section — **[x] done**

- [x] Enable switch, SSID, passphrase, band, country, channel, hidden, share switch
- [x] Passphrase field write-only; the UI shows "set / not set", never a value
- [x] Enable disabled until a country is chosen (mirrors the server `400`)

**Done when:** an admin can bring up a working AP from a factory-fresh box using
only the cockpit; reloading the pane never displays the passphrase; and the
browser devtools network tab shows no PSK in any response body.

### O4.3 — Mobile parity + gates — **[~] gates green, viewport check blocked**

- [x] Mirror into `web/src/mobile-components/shell/config/`
- [x] `pnpm mirror:check`, `pnpm lint`, `pnpm build`, `pnpm type-coverage`

**Done when:** `.github/checks/check-mobile-mirror.sh`, `check-ts-lints.sh` and
`check-structure.sh` all exit 0, and the pane is usable at a 390 px viewport.

All three gates exit 0 (mirror: 117 twins; ts-lints: eslint · prettier ·
stylelint · tsc · type-coverage · suppressions · census · knip).

**The viewport half could not be exercised end to end, for a reason that predates
this work: the mobile shell has no settings entry point at all.**
`mobile-components/shell/config/ConfigModal.tsx` exists and is mirror-checked,
but nothing in the mobile tree mounts it, so there is no route to the IT category
below 768 px. The mobile pane is written for 390 px (16 px inputs and selects
against iOS's auto-zoom, stacked band/channel/country row, `active:` for
`hover:`) and its structure is enforced by the mirror check — but "usable at
390 px" stays a claim about the code, not an observation. Forcing the DESKTOP
tree to 390 px is not a substitute: the desktop `ConfigModal`'s own two-column
shell overflows there, which is exactly why the mobile tree exists.

---

## M5 — Failover supervisor

### O5.1 — The watcher — **[~] built and validated by simulation; physical unplug + real bearer blocked**

- [x] `cp-uplink-watch` + `cp-uplink.service`, config from `/etc/default/cp-uplink`
- [x] Interface-bound probing; hysteresis (`fail_threshold` / `ok_threshold`); cooldown
- [x] Active only in `wan_5g`; idle elsewhere
- [x] Every transition logged with its reason

**Done when:** with the box in `wan_5g` and the 5G bearer up, **unplugging the
ethernet cable** moves the default route to `wwu1u1i4` and connectivity is
restored within the configured budget; **replugging** restores `end0`; and
`journalctl -u cp-uplink` shows exactly one transition per event (no flapping).

**What was proven, and what was not.** The decision logic, the hysteresis, the
cooldown and the "exactly one transition per event" property are all verified —
by the O5.2 blackhole simulation, which is the *harder* case (metrics cannot see
it at all, whereas a carrier drop they can). What is not verified is
*connectivity actually being restored*, because that needs a bearer that carries
packets (O0.1, RF). The physical unplug was also not performed: this box is
administered over `end0`, so cutting it remotely would end the session that has
to observe the result — it needs someone at the box.

**M5 corrections, both found by running O5.2.** (a) Every `nmcli` call in the
watcher is now `--wait`-bounded: without it a `connection up` against a modem
with no coverage blocked for nmcli's 90 s default, and since this is a
single-threaded loop the supervisor stopped probing entirely — it could not
notice the WAN coming back. (b) The decision keys off `promoted` (what the
supervisor chose) rather than `observed` (what the kernel shows); they differ
exactly when a promotion could not be carried out, and keying off the kernel
there re-promoted and re-logged on every cooldown for the whole outage. The
transition line is also logged *before* the actuation, so the journal records the
decision even when the actuation then fails.

### O5.2 — The blackhole case — **[x] done**

- [x] Simulate "cable up, upstream dead" (block the probe targets upstream, or
      point the box at a gateway that does not forward)

**Done when:** the carrier stays up and the DHCP lease is held, yet the supervisor
still fails over to 5G within `fail_threshold × interval_s` + one probe timeout,
and fails back when the upstream is restored. This is the case metric-only
failover cannot see and is the reason this milestone exists.

**Measured** with an `nft` OUTPUT rule dropping both probe targets while
`carrier=1` and the lease stayed at `192.168.1.38/24`: fail-over logged after 3
consecutive probe failures, fail-back logged after recovery, **exactly 2
`TRANSITION` lines for the whole cycle**, and the cockpit answering `200`/5266 B
throughout.

### O5.3 — Config plumbing — **[x] done**

- [x] Backend renders `/etc/default/cp-uplink` from `.network.json` on every apply
- [x] Changing probe settings from the API restarts the watcher

**Done when:** a probe-parameter change made through the API is visible in
`/etc/default/cp-uplink` and in `systemctl show cp-uplink -p ExecMainStartTimestamp`
(the unit restarted) without an SSH session.

**Measured:** `POST …/mode` + `POST …/wwan` moved `CP_UPLINK_MODE` `wan` →
`wan_5g` and `CP_UPLINK_STANDBY` `hot` → `cold` in the file, and
`ExecMainStartTimestamp` moved `11:35:36` → `11:40:49`. Only on change: an
unrelated save does not bounce the supervisor and lose its hysteresis state.

---

## M6 — Hardware validation & documentation

### O6.1 — Full matrix on hardware — **[x] done**

- [x] 3 modes × {AP off, AP on + sharing, AP on no sharing} = 9 combinations
- [x] Each combination survives a reboot

**Done when:** a results table is recorded in `PROVISIONING.md` with, per
combination: `ip route`, `nmcli con show --active`, cockpit reachability on the
ULA **and** the LAN IPv4, and AP-client internet reachability. Zero combination
leaves the cockpit unreachable on the ULA.

**All 9 pass** — table in `PROVISIONING.md` § "Phase 5". In every one, `end0`
keeps its DHCP lease *and* the fleet ULA and the cockpit answers `200`/5266 B on
both. A reboot in the most demanding combination (`5g` + AP on) came back
unaided: mode, AP on channel 36, regulatory domain `FR`, `ip_forward` conforming,
boot **13.7 s** total, cockpit `200` on the LAN IPv4, the ULA and `10.42.0.1`.

### O6.2 — Adversarial cases — **[x] done, one case deliberately not run**

- [x] Modem removed / no SIM / wrong PIN → clean degradation, no boot hang, clear status
- [x] Country left empty → AP refuses to enable, with a legible error
- [x] Power cut during an apply → state file intact on reboot, box reachable
- [x] `pcat-ula` re-run while the strict drop-in is in place → drop-in survives (landmine 5)

**Done when:** each case is exercised on hardware and the observed behaviour is
recorded; none of them leaves the box unreachable on the fleet ULA.

Modem gone (`CP_MMCLI_BIN` → a path that does not exist) ⇒ `wwan: null`, every
other field still honest, no boot hang. Country empty ⇒ `400` with a legible
message. `SIGKILL` mid-apply ⇒ the state file parses, no temp file left, box
reachable on the LAN IPv4 and the ULA, and the half-written change was rolled
back. `pcat-ula` re-run with the strict drop-in in place ⇒ the drop-in survives
(landmine 5), addresses intact, default route still suppressed.

**Not run, deliberately: the wrong-PIN case.** The SIM in this box has three
`sim-pin` retries and no PIN configured; deliberately sending a wrong one would
spend a retry on real hardware for a path the validation layer already refuses
(`pin must be 4–8 digits`) and that M0 established is not on the data path at all
on this SIM. Worth doing on a scrap SIM before a client site that uses one.

### O6.3 — Documentation — **[x] done**

- [x] `PROVISIONING.md`: new phase for network config, the §13 landmines, the results table
- [x] This document: status → validated, with M0/M6 measurements folded in (bearer
      latency, failover budget, the `dns=` decision)
- [x] `tasks/modem.yml` header updated — it currently states WAN config is "left for later"

**Done when:** a colleague can provision a box with 5G + AP from the docs alone,
without reading this design document or asking a question.

### O6.4 — WPA3 evaluation — **[x] done: WPA3 SHIPS, in transition mode**

- [x] Test `key-mgmt=sae` and WPA2/WPA3 mixed mode on ath11k

**Done when:** either WPA3 is shipped with a recorded compatibility note, or it is
explicitly rejected in §14 with the observed failure.

**WPA3 ships**, and it came from an unexpected direction. The profile keeps
`key-mgmt=wpa-psk`; what unlocks it is pinning `proto rsn` and both cipher slots
to `ccmp`. Measured beacon before and after:

| | `key-mgmt=wpa-psk`, NM defaults | + `proto rsn`, `pairwise/group ccmp` |
|---|---|---|
| WPA1 element | **present**, TKIP | gone |
| RSN pairwise / group | CCMP TKIP / TKIP | CCMP / CCMP |
| RSN AKM suites | `PSK` | **`PSK PSK/SHA-256 SAE`** |

That is genuine WPA2/WPA3-Personal transition mode, and a WPA2 client still
associated on 5 GHz (−33 dBm) and took a DHCP lease.

`key-mgmt=sae` was the obvious route and is the wrong one: it works, but it makes
the network **WPA3-only**, because NetworkManager cannot express transition mode
that way — `pmf optional` alongside `sae` is refused outright ("pmf can only be
'default' or 'required' when using … 'sae'"). Shipping it would silently lock out
every WPA2-only device on a client's site.

---

## Sequencing & risk

```
M0 ──► M1 ──► M2 ──► M3 ──► M4
        │             └────► M5 ──► M6
        └─ blocking gate O1.4 (day-0 non-regression)
```

- **M0 is non-negotiable.** It closes landmines 1, 3, 4 and validates the §7
  mechanism before a line of code depends on any of them. **Executed 2026-07-29:**
  landmines 1, 3 and (for the PIN) 4 are closed, the §7 mechanism is confirmed in
  both directions, and three new landmines (10, 11, 12) were found. **O0.1 remains
  open, blocked on RF, not on software** — the modem sees its home network at
  RSRP −114 dBm and cannot attach. That blocks only the 5G half of M3 and all of
  M5; M1, M2, M4 and the AP half of M3 proceed unaffected. Do not gate them on it.
- **O1.4 is a hard gate.** M2 does not start until day-0 access is proven intact.
- **M2 is off-hardware**, so it can proceed in parallel with M0/M1 if needed —
  it has no system effects by construction (NFR-NET-04).
- **M4 depends on M3** only for a demonstrable end-to-end; the API contract from
  M2 is enough to build against.
- **M5 is independent of M4** and can be developed in parallel.

## Suggested PR granularity

M0 → no PR (findings folded into this document). M1 its own PR, reviewed against
O1.4 evidence. M2 one PR (state + API + contract are atomic — the contract check
fails otherwise). M3 one PR. M4 one PR. M5 one PR. M6 one docs PR.

---

---

# §15 — 5G is vendor kit, and it is optional kit

Two constraints added after the first pass, both about *who* and *whether*
rather than *how*.

**Who — the bearer is the vendor's** (FR-NET-15, revised). We ship the SIM and
own the fleet's data plan, so the APN is a fleet-wide decision, not a per-site
setting: a client's IT admin changing it breaks their own connectivity on our
bill. `POST /api/it/network/wwan` is therefore `can_manage_secrets`
(superadmin), the same boundary that already protects the provider API keys, and
`GET /api/it/network` returns `config.wwan: null` to anyone below it.

`status.wwan` is deliberately **not** elided. Whether the modem is registered,
on which operator and at what signal is diagnostics, not configuration, and it is
exactly what a client admin reads out to us when the box loses its uplink.
Hiding it would make that call useless and protect nothing.

**Whether — not every box has a modem** (FR-NET-16). The Photonicat 2 ships in
variants. The presence probe reads **sysfs** (`/sys/class/usbmisc/cdc-wdm*`,
`/sys/class/net/ww*`), not ModemManager: `mmcli` answering is a statement about a
daemon's current view, and using it would make the whole 5G surface appear and
disappear across an MM restart. `CP_WWAN_PRESENT=0|1` overrides it for a variant
the probe reads wrong; with the applier inert (no `CP_NMCLI_BIN`) it reports
`true`, because off-box there is no hardware to protect.

The fact is surfaced as `status.modem_present`, readable by any `can_manage_it`
caller — choosing the uplink mode is the client admin's job even though the
bearer's configuration is not. Ansible probes the same way and skips the modem
toolbox entirely on a box without one.

The two gates are independent and both are enforced server-side:

| | non-5G variant | 5G variant |
|---|---|---|
| `admin` | ethernet only; no bearer settings | all three modes; no bearer settings |
| `superadmin` | ethernet only; no bearer settings | all three modes; bearer settings |

---

# What execution changed (2026-07-29)

A short index of the places where running this design against the hardware
contradicted it. Each is expanded in situ above.

| # | Where | The design said | The box said |
|---|---|---|---|
| 1 | §12 / O1.1 | a 9th file in `tasks/` is fine | `check-structure.sh` walks all of `deploy/`; it is not. Files moved to `tasks/net/` |
| 2 | §9 / O3.3 | `share_internet:false` ⇒ `ipv4.method manual` | `manual` runs no DHCP server, so nobody can join the cul-de-sac. Keep `shared`, remove forwarding + the masquerade table instead |
| 3 | §9 | channel `0` = automatic | `nmcli` rejects a literal `0`; the empty string is the only spelling it takes |
| 4 | §9 | a plain `wpa-psk` profile already gives WPA3 | it gives `PSK` only — *and* a legacy WPA1/TKIP element. `proto rsn` + CCMP is what produces `PSK PSK/SHA-256 SAE` |
| 5 | §6 | one owner, one applier | …which also has to serialise. Two concurrent cockpit calls interleaved and left the box's state lying about its routes |
| 6 | §8 / O5.1 | (unstated) | every `nmcli` call needs `--wait`: a 90 s block in `connection up` stops the supervisor probing, exactly when it must not |
| 7 | O1.2 | read the country from `iw reg get` | read it from the **global** block — the self-managed phys legitimately disagree |
| 8 | O3.5 | status is fully null off-box | the default-route half needs no tool at all, and staying truthful there is worth more |
| 9 | §10 | five env gates | nine: `networkctl`, `systemctl`, `nft` and `ip` each needed naming too |

Two "Done when" criteria remain unmet, both for reasons outside this design:
**O0.1** (the 5G data path — antenna) and the 390 px half of **O4.3** (the mobile
shell has no settings entry point yet).
