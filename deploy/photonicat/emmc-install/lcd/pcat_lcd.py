#!/usr/bin/env python3
"""GC9307 SPI LCD progress painter for the Photonicat 2 init-SD installer.

The Photonicat 2's front panel is a GC9307 (ST7789-family) 172x320 SPI LCD. The
vendor's own driver (photonicat2_mini_display + periph.io-gc9307) is a Go
*dashboard* app that paints battery/network stats, not arbitrary text, so the
zero-touch installer needs its OWN minimal panel driver: this file. It is a
self-contained reimplementation of just the init + fill + text-blit path,
transcribed from the vendor Go register sequence (st7789.go) so the panel comes
up identically.

HARDENING (v2, T656): the v1 painter assumed the OLD libgpiod v1 Python API
(gpiod.Chip / get_line / LINE_REQ_DIR_OUT) and that "GPIO122" was a raw
gpiochip0 offset. Debian 13 (trixie) — the target OS — ships libgpiod **2.x**,
whose Python API is completely different (gpiod.request_lines / LineSettings),
and RK3568 exposes SIX gpiochips where "GPIOn" is bank/line-encoded, not a flat
chip0 offset. Either mistake makes the painter crash and the technician sees
NOTHING. This version:
  * supports BOTH libgpiod v2 (preferred) and v1 (fallback) via a thin adapter;
  * resolves the RST/DC lines by scanning every gpiochip for the RK-style line
    NAME, with the flat-offset guess only as a last resort;
  * is fully crash-tolerant — any failure prints to stderr and exits non-zero,
    and the installer already calls it with `|| true`, so a dead LCD degrades to
    "no progress screen" instead of aborting the flash (power-off stays the
    authoritative done-signal).

Wiring (from vendor main.go, RK3568): SPI /dev/spidev1.0 mode 0 8-bit 50 MHz;
RST=GPIO122, DC=GPIO121, CS=GPIO13 (vendor UseCS=false), BL=PWM. Geometry
172x320, rotation 180 => MADCTL 0x48 (MX|BGR), column offset 34, row offset 0.

Usage (per state; one-shot process each):
    pcat_lcd.py flashing 43 | verify | provisioning | done | error "msg"
"""

from __future__ import annotations

import sys
import time

# spidev + gpiod are the only non-stdlib deps; missing either is a clean exit so
# the caller's `|| true` swallows it (no progress screen, flash unaffected).
try:
    import spidev  # /dev/spidev1.0
    import gpiod
except Exception as exc:  # noqa: BLE001 — any import failure = no LCD, not fatal
    print(f"pcat_lcd: gpio/spi libs unavailable ({exc}); skipping LCD", file=sys.stderr)
    raise SystemExit(0)

# ── panel geometry / registers (vendor st7789.go + registers.go) ────────────
WIDTH, HEIGHT = 172, 320
COL_OFFSET, ROW_OFFSET = 34, 0
MADCTL_ROT180 = 0x48  # MX(0x40) | BGR(0x08) — vendor SetRotation case 2

SWRESET, SLPOUT, COLMOD = 0x01, 0x11, 0x3A
MADCTL, INVOFF, NORON, DISPON = 0x36, 0x20, 0x13, 0x29
CASET, RASET, RAMWR = 0x2A, 0x2B, 0x2C

# HARDWARE-VERIFIED (T656, live vendor process /proc/PID/fd on asterix): the
# vendor driver holds RST=global gpio122 and DC=global gpio121, both on the
# `2ae30000.gpio` controller = gpiochip3. The kernel exposes NO useful line
# names here (gpioinfo shows every line `unnamed`), so name lookup is hopeless;
# the RELIABLE mapping is RK bank arithmetic: 32 lines per bank/chip, so
#   chip = global // 32 , offset = global % 32
# 122 -> chip3 offset26 , 121 -> chip3 offset25 (matches the live fds exactly).
RST_GLOBAL, DC_GLOBAL = 122, 121

BLACK, WHITE = (0, 0, 0), (255, 255, 255)
GREEN, RED, AMBER = (0x86, 0xBC, 0x6F), (0xC8, 0x50, 0x50), (0xE5, 0xC0, 0x7B)


