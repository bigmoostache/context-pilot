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
| eMMC discriminator | `/sys/block/mmcblkN/device/type` = `MMC` (SD reports `SD`); `mmcblk0boot0` sibling | **`removable` is useless — both report `0`.** Detect eMMC by `device/type`, never by `removable` or a hardcoded `mmcblk0` |
| `boot.scr` | `part uuid mmc ${devnum}:2 uuid; root=PARTUUID=${uuid}` | **self-locating** — no fstab/PARTUUID rewrite on the clone |
| Rootfs used | 18 GB of 470 | golden image shrinks to ~20 GB (~4-6 GB zst) |
| Serial | device-tree `serial-number` = `21d5aefad944808f` | `pcat-<serial>` is fleet-unique |
| LCD | GC9307 172×320, `spidev1.0`, rot 180, MADCTL `0x48`, X-offset 34 | painter drives it directly |
| LCD pins | **RST=gpio122, DC=gpio121** on `2ae30000.gpio` = gpiochip3 (ground-truth from live vendor fds) | RK bank math `chip=gpio//32 off=gpio%32` (chip3 off26/off25); lines are `unnamed`, so name-lookup fails |
| resize tools | growpart / resize2fs / e2fsck / sgdisk present | firstboot can grow to 58 GB |

---

## Layout

```
emmc-install/
├── lcd/pcat_lcd.py                 GC9307 4-state progress painter (python spidev+gpiod)
├── firstboot/
│   ├── firstboot-emmc.sh           runs ON the eMMC, first boot: un-twin + grow + access + enrol
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
- the operator's SSH public key into `root/.ssh/authorized_keys` + sets
  `PermitRootLogin prohibit-password`,
- the reusable tagged Tailscale key onto the **boot partition** (`pcat-ts-authkey`)
  — not the rootfs `/boot` dir, which the boot partition mounts over at runtime
  (the shadow bug).

It **refuses** to clone a mounted / live-root source or scratch onto the disk
being imaged (torn-image / self-corruption guards), and preserves the original
partition GUID across the shrink.

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
golden image). It also:

- drops `.backup-openwrt` so the factory OpenWrt is backed up before the wipe (D2);
- **masks the vendor `pcat2_mini_display.service`** — it grabs the LCD's GPIO
  lines via legacy sysfs, which makes the installer's painter fail with EBUSY;
- **chroot-installs the runtime deps** (`zstd gdisk cloud-guest-utils
  python3-spidev python3-libgpiod`) so a missing tool can't silently disable the
  flash or the LCD in the field;
- refuses a non-SD / mounted / running-root card (`device/type` discriminator,
  same as the on-device installer).

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
4. Board boots eMMC Debian; `firstboot-emmc.sh`:
   - un-twins the clone (regen SSH host keys + machine-id, wipe Tailscale state),
   - grows the rootfs to fill the 58 GB eMMC (`sgdisk -e` + `growpart` + `resize2fs`),
   - sets hostname `pcat-<serial>`,
   - **guarantees the LAN access plane** (step 5b): enable sshd + avahi, restart
     avahi after the rename so `pcat-<serial>.local` resolves,
   - enrols into Tailscale with `--ssh` (clock-wait + 5× retry, then shreds the key),
   - **writes `/root/ACCESS.txt`** (step 7b): hostname, LAN IP, Tailscale IP, and
     ready-to-paste ssh lines, so the first login has the exact coordinates.

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

## Access after the box is on eMMC

Two everyday paths with **zero shared failure mode** — different auth, transport,
and discovery, so one cause can't kill both:

- **P1 — Tailscale SSH** (identity plane): `tailscale ssh root@pcat-<serial>`
  from any tailnet node. Requires the ACL to allow it — an ssh `accept` rule with
  `dst: tag:pcat` (an `autogroup:self` rule does **not** cover tagged devices).
  `grants` being allow-all also lets plain `ssh -i key root@<tailscale-ip>` over
  the tailnet.
- **P2 — LAN SSH** (key plane, Tailscale-independent): `ssh root@pcat-<serial>.local`
  with the baked key; avahi mDNS handles discovery so the DHCP address is not
  needed.

Physical recovery fallbacks:

- **P3** — re-insert any bootable SD (SD boot-priority boots a known-good OS to
  mount + repair the eMMC).
- **P4** — UART serial console (`serial-getty@ttyS0`), works with no network.

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
6. **Wrong-disk wipe** → the eMMC is detected by `device/type` (`removable` is
   useless — both report `0`) and must differ from the booted device; refuses to
   guess on ambiguity.
7. **Factory OpenWrt loss** → backed up to the init-SD before the wipe (D2).
8. **Tailscale key at rest** → reusable+tagged so one image serves many boxes;
   written to the boot partition (not shadowed by the `/boot` mount) and
   **shredded** right after enrol.
9. **LCD is a dashboard, not a text device** → a dedicated GC9307 painter
   (`pcat_lcd.py`) transcribed from the vendor's exact init sequence; libgpiod
   v2/v1 adapter (trixie ships v2), RK bank-math line resolution, EBUSY
   self-unexport retry, and the vendor display masked on the init-SD.
10. **Access after reload** → two independent SSH planes (see above) plus
    `/root/ACCESS.txt` breadcrumb and UART fallback, so a box is never
    unreachable after it boots off eMMC.
11. **Clock / TLS on enrol** → NTP wait + 5× retry so a wrong RTC doesn't break
    the Tailscale enrol (`x509: not yet valid`).
12. **One card, many boxes** → the installer's done-stamp lives in `/run` (tmpfs),
    not on the card, so every fresh power-on re-flashes whatever eMMC is present.
    One init-SD serves the whole fleet; re-powering a box with the card still in
    is a harmless idempotent re-flash (same golden bytes, sha256 verify passes).

## Open items before a real run

- Provide a **base Debian image** for the init-SD (or reuse a spare
  `golden.img`).
- The whole build needs a **Linux host that can see the SD cards** — the Mac Mini
  has no card reader; asterix is the natural build host.
- **Done:** reusable tagged Tailscale enroll key minted (`tag:pcat`, 90-day),
  `tag:pcat` added to `tagOwners`, and the ssh `accept` rule for `dst: tag:pcat`
  installed in the ACL (all verified live).
