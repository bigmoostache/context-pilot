# Photonicat 2 → Debian 13 — full flashing protocol

> Complete, button-by-button procedure to put mainline **Debian 13** on a
> **Photonicat 2**. Two paths: **microSD** (simplest, reversible — do this first)
> and **eMMC** (permanent, standalone). Written for a **macOS** host.
>
> ⚠️ **Photonicat 2 = Rockchip RK3576.** The *original* Photonicat is RK3568 —
> different images, different loader. Use only `photonicat2` / `rk3576` files.

---

## 0. What you'll need

- The Photonicat 2 (charged, or with power adapter).
- A **microSD card** (≥ 8 GB) + an SD reader for the Mac — for Path A.
- A **USB-A ↔ USB-A** cable (both ends standard USB-A male) — for Path B (eMMC).
  The board's flashing port is the **USB OTG port**; the PC must supply solid
  power or the burn fails.
- ~1 GB free for the minimal image (or ~2.5 GB for the full/desktop image).

### Which image?

Prebuilt, no compilation needed. Folder:
`https://dl.photonicat.com/images/photonicat2/debian-mainline/`

| Image | Size | Pick if… |
|-------|------|----------|
| `rk3576-photonicat2-mainline-debian13-minimal-20260511.img.gz` | ~906 MB | Headless box / router / server **(recommended)** |
| `rk3576-photonicat2-mainline-debian13-full-20260511.img.gz` | ~2.2 GB | You want the GNOME desktop preinstalled (needs ≥ 2 GB RAM, ≥ 16 GB eMMC) |

- **`debian-mainline`** = upstream kernel — cleanest, most robust for a general
  Debian box. **This is the pick for "simplest + most robust".**
- **`debian-rk`** (sibling folder) = Rockchip BSP kernel — only if you need full
  vendor hardware accel (NPU, GPU quirks, some 5G modems). Heavier.

**SHA256 (2026‑05‑11 builds):**
```
da3566312356554484ecbbab70d2f38b6c31c26822ad278438f7baf81fad1be7  minimal-20260511
f6b414af8d5f9c2b5649059f159a948b3e620e7305ad26d11c9356b8fabf43a1  full-20260511
```

---

## 1. (Optional but recommended) Back up the current OpenWrt config

If the box still runs stock photonicatWrt and you want to keep its settings:

1. Connect to its WiFi (default password `photonicat`) or a LAN port.
2. Open `http://172.16.0.1/cgi-bin/luci/admin/system/flashops`
3. Click **Generate archive** → saves a `.tar` backup to your Mac.

---

## 2. Download + verify the image (on the Mac)

```sh
cd ~/Downloads

# 1. image + checksums
curl -LO https://dl.photonicat.com/images/photonicat2/debian-mainline/rk3576-photonicat2-mainline-debian13-minimal-20260511.img.gz
curl -LO https://dl.photonicat.com/images/photonicat2/debian-mainline/SHA256SUMS

# 2. verify (must print a line ending in "OK")
shasum -a 256 rk3576-photonicat2-mainline-debian13-minimal-20260511.img.gz
#   compare the hash against da3566...1be7 (minimal) in SHA256SUMS

# 3. uncompress (needed for dd / rkdeveloptool; Etcher can take the .gz directly)
gunzip -k rk3576-photonicat2-mainline-debian13-minimal-20260511.img.gz
#   -k keeps the .gz; you now also have the .img
```

A truncated download is the #1 cause of "it won't boot" — do not skip the hash.

---

## Path A — microSD (simplest, safest, reversible) ← start here

The board boots from SD when a card is present, leaving the eMMC untouched. If
anything goes wrong, pull the card and you're back to normal.

### A.1 — Flash the card (choose ONE method)

**Method 1 — balenaEtcher (GUI, zero commands, easiest):**
1. Install Etcher: `brew install --cask balenaetcher` (or download from balena.io).
2. Open Etcher → **Flash from file** → select the `.img.gz` (Etcher decompresses
   automatically).
3. **Select target** → your SD card.
4. **Flash!** → wait for flash + validate to finish.