def enable_backlight() -> None:
    """Unblank + brighten the panel backlight via sysfs (best-effort).

    On the VENDOR image the backlight was already on (the vendor dashboard drove
    it), so the painter never had to. On a plain Armbian image nothing turns it
    on: the backlight defaults to FB_BLANK_POWERDOWN (bl_power=4) with the panel
    dark even though our SPI pixel writes succeed — the "black screen" symptom.
    We walk /sys/class/backlight/*, set bl_power=0 (unblank) and brightness high.
    Any failure is swallowed: a missing/odd backlight must not abort the paint.
    """
    import glob
    import os

    for bl in glob.glob("/sys/class/backlight/*"):
        try:
            with open(os.path.join(bl, "bl_power"), "w") as fh:
                fh.write("0")
        except Exception:  # noqa: BLE001 — best-effort
            pass
        try:
            maxb = 255
            try:
                with open(os.path.join(bl, "max_brightness")) as fh:
                    maxb = int(fh.read().strip() or "255")
            except Exception:  # noqa: BLE001
                pass
            with open(os.path.join(bl, "brightness"), "w") as fh:
                fh.write(str(maxb))
        except Exception:  # noqa: BLE001 — best-effort
            pass


def rgb565(rgb: tuple[int, int, int]) -> bytes:
    """Pack (R,G,B) into a big-endian RGB565 word (BGR handled by MADCTL)."""
    r, g, b = rgb
    v = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
    return bytes((v >> 8, v & 0xFF))


# ── libgpiod v2 / v1 adapter ─────────────────────────────────────────────────
# We expose a tiny GpioOut with .set(0/1) and .release(), built on whichever API
# the installed libgpiod offers. v2 is detected by the presence of request_lines.
_HAVE_V2 = hasattr(gpiod, "request_lines") or hasattr(gpiod, "LineSettings")


def _resolve_line(global_num: int):
    """Map an RK global GPIO number to (chip_path, offset).

    HARDWARE-VERIFIED (T656): RK banks are 32 lines each and map 1:1 onto the
    gpiochips, so chip index = global // 32 and offset = global % 32 (gpio122 ->
    gpiochip3 off26, confirmed against the live vendor process's open fds). We
    still cross-check the derived chip's line count and, if the arithmetic ever
    overshoots on an unexpected topology, walk the chips accumulating line
    counts to find the one that owns this global number.
    """
    chip_idx, offset = divmod(global_num, 32)
    cand = f"/dev/gpiochip{chip_idx}"

    def _num_lines(path: str) -> int:
        try:
            if _HAVE_V2:
                return gpiod.Chip(path).get_info().num_lines
            return gpiod.Chip(path).num_lines()
        except Exception:  # noqa: BLE001 — probe best-effort
            return 0

    # happy path: the 32-per-bank chip exists and is wide enough
    if _num_lines(cand) > offset:
        return cand, offset

    # fallback: accumulate line counts across chips (handles non-32 banks)
    import glob

    base = 0
    for path in sorted(glob.glob("/dev/gpiochip*")):
        n = _num_lines(path)
        if base <= global_num < base + n:
            return path, global_num - base
        base += n
    # last resort: trust the arithmetic even if we couldn't size the chip
    return cand, offset


class GpioOut:
    """A single output line, libgpiod-version-agnostic."""

    def __init__(self, global_num: int) -> None:
        path, off = _resolve_line(global_num)
        self._off = off
        self._request(path, off, global_num)

    def _request(self, path: str, off: int, global_num: int) -> None:
        """Claim the line, defeating a stale LEGACY-sysfs export on EBUSY.

        The vendor dashboard claims RST/DC through /sys/class/gpio (verified on
        asterix); a crashed holder can leave the line exported, so a fresh
        libgpiod request_lines fails with EBUSY (errno 16). We unexport it and
        retry once. On the init-SD the vendor service is masked, so this is
        belt-and-braces, not the primary fix.
        """
        try:
            self._claim(path, off)
        except OSError as exc:
            if exc.errno != 16:  # EBUSY only
                raise
            try:
                with open("/sys/class/gpio/unexport", "w") as fh:
                    fh.write(str(global_num))
            except Exception:  # noqa: BLE001 — best-effort
                pass
            time.sleep(0.2)
            self._claim(path, off)

    def _claim(self, path: str, off: int) -> None:
        if _HAVE_V2:
            settings = gpiod.LineSettings(direction=gpiod.line.Direction.OUTPUT)
            self._req = gpiod.request_lines(
                path, consumer="pcat_lcd", config={off: settings}
            )
            self._v2 = True
        else:
            chip = gpiod.Chip(path)
            self._line = chip.get_line(off)
            self._line.request(consumer="pcat_lcd", type=gpiod.LINE_REQ_DIR_OUT)
            self._v2 = False

    def set(self, value: int) -> None:
        if self._v2:
            v = gpiod.line.Value.ACTIVE if value else gpiod.line.Value.INACTIVE
            self._req.set_value(self._off, v)
        else:
            self._line.set_value(value)

    def release(self) -> None:
        try:
            (self._req if self._v2 else self._line).release()
        except Exception:  # noqa: BLE001
            pass


