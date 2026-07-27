# Photonicat 2 (RK3576) — SD-installer → Armbian-on-eMMC → Ansible

Insert the **init-SD**, power the board, watch the LCD. The SD boots a small
installer that flashes a **pristine Armbian image** onto the **eMMC**, injects
the operator's **root SSH key**, imposes a **deterministic IPv6 address derived
from the hardware serial**, verifies the write, and paints `DONE` **with that
address**. Note it, press the **power button**, pull the SD, power on — the board
boots Armbian from eMMC, which regenerates its identity and grows the rootfs on
its own, and answers at an address you already know. From there **Ansible owns
everything else** (the maintenance user, hostname, Context Pilot, and later
Tailscale).

> **Status: built and validated end-to-end on real RK3576 hardware.** Flash →
> eMMC boot → root-key SSH → Ansible deploy (stable + nightly) all confirmed.
>
> **The fleet ULA is validated too** (2026-07-26, serial `7681f2a227e0f10d`):
> present on both ports at the FIRST boot, equal to the address predicted before
> the board had ever run, and two full `site.yml` runs (stable + nightly) driven
> entirely over IPv6. The stack turned out to be **netplan → systemd-networkd**
> (no NetworkManager), and it **drops foreign addresses on reconfigure** — hence
> the declarative half described below. Survives reboot and
> `networkctl reconfigure`.

---

## Why this shape

`armbian-install` (the interactive on-device TUI) was once framed as the blocker.
That was a false dichotomy: Rockchip images carry `idbloader`+U-Boot in the
sectors *before* the GPT, so a plain **`dd` of the image boots the eMMC** — no
installer, no interaction (`docs/debian2-flash-protocol.md`, Path C). So we take
the third option: **`dd` a reproducible, pristine base image, then let a
declarative tool (Ansible) do the config.**

What we keep: an **offline, deterministic, verified** field flash (`dd` + sha256).

What we dropped (and why): the previous system cloned a *golden image* from a
live, hand-built box — a **pet**: not reproducible, and it baked in accidental
state. It also carried ~250 lines of offline block-surgery (loop-mount, shrink,
GPT rebuild) that existed **only** because it cloned a full disk. A correctly
sized base image that **auto-expands on first boot** makes all of it vanish, and
**Armbian's own first-run** regenerates SSH host keys + machine-id and resizes
the rootfs for free — so no bespoke firstboot script is needed.

---

## Board facts (probed live on RK3576 Armbian)

| Fact | Value |
|------|-------|
| Board / SoC | Ariaboard **Photonicat 2, RK3576** aarch64 (`/proc/device-tree/compatible` = `ariaboard,photonicat2` + `rockchip,rk3576`) |
| Boot priority | **SD** first; pull SD ⇒ boots eMMC |
| eMMC discriminator | `/sys/block/mmcblkN/device/type` = `MMC` (SD reports `SD`). **`removable` is useless — both report `0`.** Never hardcode `mmcblk0`. |
| Serial → hostname | device-tree `serial-number` → Ansible sets `dh-<serial>` |
| Serial → address | the same 16-hex serial IS the ULA interface-id (`7681f2a227e0f10d` → `…:1:7681:f2a2:27e0:f10d`) |
| eMMC size | ~58 GB; the base image auto-expands to fill it |
| eMMC boot after `dd` | **works** — Rockchip loader lives pre-GPT, so a raw `dd` boots (validated) |
| LCD | GC9307 172×320 on `/dev/spidev1.0`; **RST=gpio122, DC=gpio121 on gpiochip3** (RK bank math `chip=gpio//32`); MADCTL `0x48`, X-offset 34 |
| LCD backlight | `/sys/class/backlight/backlight` — Armbian leaves it **blanked** (`bl_power=4`); the painter unblanks it itself (the vendor image kept it lit via its dashboard, Armbian does not) |

---

## Layout

```
emmc-install/
├── build/
│   ├── build-init-sd.sh     assemble the init-SD (flash Armbian to the card + seed payload)
│   └── fetch-lcd-deps.sh    (re)download the arm64 LCD .deb into deps/ (pinned)
├── deps/                    python3-spidev + python3-libgpiod (arm64, offline LCD deps)
├── installer/
│   ├── init-sd-install.sh   runs ON the init-SD: detect eMMC → dd Armbian → verify → inject root key + ULA → poweroff
│   └── pcat-installer.service   systemd oneshot that runs it on the SD's boot
├── ula/                     the fleet-ULA kit, copied onto the eMMC rootfs
│   ├── pcat-ula.sh              assigns <prefix>:<port>:<serial> to each on-board Ethernet port
│   ├── pcat-ula.service         runs it at boot
│   └── nm-dispatcher-50-pcat-ula  re-asserts it after NetworkManager reconfigures a link
├── lcd/pcat_lcd.py          GC9307 progress painter (flashing % / verify / done+address / error; self-enables backlight)
└── README.md
```