**Method 2 — `dd` (terminal):**
```sh
diskutil list                      # find your SD card, e.g. /dev/disk4
diskutil unmountDisk /dev/disk4    # unmount (do NOT eject)
# NOTE the 'r' in rdisk4 = raw device = much faster
sudo dd if=rk3576-photonicat2-mainline-debian13-minimal-20260511.img \
        of=/dev/rdisk4 bs=4m status=progress
sync
diskutil eject /dev/disk4
```
> 🛑 Triple-check the disk number. `dd` to the wrong `/dev/diskN` wipes it. Your
> Mac's internal disk is `disk0`/`disk1` — never target those.

### A.2 — Boot the Photonicat 2 from SD

1. **Power OFF** the box (if on, long-press the button 3 s → power LED off).
2. Insert the microSD card.
3. **Power ON**: long-press the button **3 s** → power LED goes solid.
4. Give it 1–2 minutes on first boot (it expands the root filesystem).

### A.3 — Log in

- If you have a **USB-C serial / HDMI + keyboard**, use the console directly.
- Otherwise, find it on the network (see **§4 First boot & login**).

If you like it, you can make it permanent by copying it to eMMC from *inside*
the running system — see **Path C**. Otherwise, you're done.

---

## Path B — eMMC via maskrom (permanent, standalone)

Writes Debian straight to the onboard eMMC using Rockchip's loader while the
board is in **maskrom (burn) mode**.

> ⚠️ **macOS caveat.** Rockchip's flashing tools (`upgrade_tool`, `RKDevTool`)
> are **Linux/Windows only**, and `rkdeveloptool` on macOS is flaky. If you only
> have the Mac, prefer **Path A + Path C** (SD first, then copy to eMMC from
> inside Debian) — it needs no Rockchip tool at all. Use Path B if you have a
> **Linux PC** (or a Linux VM with USB passthrough) or a Windows PC.

### B.1 — Get the loader + tool (on a Linux PC)

```sh
# loader (RK3576-specific — NOT the rk3568 one)
wget https://dl.photonicat.com/images/photonicat2/RK3576_MiniLoaderAll.bin

# build rkdeveloptool
git clone https://github.com/rockchip-linux/rkdeveloptool.git
cd rkdeveloptool
sudo apt-get install -y libudev-dev libusb-1.0-0-dev dh-autoreconf
aclocal && autoreconf -i && autoheader && automake --add-missing && ./configure && make
```
(Alternatively use Photonicat's `Linux_Upgrade_Tool_v1.57` from
`https://dl.photonicat.com/tools/` — its commands are `upgrade_tool ef|db|wl|rd`.)

### B.2 — Enter maskrom (burn) mode — exact button sequence