class Panel:
    """Minimal GC9307 driver: reset, init, windowed fills, scaled bitmap text."""

    def __init__(self) -> None:
        self.spi = spidev.SpiDev()
        self.spi.open(1, 0)  # bus 1 dev 0 -> /dev/spidev1.0
        self.spi.mode = 0
        self.spi.max_speed_hz = 50_000_000
        self._dc = GpioOut(DC_GLOBAL)
        self._rst = GpioOut(RST_GLOBAL)

    def _cmd(self, c: int) -> None:
        self._dc.set(0)
        self.spi.writebytes([c])

    def _data(self, payload: bytes) -> None:
        self._dc.set(1)
        for i in range(0, len(payload), 4096):  # spidev per-ioctl bufsiz cap
            self.spi.writebytes2(payload[i : i + 4096])

    def init(self) -> None:
        enable_backlight()  # Armbian leaves the panel blanked; the vendor didn't
        self._rst.set(1); time.sleep(0.010)
        self._rst.set(0); time.sleep(0.050)
        self._rst.set(1); time.sleep(0.010)
        self._cmd(SWRESET); time.sleep(0.010)
        self._cmd(SLPOUT); time.sleep(0.010)
        self._cmd(COLMOD); self._data(bytes((0x55,))); time.sleep(0.010)
        self._cmd(MADCTL); self._data(bytes((MADCTL_ROT180,)))
        self._cmd(INVOFF); time.sleep(0.010)
        self._cmd(NORON); time.sleep(0.010)
        self._cmd(DISPON); time.sleep(0.010)

    def _window(self, x: int, y: int, w: int, h: int) -> None:
        x += COL_OFFSET
        y += ROW_OFFSET
        self._cmd(CASET)
        self._data(bytes(((x >> 8) & 0xFF, x & 0xFF, ((x + w - 1) >> 8) & 0xFF, (x + w - 1) & 0xFF)))
        self._cmd(RASET)
        self._data(bytes(((y >> 8) & 0xFF, y & 0xFF, ((y + h - 1) >> 8) & 0xFF, (y + h - 1) & 0xFF)))
        self._cmd(RAMWR)

    def fill_rect(self, x: int, y: int, w: int, h: int, colour: tuple[int, int, int]) -> None:
        if w <= 0 or h <= 0:
            return
        self._window(x, y, w, h)
        self._data(rgb565(colour) * (w * h))

    def clear(self, colour: tuple[int, int, int] = BLACK) -> None:
        self.fill_rect(0, 0, WIDTH, HEIGHT, colour)

    def text(self, s: str, x: int, y: int, colour: tuple[int, int, int], scale: int = 2) -> None:
        cx = x
        for ch in s.upper():
            glyph = FONT5X7.get(ch, FONT5X7[" "])
            for col in range(5):
                bits = glyph[col]
                for row in range(7):
                    if bits & (1 << row):
                        self.fill_rect(cx + col * scale, y + row * scale, scale, scale, colour)
            cx += 6 * scale
            if cx > WIDTH:  # don't run glyphs off the panel
                break

    def close(self) -> None:
        try:
            self.spi.close()
        finally:
            self._dc.release()
            self._rst.release()


# ── 5x7 bitmap font — column-major, LSB=top row (installer glyphs only) ──────
FONT5X7: dict[str, tuple[int, int, int, int, int]] = {
    " ": (0x00, 0x00, 0x00, 0x00, 0x00),
    "%": (0x23, 0x13, 0x08, 0x64, 0x62),
    "-": (0x08, 0x08, 0x08, 0x08, 0x08),
    ".": (0x00, 0x60, 0x60, 0x00, 0x00),
    "0": (0x3E, 0x51, 0x49, 0x45, 0x3E),
    "1": (0x00, 0x42, 0x7F, 0x40, 0x00),
    "2": (0x42, 0x61, 0x51, 0x49, 0x46),
    "3": (0x21, 0x41, 0x45, 0x4B, 0x31),
    "4": (0x18, 0x14, 0x12, 0x7F, 0x10),
    "5": (0x27, 0x45, 0x45, 0x45, 0x39),
    "6": (0x3C, 0x4A, 0x49, 0x49, 0x30),
    "7": (0x01, 0x71, 0x09, 0x05, 0x03),
    "8": (0x36, 0x49, 0x49, 0x49, 0x36),
    "9": (0x06, 0x49, 0x49, 0x29, 0x1E),
    "A": (0x7E, 0x11, 0x11, 0x11, 0x7E),
    "B": (0x7F, 0x49, 0x49, 0x49, 0x36),
    "C": (0x3E, 0x41, 0x41, 0x41, 0x22),
    "D": (0x7F, 0x41, 0x41, 0x22, 0x1C),
    "E": (0x7F, 0x49, 0x49, 0x49, 0x41),
    "F": (0x7F, 0x09, 0x09, 0x09, 0x01),
    "G": (0x3E, 0x41, 0x49, 0x49, 0x7A),
    "H": (0x7F, 0x08, 0x08, 0x08, 0x7F),
    "I": (0x00, 0x41, 0x7F, 0x41, 0x00),
    "K": (0x7F, 0x08, 0x14, 0x22, 0x41),
    "L": (0x7F, 0x40, 0x40, 0x40, 0x40),
    "M": (0x7F, 0x02, 0x0C, 0x02, 0x7F),
    "N": (0x7F, 0x04, 0x08, 0x10, 0x7F),
    "O": (0x3E, 0x41, 0x41, 0x41, 0x3E),
    "P": (0x7F, 0x09, 0x09, 0x09, 0x06),
    "R": (0x7F, 0x09, 0x19, 0x29, 0x46),
    "S": (0x46, 0x49, 0x49, 0x49, 0x31),
    "T": (0x01, 0x01, 0x7F, 0x01, 0x01),
    "U": (0x3F, 0x40, 0x40, 0x40, 0x3F),
    "V": (0x1F, 0x20, 0x40, 0x20, 0x1F),
    "W": (0x7F, 0x20, 0x18, 0x20, 0x7F),
    "X": (0x63, 0x14, 0x08, 0x14, 0x63),
    "Y": (0x07, 0x08, 0x70, 0x08, 0x07),
    "Z": (0x61, 0x51, 0x49, 0x45, 0x43),
}


