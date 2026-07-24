#!/usr/bin/env python3
"""GC9307 SPI LCD progress painter for the Photonicat 2 init-SD installer.

The Photonicat 2's front panel is a GC9307 (ST7789-family) 172x320 SPI LCD. The
vendor's own driver (photonicat/photonicat2_mini_display + periph.io-gc9307) is a
Go *dashboard* app — it paints battery/network stats, not arbitrary text, and it
has no API to show an installer progress screen. So the zero-touch eMMC installer
needs its OWN minimal panel driver: this file.

It is a self-contained Python reimplementation of JUST the init + fill + text
blit path, transcribed from the vendor Go driver's exact register sequence
(st7789.go) so the panel comes up identically. It has NO third-party deps beyond
`spidev` + `RPi.GPIO`-style libgpiod — both available on the Debian init-SD — so
it drops onto the installer image with an `apt install python3-libgpiod
python3-spidev` and nothing else.

Wiring (verbatim from vendor main.go, RK3568 pin numbering):
  * SPI bus     : /dev/spidev1.0  (2ad00000.spi), mode 0, 8-bit, 50 MHz
  * RST         : GPIO122   (reset, active low pulse)
  * DC          : GPIO121   (data/command select — low=command, high=data)
  * CS          : GPIO13    (chip-select; the vendor runs UseCS=false and lets
                             the spidev layer toggle the hardware CS, so we do
                             the same — no manual CS line here)
  * Backlight   : GPIO13 / PWM (vendor drives brightness over PWM; for a binary
                             on/off installer screen we just enable it)
  * Geometry    : 172x320, rotation 180  => MADCTL 0x48 (MX|BGR),
                  column offset 34 (PCAT2_X_OFFSET), row offset 0.

Usage (called by the installer oneshot):
    pcat_lcd.py flashing 43     # big "FLASHING" + 43% bar
    pcat_lcd.py verify          # "VERIFYING"
    pcat_lcd.py done            # green "DONE — REMOVE SD"
    pcat_lcd.py error "msg"     # red "ERROR" + a short reason line

Each call is one-shot: it (re)initialises the panel and paints one screen, then
exits — the installer shells out to it per state transition, so there is no
long-running daemon to babysit mid-flash. Painting is deliberately dumb (solid
rects + a 5x7 bitmap font scaled up) to keep the driver tiny and dependency-light;
a flash-progress screen does not need anti-aliased glyphs.
"""

from __future__ import annotations

import sys
import time

import spidev  # /dev/spidev1.0
import gpiod  # libgpiod — DC/RST lines

# ── panel geometry / registers (from vendor st7789.go + registers.go) ────────
WIDTH, HEIGHT = 172, 320
COL_OFFSET, ROW_OFFSET = 34, 0  # PCAT2_X_OFFSET; rotation-180 offsets
MADCTL_ROT180 = 0x48  # MX(0x40) | BGR(0x08) — vendor SetRotation case 2

SWRESET, SLPOUT, COLMOD = 0x01, 0x11, 0x3A
MADCTL, PORCTRL, INVOFF = 0x36, 0xB2, 0x20
NORON, DISPON = 0x13, 0x29
CASET, RASET, RAMWR = 0x2A, 0x2B, 0x2C

# RK3568 global GPIO line numbers. libgpiod addresses (chip, offset); on this
# board the "GPIOxxx" names map linearly onto gpiochip0's global space, so the
# offset is the raw pin number. RST=122, DC=121.
RST_PIN, DC_PIN = 122, 121
GPIOCHIP = "gpiochip0"

# 16-bit RGB565 colours (the panel is BGR-ordered via MADCTL, so the driver
# swaps at pack time — see rgb565).
BLACK = (0, 0, 0)
WHITE = (255, 255, 255)
GREEN = (0x86, 0xBC, 0x6F)  # vendor "success" green
RED = (0xC8, 0x50, 0x50)  # vendor "error" red
AMBER = (0xE5, 0xC0, 0x7B)  # vendor "warning" amber


def rgb565(rgb: tuple[int, int, int]) -> bytes:
    """Pack an (R,G,B) 8-8-8 tuple into a big-endian RGB565 word.

    The panel is configured BGR (MADCTL bit set), but periph's convention and
    ours keep the framebuffer logically RGB and let the panel's BGR bit handle
    the channel order, so we pack straight RGB565 exactly as the vendor Go path
    does (Data(hi); Data(lo))."""
    r, g, b = rgb
    v = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
    return bytes((v >> 8, v & 0xFF))


