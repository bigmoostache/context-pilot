# Photonicat 2 — zero-touch Debian-to-eMMC install system

Insert the **init-SD**, power the board, walk away. A daemon flashes Debian onto
the **eMMC**, verifies it, shows progress on the front LCD, and powers off. Pull
the init-SD, insert the big **prod-SD** (storage), power on — the board now runs
Debian from eMMC, enrolled in Tailscale, with the operator SSH key installed.
Context Pilot is provisioned afterwards with one Ansible command from a control
node.

Everything here is **build/plan tooling** — no destructive step runs
automatically from this repo. You run the two `build/` scripts by hand once the
SD cards are reachable.

---

## Why this shape (the one big decision)

The reference recipe used Armbian + `armbian-install`, an **interactive TUI with
no unattended mode** — the blocker. This system drops it entirely and instead
**clones a known-good golden eMMC image and `dd`s it on**: deterministic,
offline, zero interaction.

The golden image is cloned from **asterix itself**, whose SD already boots Debian
13 correctly on this exact board — so the bootloader + rootfs are proven. Locked
decisions: **clone asterix Debian**, **back up the factory OpenWrt first**,
**control-node Ansible**, **LCD 4-state progress**, **reusable tagged Tailscale
key**, operator SSH key baked in, hostname **`pcat-<serial>`**.

---

## Verified board facts (probed live on asterix)

| Fact | Value | Consequence |
|------|-------|-------------|
| Board | Ariaboard Photonicat 2, RK3568 aarch64 | — |
| OS | Debian 13 trixie, kernel 6.18.28 | plain Debian, no `armbian-install` |
| Boots from | **SD** `mmcblk1p2`; eMMC `mmcblk0` = vendor OpenWrt (squashfs) | SD has boot priority → pull SD = boot eMMC |
| `boot.scr` | `part uuid mmc ${devnum}:2 uuid; root=PARTUUID=${uuid}` | **self-locating** — no fstab/PARTUUID rewrite on the clone |
| Rootfs used | 18 GB of 470 | golden image shrinks to ~20 GB (~4-6 GB zst) |
| Serial | device-tree `serial-number` = `21d5aefad944808f` | `pcat-<serial>` is fleet-unique |
| LCD | GC9307 172×320, spidev1.0 present, rot 180, X-offset 34 | painter drives it directly |
| resize tools | growpart / resize2fs / e2fsck / sgdisk present | firstboot can grow to 58 GB |

---

## Layout

```
emmc-install/
├── lcd/pcat_lcd.py                 GC9307 4-state progress painter (python spidev+gpiod)
├── firstboot/
│   ├── firstboot-emmc.sh           runs ON the eMMC, first boot: un-twin + grow + enrol
│   └── pcat-firstboot.service      systemd oneshot (self-disabling)
├── installer/
│   ├── init-sd-install.sh          runs ON the init-SD: flash eMMC + verify + poweroff
│   └── pcat-installer.service      systemd oneshot
├── build/
│   ├── build-golden-image.sh       clone asterix SD → shrink → seed → compress
│   └── build-init-sd.sh            write base Debian + seed installer payload
└── README.md                       this file
```

---

## The flow, end to end

### Stage 1 — build the golden eMMC image (once, on a Linux host / asterix)

`build/build-golden-image.sh` clones asterix's SD, shrinks the rootfs to
~20 GB, seeds it, and compresses it. It **bakes in**:

- `firstboot-emmc.sh` + its unit (enabled),
- the operator's SSH public key into `root/.ssh/authorized_keys`,
- the reusable tagged Tailscale key into `/boot/pcat-ts-authkey`.

```bash
SRC_DISK=/dev/mmcblk1 \
OUT=/mnt/scratch/pcat-golden \
PUBKEY=/path/to/operator_key.pub \
TS_AUTHKEY='tskey-auth-...tagged-reusable...' \
sudo ./build/build-golden-image.sh
```

Output: `golden.img.zst`, `.sha256`, `.img-size`.

### Stage 2 — build the init-SD (once per installer card)

`build/build-init-sd.sh` writes a base Debian image to the small card and seeds
the **installer** payload (`init-sd-install.sh`, `pcat_lcd.py`, the unit, and the
golden image). It also drops `.backup-openwrt` so the factory OpenWrt is backed
up before the wipe (D2).

```bash
BASE_IMG=/path/to/base-debian-pcat.img \
CARD=/dev/sdX \
GOLDEN_DIR=/mnt/scratch/pcat-golden \
sudo ./build/build-init-sd.sh
```

> The base image must have `python3`, `python3-spidev`, `python3-libgpiod` for
> the LCD progress screen (otherwise progress is silently skipped — the install
> still works, power-off is still the done-signal).

### Stage 3 — the field install (technician, zero-touch)

1. Insert **init-SD**, power on.
2. LCD shows `FLASHING %` → `VERIFYING` → `DONE — REMOVE SD`; board powers off.
3. Pull the init-SD, insert the **prod-SD**, power on.
4. Board boots eMMC Debian; `firstboot-emmc.sh` regenerates SSH host keys +
   machine-id, wipes the cloned Tailscale identity, grows the rootfs to 58 GB,
   sets hostname `pcat-<serial>`, and enrols into Tailscale with `--ssh`.

### Stage 4 — Context Pilot provisioning (control node, one command)

From your machine (SSH key already baked in, so no `ssh-copy-id`):

```bash
cd deploy/ansible
./.venv/bin/ansible-playbook -i 'pcat-<serial>,' site.yml \
  -e cp_install_display=true \
  -e cp_superadmin_email=... -e cp_superadmin_password=... \
  -e cp_admin_email=... -e cp_admin_password=...
```

---

## Frictions, pre-solved

1. **`armbian-install` interactivity** → eliminated (image-clone path).
2. **Identity twins** (SSH host keys, machine-id, Tailscale state) → wiped +
   regenerated in `firstboot-emmc.sh`, and pre-cleared at build time.
3. **Small image on a big eMMC** → `sgdisk -e` relocates the backup GPT, then
   `growpart` + `resize2fs` fill the 58 GB on first boot.
4. **`boot.scr` root selection** → self-locating; the clone needs no PARTUUID
   surgery. Confirmed from the live `boot.scr`.
5. **Verify before poweroff** → the installer sha256s the written region and
   only powers off on a match; a bad flash leaves the ERROR screen up.
6. **Re-flash loop** → stamp file on the init-SD + SD boot-priority means
   pulling the card is the intended stop; the stamp is belt-and-braces.
7. **Factory OpenWrt loss** → backed up to the init-SD before the wipe (D2).
8. **Tailscale key at rest** → reusable+tagged so one image serves many boxes;
   `firstboot` **shreds** it right after enrol so a pulled eMMC can't leak it.
9. **LCD is a dashboard, not a text device** → a dedicated GC9307 painter
   (`pcat_lcd.py`) transcribed from the vendor's exact init sequence.

## Open items before a real run

- Provide a **base Debian image** for the init-SD (or reuse a spare
  `golden.img`).
- Mint the **reusable tagged Tailscale auth key** (tag `tag:pcat`) and pass it as
  `TS_AUTHKEY`.
- Add `tag:pcat` to the tailnet ACL `tagOwners` (pending).
- The whole build needs a **Linux host that can see the SD cards** — the Mac Mini
  has no card reader; asterix is the natural build host.