def _screen_flashing(p: Panel, pct: int) -> None:
    pct = max(0, min(100, pct))
    p.clear(BLACK)
    p.text("FLASHING", 8, 40, AMBER, scale=2)
    bx, by, bw, bh = 12, 150, WIDTH - 24, 26
    p.fill_rect(bx, by, bw, bh, WHITE)
    p.fill_rect(bx + 2, by + 2, bw - 4, bh - 4, BLACK)
    p.fill_rect(bx + 2, by + 2, int((bw - 4) * pct / 100), bh - 4, AMBER)
    p.text(f"{pct}%", 60, 200, WHITE, scale=3)
    p.text("DO NOT REMOVE", 14, 270, WHITE, scale=1)


def _screen_verify(p: Panel) -> None:
    p.clear(BLACK)
    p.text("VERIFYING", 8, 120, WHITE, scale=2)
    p.text("CHECKSUM", 20, 160, WHITE, scale=2)


def _screen_provisioning(p: Panel) -> None:
    # Shown while pcat-provision.service runs the local Ansible play (download
    # release + Caddy, seed admin, compile the display driver). Minutes-long,
    # so a static "working" screen is enough — no percentage is available.
    p.clear(BLACK)
    p.text("INSTALLING", 8, 110, AMBER, scale=2)
    p.text("CONTEXT PILOT", 8, 150, AMBER, scale=1)
    p.text("PLEASE WAIT", 20, 210, WHITE, scale=1)


def _screen_done(p: Panel) -> None:
    p.clear(BLACK)
    p.text("DONE", 40, 110, GREEN, scale=4)
    p.text("REMOVE SD", 16, 180, GREEN, scale=2)
    p.text("POWERING OFF", 18, 240, WHITE, scale=1)


def _screen_error(p: Panel, msg: str) -> None:
    p.clear(BLACK)
    p.text("ERROR", 30, 110, RED, scale=4)
    line = msg[:42]
    for i in range(0, len(line), 14):
        p.text(line[i : i + 14], 8, 180 + (i // 14) * 16, WHITE, scale=1)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: pcat_lcd.py {flashing PCT|verify|done|error [msg]}", file=sys.stderr)
        return 2
    state = argv[1].lower()
    try:
        panel = Panel()
    except Exception as exc:  # noqa: BLE001 — no panel = no progress, never fatal
        print(f"pcat_lcd: panel init failed ({exc})", file=sys.stderr)
        return 0
    try:
        panel.init()
        if state == "flashing":
            _screen_flashing(panel, int(argv[2]) if len(argv) > 2 else 0)
        elif state == "verify":
            _screen_verify(panel)
        elif state == "provisioning":
            _screen_provisioning(panel)
        elif state == "done":
            _screen_done(panel)
        elif state == "error":
            _screen_error(panel, argv[2] if len(argv) > 2 else "unknown")
        else:
            print(f"unknown state {state!r}", file=sys.stderr)
            return 2
    except Exception as exc:  # noqa: BLE001 — a paint failure must not abort install
        print(f"pcat_lcd: paint failed ({exc})", file=sys.stderr)
        return 0
    finally:
        panel.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