**4G/5G version (with battery):**
1. **Power the box OFF** (long-press 3 s if it's on → LED off).
2. **Connect the USB-A↔USB-A cable first**: board USB OTG port ↔ PC USB port.
3. On the button: **short-press 3 times**, then **long-press ≥ 10 s** and hold.
4. Release when the **power LED blinks FAST** (0.25 s on / 0.25 s off) — that's
   burn mode.

**Home version (no battery):**
1. Keep it powered (USB-C 5 V connected).
2. Connect the USB-A↔USB-A cable to the OTG port ↔ PC.
3. **Long-press `Reset` for 9 s** → power LED blinks fast (0.25/0.25) = burn mode.

> Reference video: `https://dl.photonicat.com/misc/photonicat_flashing_mode_video.mp4`
>
> (For contrast — **factory reset** is a *different* combo: 4G/5G = short-press ×1
> then long-press ≥ 17 s; Home = hold Reset 16 s. LED then blinks **slow**,
> 0.5/0.5. You do **not** want this for flashing.)

### B.3 — Flash

```sh
# confirm the board is seen in maskrom
sudo ./rkdeveloptool ld            # should list a Maskrom device

# (only when switching kernel TYPE — e.g. OpenWrt→Debian — erase first)
# with Linux_Upgrade_Tool:  sudo ./upgrade_tool ef RK3576_MiniLoaderAll.bin

sudo ./rkdeveloptool db RK3576_MiniLoaderAll.bin      # load bootloader
sudo ./rkdeveloptool wl 0 rk3576-photonicat2-mainline-debian13-minimal-20260511.img
sudo ./rkdeveloptool rd                               # reboot into Debian
```
The board reboots into Debian off the eMMC. No SD card needed afterward.

---

## Path C — SD → eMMC copy (macOS-friendly, no Rockchip tool)

Best route if you only have a Mac. Flash SD (Path A), boot Debian from it, then
clone it onto the internal eMMC from inside the running system.

1. Do **Path A** (SD) and boot Debian from the card.
2. Log in (see **§4**), become root.
3. Identify the devices:
   ```sh
   lsblk
   #   mmcblk1 (or mmcblk0) = the SD you're running from
   #   mmcblk0 (or mmcblk1) = the internal eMMC  ← the OTHER one
   #   Confirm sizes to be sure which is which before writing!
   ```
4. Write the *same uncompressed image* (copy it onto the SD's Debian first, e.g.
   via `scp` or a USB stick) to the eMMC:
   ```sh
   # DOUBLE-CHECK the target is the eMMC, not your running SD:
   dd if=rk3576-photonicat2-mainline-debian13-minimal-20260511.img \
      of=/dev/mmcblk0 bs=4M status=progress conv=fsync
   sync
   ```
5. Power off, **remove the SD card**, power on. It now boots from eMMC.

---

## 3. First boot & login

- **First boot takes 1–2 min** — the image auto-expands the rootfs to fill the
  card/eMMC. Don't pull power during this.
- **Find it on the network:** plug Ethernet into the LAN/WAN port, or check your
  router's DHCP leases for a new host. Then:
  ```sh
  ping photonicat2.local            # mDNS may resolve it
  # or scan your subnet:
  # (on the Mac) arp -a    →  ssh <user>@<its-ip>
  ```
- **Default credentials (⚠️ verify):** the official docs don't publish the
  Debian image's default login. For these Rockchip mainline builds it's commonly
  **`root` / `root`** or **`pcat` / `pcat`**. Try those first.
  - **If locked out:** on the Mac, put the SD back in the reader, mount the
    rootfs partition, and clear/reset the root password in `/etc/shadow`
    (or boot to single-user via u-boot). I can walk you through this if needed.

### First-login hardening (recommended)

```sh
passwd                              # set a real root password
# create your user
adduser guillaume && usermod -aG sudo guillaume
# enable + secure SSH
apt update && apt install -y openssh-server
systemctl enable --now ssh
# set the hostname
hostnamectl set-hostname photonicat2
```

---

## 4. Gotchas checklist

- ✅ **RK3576 / photonicat2 files only.** The RK3568 loader/images will not boot
  a Photonicat 2.
- ✅ **Verify SHA256** before flashing.
- ✅ **`gunzip` the image** for `dd` / `rkdeveloptool` (Etcher accepts `.gz`).
- ✅ **USB cable in first, then the button combo** for maskrom (and give the PC
  port real power).
- ✅ **Burn mode = FAST blink** (0.25/0.25). Slow blink (0.5/0.5) is *factory
  reset* — wrong mode.
- ✅ On macOS-only, prefer **SD (Path A)** then **SD→eMMC (Path C)** — avoids the
  flaky Rockchip-on-macOS tooling entirely.
- ✅ **Try SD first.** It's reversible: pull the card to get your old system back.

---

## Source references

- Prebuilt images: `dl.photonicat.com/images/photonicat2/debian-mainline/`
- Loader: `dl.photonicat.com/images/photonicat2/RK3576_MiniLoaderAll.bin`
- Build/flash repo (RK3576): `github.com/photonicat/rockchip_rk3576_linux_mainline`
- Official flashing manual: `photonicat.com/wiki/Photonicat_刷机操作手册`
- Button functions: `photonicat.com/wiki/Photonicat_快速上手`
- Tools: `dl.photonicat.com/tools/` · Video: `dl.photonicat.com/misc/photonicat_flashing_mode_video.mp4`