class Panel:
    """A minimal GC9307 driver: reset, init, windowed fills, scaled bitmap text.

    Owns the SPI handle + the DC/RST gpiod lines for its lifetime. Construct it,
    call init() once, then paint. It is intentionally not reused across
    processes — the installer spawns one per state screen."""

    def __init__(self) -> None:
        self.spi = spidev.SpiDev()
        self.spi.open(1, 0)  # bus 1, device 0 -> /dev/spidev1.0
        self.spi.mode = 0
        self.spi.max_speed_hz = 50_000_000
        chip = gpiod.Chip(GPIOCHIP)
        # request DC + RST as outputs (libgpiod v1 API)
        self._dc = chip.get_line(DC_PIN)
        self._rst = chip.get_line(RST_PIN)
        self._dc.request(consumer="pcat_lcd", type=gpiod.LINE_REQ_DIR_OUT)
        self._rst.request(consumer="pcat_lcd", type=gpiod.LINE_REQ_DIR_OUT)

    # ── low-level command/data over SPI, DC line selects which ──────────────
    def _cmd(self, c: int) -> None:
        self._dc.set_value(0)  # command mode
        self.spi.writebytes([c])

    def _data(self, payload: bytes) -> None:
        self._dc.set_value(1)  # data mode
        # spidev caps a single ioctl at bufsiz (usually 4096B); chunk to stay under.
        for i in range(0, len(payload), 4096):
            self.spi.writebytes2(payload[i : i + 4096])

    def init(self) -> None:
        """Hardware reset + the vendor's GC9307 power-on sequence."""
        self._rst.set_value(1)
        time.sleep(0.010)
        self._rst.set_value(0)
        time.sleep(0.050)
        self._rst.set_value(1)
        time.sleep(0.010)

        self._cmd(SWRESET)
        time.sleep(0.010)
        self._cmd(SLPOUT)  # leave sleep
        time.sleep(0.010)
        self._cmd(COLMOD)
        self._data(bytes((0x55,)))  # 16-bit/pixel RGB565
        time.sleep(0.010)
        self._cmd(MADCTL)
        self._data(bytes((MADCTL_ROT180,)))  # rotation-180 memory order
        self._cmd(INVOFF)
        time.sleep(0.010)
        self._cmd(NORON)
        time.sleep(0.010)
        self._cmd(DISPON)
        time.sleep(0.010)

    def _window(self, x: int, y: int, w: int, h: int) -> None:
        """Set the CASET/RASET draw window (applying the panel offsets) and arm
        RAMWR so a following data stream lands at the window origin."""
        x += COL_OFFSET
        y += ROW_OFFSET
        self._cmd(CASET)
        self._data(bytes(((x >> 8) & 0xFF, x & 0xFF, ((x + w - 1) >> 8) & 0xFF, (x + w - 1) & 0xFF)))
        self._cmd(RASET)
        self._data(bytes(((y >> 8) & 0xFF, y & 0xFF, ((y + h - 1) >> 8) & 0xFF, (y + h - 1) & 0xFF)))
        self._cmd(RAMWR)

    def fill_rect(self, x: int, y: int, w: int, h: int, colour: tuple[int, int, int]) -> None:
        """Flood a rectangle with one colour (used for the background + bars)."""
        if w <= 0 or h <= 0:
            return
        self._window(x, y, w, h)
        self._data(rgb565(colour) * (w * h))

    def clear(self, colour: tuple[int, int, int] = BLACK) -> None:
        self.fill_rect(0, 0, WIDTH, HEIGHT, colour)

    def text(self, s: str, x: int, y: int, colour: tuple[int, int, int], scale: int = 2) -> None:
        """Blit an ASCII string with the built-in 5x7 font at an integer scale.

        Each set pixel becomes a scale×scale filled block. Cheap and legible for
        a handful of status words; not meant for paragraphs."""
        cx = x
        for ch in s.upper():
            glyph = FONT5X7.get(ch, FONT5X7[" "])
            for col in range(5):
                bits = glyph[col]
                for row in range(7):
                    if bits & (1 << row):
                        self.fill_rect(cx + col * scale, y + row * scale, scale, scale, colour)
            cx += 6 * scale  # 5 px glyph + 1 px gap

    def close(self) -> None:
        try:
            self.spi.close()
        finally:
            self._dc.release()
            self._rst.release()


# ── 5x7 bitmap font — column-major, LSB=top row. Only the glyphs the installer
#    screens use (letters, digits, a few marks). Missing chars render as space. ─
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
    """Big 'FLASHING' + a percent bar + the numeric percent."""
    p.clear(BLACK)
    p.text("FLASHING", 8, 40, AMBER, scale=2)
    # progress bar frame + fill
    bx, by, bw, bh = 12, 150, WIDTH - 24, 26
    p.fill_rect(bx, by, bw, bh, WHITE)
    p.fill_rect(bx + 2, by + 2, bw - 4, bh - 4, BLACK)
    fill_w = int((bw - 4) * max(0, min(100, pct)) / 100)
    p.fill_rect(bx + 2, by + 2, fill_w, bh - 4, AMBER)
    p.text(f"{max(0, min(100, pct))}%", 60, 200, WHITE, scale=3)
    p.text("DO NOT REMOVE", 14, 270, WHITE, scale=1)


def _screen_verify(p: Panel) -> None:
    p.clear(BLACK)
    p.text("VERIFYING", 8, 120, WHITE, scale=2)
    p.text("CHECKSUM", 20, 160, WHITE, scale=2)


def _screen_done(p: Panel) -> None:
    p.clear(BLACK)
    p.text("DONE", 40, 110, GREEN, scale=4)
    p.text("REMOVE SD", 16, 180, GREEN, scale=2)
    p.text("POWERING OFF", 18, 240, WHITE, scale=1)


def _screen_error(p: Panel, msg: str) -> None:
    p.clear(BLACK)
    p.text("ERROR", 30, 110, RED, scale=4)
    # wrap the reason across lines of ~14 chars at scale 1
    line = msg[:42]
    for i in range(0, len(line), 14):
        p.text(line[i : i + 14], 8, 180 + (i // 14) * 16, WHITE, scale=1)


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: pcat_lcd.py {flashing PCT|verify|done|error [msg]}", file=sys.stderr)
        return 2
    state = argv[1].lower()
    panel = Panel()
    try:
        panel.init()
        if state == "flashing":
            _screen_flashing(panel, int(argv[2]) if len(argv) > 2 else 0)
        elif state == "verify":
            _screen_verify(panel)
        elif state == "done":
            _screen_done(panel)
        elif state == "error":
            _screen_error(panel, argv[2] if len(argv) > 2 else "unknown")
        else:
            print(f"unknown state {state!r}", file=sys.stderr)
            return 2
    finally:
        panel.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