---

## The flow, end to end

### Stage 0 — get the base image (once, reproducible)

Download the Armbian **photonicat2** image, verify its SHA256, and **archive it
in your own storage**. Build from that copy, never a live CDN link.

### Stage 1 — build the init-SD (once per card)

`build/build-init-sd.sh` writes the Armbian image to the SD (making it bootable),
grows its rootfs, and seeds the payload: a copy of the image, its
decompressed-sha256 + size, the operator **root** pubkey, the installer + its
oneshot unit, the LCD painter, and the LCD `.deb`s. Runs on a Linux host — even
x86, since the LCD deps ride as pre-fetched `.deb` (no cross-arch chroot needed).

```bash
# variables AFTER sudo: sudo's env_reset would otherwise drop them
sudo ARMBIAN_IMG=/path/to/Armbian_photonicat2_trixie.img.xz \
     CARD=/dev/sdX \
     PUBKEY=/path/to/operator_root.pub \
     ./build/build-init-sd.sh
#   ULA_PREFIX=fd..:..:.. overrides the fleet /48 (default = the product
#   constant; see "The fleet ULA" below — do NOT vary it per card)
#   AUTO_POWEROFF=1       board powers itself off when done (unattended flashing).
#   Default is manual: the board stays on so the LCD keeps showing the address.
```

The installer auto-detects the payload's compression by **magic bytes** (not the
filename), so an image whose `.xz` extension was stripped still flashes correctly.

### Stage 2 — the field install (zero-touch)

Insert the init-SD, power on. The oneshot installer:
1. detects the eMMC by `device/type` (must differ from the booted SD);
2. `dd`s the pristine Armbian image onto it, painting **`FLASHING %`** on the LCD;
3. **verifies** the written region against the recorded sha256 (`VERIFYING`);
4. mounts the eMMC rootfs once and injects **only** the root SSH key
   (`authorized_keys` + `PermitRootLogin prohibit-password`), and removes
   Armbian's interactive first-login gate (`/root/.not_logged_in_yet`);
5. **imposes the fleet ULA** — the `ula/` kit plus this board's interface-id,
   read from its own device-tree serial (same source Ansible uses for the
   hostname, so name and address never disagree);
6. paints **`DONE` + the interface-id** and stops, **leaving the board on** —
   **the DONE screen is the done-signal**.

The board is left running on purpose: an automatic power-off would blank the panel
before anyone could read the address. Nothing is written past the verify, so it can
sit there indefinitely. Operator sequence: **read the address → press the power
button → board dark → pull the SD** (while the board runs, that card is its rootfs).
For unattended fleet flashing, `AUTO_POWEROFF=1` at build time restores
"board dark = done" — see Stage 1.

Then power on → Armbian boots from eMMC, regenerates identity, grows the rootfs,
and is reachable as `root` with the baked key **at an address you already knew**.

### Stage 3 — Ansible takes over (`deploy/ansible`)

From a control node (root key already in place):

```bash
cd deploy/ansible
python3 -m venv .venv && ./.venv/bin/pip install ansible   # once
# put the box's ULA (or its DHCP IP) in inventory.ini, then:
./.venv/bin/ansible-playbook -i inventory.ini site.yml            # channel=stable by default
#   nightly:            -e channel=nightly
#   forced credentials: -e cp_admin_email=… -e cp_admin_password=…  (+ cp_superadmin_*)
```

Ansible owns all per-box + product config: `bringup` (hostname `dh-<serial>` +
maintenance user `dh` with the root key reused and NOPASSWD sudo), the
**signed release channel** fetch (`channels/stable.json|nightly.json`, sha256
verified), Context Pilot + Caddy, the admin seed, and the GC9307 display driver.

---

## The fleet ULA — knowing a box's address before you meet it

