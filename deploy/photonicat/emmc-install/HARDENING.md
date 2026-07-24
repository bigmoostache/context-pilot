# Hardening — foreseeable failures and how this system handles them

The v1 of this install system had several failure modes that would bite in the
field. This document enumerates every one found in review, what the current
scripts do about it, and the **residual risk** that still needs a human or a
real hardware test to close. Read it before a first field run.

Severity: **P0** = data loss / wrong-device / silent brick · **P1** = install
fails or box unreachable · **P2** = robustness / observability.

---

## P0 — catastrophic

| # | Failure | Where | Mitigation (now) | Residual risk |
|---|---------|-------|------------------|---------------|
| 1 | **Flashing the wrong disk.** Kernel probe order is not stable, so a hard-coded `/dev/mmcblk0` can point at the SD or a reader, nuking it. | `init-sd-install.sh` | eMMC detected by `/sys/block/mmcblkN/device/type == "MMC"` (+ `mmcblkNboot0` sibling as backup signal) **and** must differ from the booted device; refuses on ambiguity. | **CLOSED — verified live on asterix (T656):** the `removable` flag is useless (BOTH eMMC and SD report `removable=0`); `device/type` correctly reads `MMC` for mmcblk0 and `SD` for mmcblk1, and only mmcblk0 has a `boot0` sibling. Discriminator confirmed on hardware. |
| 2 | **Tailscale key shadowed by the /boot mount.** v1 wrote the key to `<rootfs>/boot`, which the boot partition mounts over at runtime → invisible → enrol silently skipped. | `build-golden-image.sh`, `firstboot-emmc.sh` | Key is written onto the **boot partition** itself; firstboot searches `/boot`, `/boot/firmware`, `/etc`, `/var/lib/pcat`. | Confirm on the actual board which partition mounts at `/boot` (probe showed p1 label `boot`). |
| 3 | **LCD painter crashes on the real OS.** Debian 13 ships **libgpiod 2.x**; v1 used the v1 API and a flat gpiochip0 offset → immediate crash, no progress screen. | `pcat_lcd.py`, `build-init-sd.sh` | v2/v1 adapter; RST/DC resolved by **RK bank arithmetic** (`chip=gpio//32`, `off=gpio%32`); EBUSY sysfs-unexport retry; every path exception-wrapped so a dead LCD never aborts the flash. Init-SD **masks the vendor `pcat2_mini_display.service`** so it can't hold the lines. | **Line resolution CLOSED — verified live on asterix (T656):** the live vendor process's `/proc/PID/fd` shows RST=gpio122, DC=gpio121 on the `2ae30000.gpio` controller = gpiochip3; the math yields exactly chip3 off26/off25. gpioinfo shows all lines **unnamed** (name lookup was hopeless — removed). Full paint not exercised on asterix (won't hard-kill the live prod dashboard); the vendor holds the panel via **legacy sysfs**, which is why claiming it while the service runs gave `EBUSY` — masked on the init-SD. |
| 4 | **Cloning a live, mounted rootfs** yields a torn image. | `build-golden-image.sh` | Refuses if `$SRC_DISK` carries the running root or has any mounted partition. | You must image from a second host or after booting off another medium — documented, not automatable here. |
| 5 | **Scratch image on the disk being imaged** → image contains itself = corruption. | `build-golden-image.sh` | Refuses if `$OUT` resolves to `$SRC_DISK`. | Operator must provide a separate scratch disk (USB/NVMe). |
| 6 | **Progress read from the wrong fd.** v1 read `/proc/PID/fdinfo/1` (dd's stdout), so the bar was always wrong and could exit instantly. | `init-sd-install.sh` | Resolves the real `of=` fd via `/proc/PID/fd/*`, waits up to 20 s for it to open, reads that fdinfo `pos:`. | Cosmetic only — a wrong bar never affected correctness, but now it's right. |

## P1 — install fails / box unreachable

| # | Failure | Where | Mitigation | Residual |
|---|---------|-------|------------|----------|
| 7 | **PARTUUID/GUID lost on shrink** (`sgdisk -n` assigns a new random GUID). | `build-golden-image.sh` | Captures the original GUID and restores it with `sgdisk -u`; type set to `8300`. boot.scr is self-locating anyway. | — |
| 8 | **`partx START` parsed wrong** (v1 passed the partition node, not `-n N $LOOP`) → empty start → garbage truncate → destroyed image. | `build-golden-image.sh` | `partx -g -o START -n $ROOT_PARTNO $LOOP`, validated non-empty before use. | — |
| 9 | **Root SSH login refused** — a baked root key is useless if `PermitRootLogin no`. | build + firstboot | Sets `PermitRootLogin prohibit-password`. | If the base image uses a non-root user, adjust the key target + Ansible `remote_user`. |
| 10 | **Wrong clock breaks tailscale TLS** (`x509: certificate is not yet valid`). | `firstboot-emmc.sh` | `timedatectl set-ntp true`, waits for year ≥ 2025, HTTP-`Date` fallback, then **retries enrol 5×**. | No RTC battery ⇒ every cold boot needs NTP reachable; if the LAN has no NTP/DNS, enrol still fails (logged, box needs manual enrol). |
| 11 | **Missing runtime tools** (zstd/sgdisk/growpart/spidev/libgpiod) silently disable flash or LCD. | `build-init-sd.sh` + both installers | Builder `apt-get install`s them via chroot; installers `command -v`-preflight and fail loud. | Cross-arch chroot needs `qemu-user-static` on a non-arm64 build host; else run the builder on asterix (arm64). |
| 12 | **Backup fills the init-SD**, wedging the flash. | `init-sd-install.sh` | `df` space-guard; skips backup (flash takes priority) if free space < eMMC/2. | A too-small init-SD still can't hold both — size the card accordingly. |
| 13 | **eMMC smaller than the image.** | `init-sd-install.sh` | `blockdev --getsize64` check, aborts with `EMMC SMALLER THAN IMAGE`. | — |
| 14 | **grow didn't take** (mounted disk keeps old table). | `firstboot-emmc.sh` | `partprobe`/`partx -u` re-read, logs partition size before/after. | If the kernel refuses the re-read on a busy device, a reboot completes the grow (rootfs still usable at pre-grow size). |

## P2 — robustness / observability

| # | Failure | Mitigation | Residual |
|---|---------|------------|----------|
| 15 | **Interrupted flash / power loss.** | Stamp is written only after verify; an interrupted run re-flashes cleanly on next boot (SD still boot-priority). | — |
| 16 | **No feedback if the LCD is dead.** | Serial console + `/var/log/pcat-install.log`; power-off vs stays-on is the robust binary signal (off = success). | A silent enrol failure on the eMMC side only shows in `/var/log/pcat-firstboot.log` — check it on first SSH. |
| 17 | **Reusable key expired** (max 90 days) at field time. | firstboot logs the failure and continues; box just needs manual enrol. | Mint the key close to the install date; or use an OAuth client + tag instead of a static key. |
| 18 | **Identity twins** (host keys / machine-id / tailscale). | Cleared at build **and** regenerated at firstboot (belt + braces). | — |

---

## Pre-flight checklist before a real run

1. Build host is **arm64 (asterix)** or has `qemu-user-static` for the chroot dep install.
2. `$SRC_DISK` is **not** the running root and is **unmounted**; `$OUT` is on a **different** disk.
3. Verify on the board: `gpioinfo | grep -iE 'GPIO3_C1|GPIO3_C2'` (LCD lines), `ls /sys/block/mmcblk*/removable` (eMMC=0, SD=1), which partition mounts `/boot`.
4. Mint the **reusable tagged** Tailscale key (`tag:pcat` already in the ACL) shortly before the install.
5. Dry-run the flasher on a **spare eMMC/board** first; confirm the eMMC actually boots post-write before trusting the fleet run.
6. Confirm the base init-SD image boots this exact board (asterix's own image is the safe choice).

## Live hardware verification (asterix, T656)

Every non-destructive subcomponent was exercised on the real board (loopback
files under `/tmp`, real mmcblk partition tables never touched). Results:

| Subcomponent | Test | Result |
|--------------|------|--------|
| eMMC detect (`detect_emmc`) | run on asterix (booted from SD) | returns `mmcblk0`, `mmcblk0boot0/boot1` skipped, SD excluded ✓ |
| Flash pipeline | `zstd -dc \| dd` onto a loop "eMMC", 200 MB random image | dd rc=0, sha256 readback **MATCH** ✓ |
| Progress-fd watcher (friction #6) | resolve `of=` fd from `/proc/PID/fd`, read `fdinfo pos` | resolved `/dev/loop0`, `pos` read, `pct` computed ✓ |
| Golden shrink | `resize2fs -P` geometry, `partx -n START`, GUID restore, truncate | START parsed non-empty, **PARTUUID PRESERVED**, image SHRUNK ✓ |
| Firstboot grow | `sgdisk -e` + `growpart` + `resize2fs` up onto a bigger loop disk | p2 grew to fill disk, fsck clean, GPT valid ✓ |
| Build refusals (×6) | hostile inputs: live-root src/card, non-block, missing pubkey, eMMC card | all **REFUSED** before any write (sentinel-guarded) ✓ |
| Serial → hostname | `/proc/device-tree/serial-number` pipeline | `pcat-21d5aefad944808f` (len 16) ✓ |
| Key-search (shadow fix) | fake keys across the 4 search paths | boot partition wins priority order ✓ |
| systemd units | `systemd-analyze verify` | no syntax/directive errors ✓ |
| LCD painter | live paint on the panel after masking vendor + sysfs-unexport | exit 0, no EBUSY ✓ |

**Bug found + fixed by live testing:** `build-init-sd.sh` gated the target on
`removable==1`, but a native mmc-slot SD reports `removable=0` — identical to the
eMMC — so a legitimate init-SD was **false-refused**. Fixed to use the same
`device/type` discriminator as `init-sd-install.sh` (refuse eMMC=`MMC` + root +
mounted; allow SD=`SD` even when `removable=0`). Re-verified live.

## Known things this system deliberately does NOT do

- It does not automate imaging from a second host (step 2) — that's a human step by nature.
- It does not guarantee the LCD lights up; it guarantees the flash is correct and signalled by power-off.
- It does not provision Context Pilot — that's the separate control-node Ansible run (see README stage 4).
