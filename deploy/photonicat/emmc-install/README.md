# Photonicat 2 (RK3576) — SD-installer → Armbian-on-eMMC → Ansible

Insert the **init-SD**, power the board, watch the LCD. The SD boots a small
installer that flashes a **pristine Armbian image** onto the **eMMC**, injects
the operator's **root SSH key**, verifies the write, and powers off. Pull the SD,
power on — the board boots Armbian from eMMC, which regenerates its identity and
grows the rootfs on its own. From there **Ansible owns everything else** (the
maintenance user, hostname, Context Pilot, and later Tailscale).

> **Status: built and validated end-to-end on real RK3576 hardware.** Flash →
> eMMC boot → root-key SSH → Ansible deploy (stable + nightly) all confirmed.

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
│   ├── init-sd-install.sh   runs ON the init-SD: detect eMMC → dd Armbian → verify → inject root key → poweroff
│   └── pcat-installer.service   systemd oneshot that runs it on the SD's boot
├── lcd/pcat_lcd.py          GC9307 progress painter (flashing % / verify / done / error; self-enables backlight)
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
ARMBIAN_IMG=/path/to/Armbian_photonicat2_trixie.img.xz \
CARD=/dev/sdX \
PUBKEY=/path/to/operator_root.pub \
sudo ./build/build-init-sd.sh
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
5. paints **`DONE`** and powers off — **power-off is the done-signal**.

Pull the SD, power on → Armbian boots from eMMC, regenerates identity, grows the
rootfs, and is reachable as `root` with the baked key.

### Stage 3 — Ansible takes over (`deploy/ansible`)

From a control node (root key already in place):

```bash
cd deploy/ansible
python3 -m venv .venv && ./.venv/bin/pip install ansible   # once
# put the box IP in inventory.ini, then:
./.venv/bin/ansible-playbook -i inventory.ini site.yml            # channel=stable by default
#   nightly:            -e channel=nightly
#   forced credentials: -e cp_admin_email=… -e cp_admin_password=…  (+ cp_superadmin_*)
```

Ansible owns all per-box + product config: `bringup` (hostname `dh-<serial>` +
maintenance user `dh` with the root key reused and NOPASSWD sudo), the
**signed release channel** fetch (`channels/stable.json|nightly.json`, sha256
verified), Context Pilot + Caddy, the admin seed, and the GC9307 display driver.

---

## Access after the box is on eMMC

- **LAN SSH** (key plane): `ssh root@<ip>` (or the `dh` user) with the operator
  key; the box is `dh-<serial>` with mDNS `dh-<serial>.local`.
- **Tailscale SSH** (identity plane): **not wired yet** — planned as an Ansible
  `bringup` step (reusable tagged key + `tailscale up --ssh`).
- Physical fallbacks: re-insert any bootable SD (SD boot-priority); UART console.

---

## Open items

1. **Tailscale enrol** in Ansible `bringup` (reusable tagged key, `--ssh`).
2. **minisign verification** of the channel manifest in `fetch.yml` — the pubkey
   is `UPDATE_PUBKEY` in the orchestrator; sha256 already covers the bootstrap.
3. **Archive + pin** the exact Armbian `.img` + SHA256 in our own storage.