A freshly-installed box has no name of ours and takes whatever IPv4 the client's
DHCP hands out, so the operator used to have to *hunt* for it (router lease table,
ping sweep) before Ansible could connect. A **ULA** (Unique Local Address,
RFC 4193 — IPv6's answer to `192.168.x.x`) removes the hunt, because an IPv6
address needs neither DHCP, nor a router, nor mDNS, nor anything from the client's
network in order to exist on the link.

```
fd59:ec78:2da4 : 1    : 7681:f2a2:27e0:f10d
└─ fleet /48 ─┘  └port┘ └─ the hardware serial, verbatim ─┘
```

| Field | Bits | Value |
|-------|------|-------|
| `fd` | 8 | RFC 4193 puts locally-assigned ULAs in `fc00::/7` **with the L bit set** ⇒ always `fd` |
| global ID | 40 | **drawn once, a product constant** — random, so it cannot collide with the client's addressing |
| subnet ID | 16 | the **Ethernet port**: `end0` → `:1:`, `end1` → `:2:` |
| interface ID | 64 | the 16-hex-char device-tree serial, split into 4 groups |

Two consequences worth spelling out. **The address is derivable from the label** —
`7681f2a227e0f10d` → `…:1:7681:f2a2:27e0:f10d` — so the LCD readout is a
convenience, not a dependency: `inventory.ini` can be filled before the box ships.
And **one /64 per port** is not decoration: the same address on both ports would
trip duplicate address detection the moment a technician patches them into the
same switch.

The prefix lives in exactly two places, which **must stay in sync**: `ULA_PREFIX`
in `build/build-init-sd.sh` and `box_ula_prefix` in `deploy/ansible/site.yml`.
Never regenerate it per box or per build — boxes and the control node must share a
prefix to talk to each other, and every inventory already written against the old
one would go dark.

On the box it is three files, installed by the flasher and re-asserted by Ansible:
`/usr/local/sbin/pcat-ula` (the assigner), `pcat-ula.service` (at boot), and
`/etc/NetworkManager/dispatcher.d/50-pcat-ula`.

The assigner does **two** complementary things, because neither alone is enough:

1. **`ip -6 addr replace`** — makes the address live immediately, on any stack,
   including on a box Ansible has never touched. This is what makes first contact
   possible at all.
2. **A declarative `.network` per port** — `/etc/systemd/network/05-pcat-ula-<if>.network`,
   so systemd-networkd *owns* the address. Measured on systemd 257: `networkctl
   reconfigure` **deletes** an address networkd did not configure itself, so a DHCP
   renew or a carrier bounce would silently take the ULA away. (`ManageForeignAddresses`
   does not exist in 257 — it is parsed and ignored.) Only the first matching
   `.network` applies and the distro's netplan file matches `e*` in one go, so a
   drop-in could not carry a *per-port* address: we generate one file per interface,
   sorted ahead of the distro's, inheriting its content **verbatim** (DHCP, RA,
   route metric) with `Address=` appended. Regenerated at every boot, rewritten only
   when the content actually changes, and the source file is tracked by sha256 so the
   inherited settings never drift.

The NetworkManager dispatcher hook is the same idea for the NM case (NM likewise
drops addresses it did not add). It is inert on this image — kept as a safety net
should Armbian switch stacks.

---

## Access after the box is on eMMC

- **ULA SSH** (deterministic plane): `ssh root@fd59:ec78:2da4:1:<serial-as-groups>`.
  One-time on the control node, per interface facing the boxes:
  `sudo ip -6 addr add fd59:ec78:2da4:1::1/64 dev <iface>`.
- **LAN SSH** (whatever DHCP gave it): `ssh root@<ip>` with the operator key;
  after Ansible the box is `dh-<serial>` with mDNS `dh-<serial>.local`.
- **Tailscale SSH** (identity plane): **not wired yet** — planned as an Ansible
  `bringup` step (reusable tagged key + `tailscale up --ssh`).
- Physical fallbacks: re-insert any bootable SD (SD boot-priority); UART console.

To answer "what is plugged into this switch?", `../tools/pcat-discover.sh` pings
the all-nodes multicast address from inside our prefix and lists the boxes that
reply (`--inventory` emits paste-ready inventory lines). A `/64` holds 2⁶⁴
addresses and cannot be swept, so multicast is the only enumeration available.

**Scope, stated plainly:** a ULA is reachable only from the **same L2 segment**.
This is a day-0 / on-site break-glass plane, *not* remote access — fleet-wide
reach from our side stays Tailscale's job.

---

## Open items

1. **Tailscale enrol** in Ansible `bringup` (reusable tagged key, `--ssh`).
2. **minisign verification** of the channel manifest in `fetch.yml` — the pubkey
   is `UPDATE_PUBKEY` in the orchestrator; sha256 already covers the bootstrap.
3. **Archive + pin** the exact Armbian `.img` + SHA256 in our own storage.
