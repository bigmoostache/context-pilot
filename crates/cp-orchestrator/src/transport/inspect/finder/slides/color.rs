//! `DrawingML` colours: the theme palette, the master's colour map, and the
//! colour transforms (`lumMod`, `lumOff`, `tint`, `shade`, `satMod`, `alpha`).
//!
//! Every colour leaves this module as CSS hex (`#rrggbb`, or `#rrggbbaa` when
//! translucent) so the frontend never interprets OOXML. Float math goes through
//! `cp_base::cast::float_math` (the audited `float_arithmetic` chokepoint).

use std::collections::BTreeMap;

use cp_base::cast::Safe as _;
use cp_base::cast::float_math as fm;

use super::package::Package;
use super::xml::Node;

/// OOXML percentages are expressed in thousandths of a percent.
const PCT: f64 = 100_000.0;

/// An sRGB colour with alpha, every channel in `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rgba {
    /// Red.
    pub red: f64,
    /// Green.
    pub green: f64,
    /// Blue.
    pub blue: f64,
    /// Opacity.
    pub alpha: f64,
}

impl Rgba {
    /// Parse `RRGGBB` hex.
    pub(super) fn hex6(raw: &str) -> Option<Self> {
        let value = u32::from_str_radix(raw.get(..6)?, 16).ok()?;
        let channel = |shift: u32| fm::div(f64::from(value.checked_shr(shift).unwrap_or(0) & 0xFF), 255.0);
        Some(Self { red: channel(16), green: channel(8), blue: channel(0), alpha: 1.0 })
    }

    /// CSS hex, alpha only when below 1.
    pub(super) fn css(self) -> String {
        let byte = |val: f64| fm::mul(val.clamp(0.0, 1.0), 255.0).round().to_u8();
        let (red, green, blue) = (byte(self.red), byte(self.green), byte(self.blue));
        if self.alpha < 0.998f64 {
            return format!("#{red:02x}{green:02x}{blue:02x}{:02x}", byte(self.alpha));
        }
        format!("#{red:02x}{green:02x}{blue:02x}")
    }
}

/// The deck's theme as seen from one slide master.
pub(super) struct Theme {
    /// Palette by scheme name (`dk1`, `lt1`, `accent1`, `hlink`, …).
    pub colors: BTreeMap<String, Rgba>,
    /// Master colour map: logical name (`tx1`, `bg1`) → palette name.
    pub map: BTreeMap<String, String>,
    /// Heading font (`+mj-lt`).
    pub major: String,
    /// Body font (`+mn-lt`).
    pub minor: String,
    /// `a:fillStyleLst` entries (for `fillRef idx` 1..).
    pub fills: Vec<Node>,
    /// `a:lnStyleLst` entries (for `lnRef idx`).
    pub lines: Vec<Node>,
    /// `a:bgFillStyleLst` entries (for `bgRef idx` 1001..).
    pub bg_fills: Vec<Node>,
}

impl Theme {
    /// Load the theme linked from `master_part`, with the master's `p:clrMap`.
    pub(super) fn load(pkg: &Package, master_part: &str, master: &Node) -> Self {
        let doc = pkg.rel_of_type(master_part, "/theme").and_then(|part| pkg.xml(&part));
        let elements = doc.as_ref().and_then(|node| node.child("a:themeElements"));
        let colors = elements.and_then(|el| el.child("a:clrScheme")).map(palette).unwrap_or_default();
        let fonts = elements.and_then(|el| el.child("a:fontScheme"));
        let face = |kind: &str| {
            fonts
                .and_then(|node| node.path(&format!("{kind}/a:latin")))
                .and_then(|node| node.attr("typeface"))
                .unwrap_or("Calibri")
                .to_owned()
        };
        let fmt = elements.and_then(|el| el.child("a:fmtScheme"));
        let list =
            |name: &str| fmt.and_then(|node| node.child(name)).map(|node| node.children.clone()).unwrap_or_default();
        let map = master.child("p:clrMap").map(|node| node.attrs.iter().cloned().collect()).unwrap_or_default();
        Self {
            colors,
            map,
            major: face("a:majorFont"),
            minor: face("a:minorFont"),
            fills: list("a:fillStyleLst"),
            lines: list("a:lnStyleLst"),
            bg_fills: list("a:bgFillStyleLst"),
        }
    }

    /// A theme with no palette (tests).
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            colors: BTreeMap::new(),
            map: BTreeMap::new(),
            major: "Calibri".to_owned(),
            minor: "Calibri".to_owned(),
            fills: Vec::new(),
            lines: Vec::new(),
            bg_fills: Vec::new(),
        }
    }

    /// Resolve a theme font reference (`+mj-lt` / `+mn-lt`) to a face name.
    pub(super) fn font(&self, face: &str) -> String {
        if face.starts_with("+mj") {
            return self.major.clone();
        }
        if face.starts_with("+mn") {
            return self.minor.clone();
        }
        face.to_owned()
    }

    /// The colour held by `holder` (its first colour child, e.g. inside
    /// `a:solidFill`), with `ph` standing in for `phClr`.
    pub(super) fn color(&self, holder: &Node, ph: Option<Rgba>) -> Option<Rgba> {
        holder.children.iter().find_map(|node| self.base(node, ph).map(|base| transform(base, node)))
    }

    /// The untransformed colour of one colour element.
    fn base(&self, node: &Node, ph: Option<Rgba>) -> Option<Rgba> {
        match node.name.as_str() {
            "a:srgbClr" => Rgba::hex6(node.attr("val")?),
            "a:sysClr" => Rgba::hex6(node.attr("lastClr").unwrap_or("000000")),
            "a:schemeClr" => self.scheme(node.attr("val")?, ph),
            "a:prstClr" => preset(node.attr("val")?),
            "a:scrgbClr" => Some(Rgba { red: pct(node, "r"), green: pct(node, "g"), blue: pct(node, "b"), alpha: 1.0 }),
            "a:hslClr" => {
                let hue = fm::div(node.int("hue").unwrap_or(0).to_f64(), 60_000.0);
                Some(from_hsl(hue, pct(node, "sat"), pct(node, "lum"), 1.0))
            }
            _ => None,
        }
    }

    /// A scheme colour by name, through the master colour map.
    fn scheme(&self, name: &str, ph: Option<Rgba>) -> Option<Rgba> {
        if name == "phClr" {
            return ph;
        }
        let mapped = self.map.get(name).map_or(name, String::as_str);
        self.colors.get(mapped).copied()
    }
}

/// Read `a:clrScheme` into a name → colour palette.
fn palette(scheme: &Node) -> BTreeMap<String, Rgba> {
    scheme
        .children
        .iter()
        .filter_map(|slot| {
            let inner = slot.children.first()?;
            let hex = inner.attr("val").filter(|_| inner.name == "a:srgbClr").or_else(|| inner.attr("lastClr"))?;
            Some((slot.local().to_owned(), Rgba::hex6(hex)?))
        })
        .collect()
}

/// Integer percentage attribute as a `0..=1` fraction.
fn pct(node: &Node, key: &str) -> f64 {
    fm::div(node.int(key).unwrap_or(0).to_f64(), PCT)
}

/// Apply every transform child of a colour element, in document order.
fn transform(base: Rgba, node: &Node) -> Rgba {
    node.children.iter().fold(base, |color, op| {
        let val = pct(op, "val");
        match op.local() {
            "alpha" => Rgba { alpha: val, ..color },
            "alphaMod" => Rgba { alpha: fm::mul(color.alpha, val), ..color },
            "tint" => channels(color, |chan| fm::sub(1.0, fm::mul(fm::sub(1.0, chan), val))),
            "shade" => channels(color, |chan| fm::mul(chan, val)),
            "lumMod" => hsl_edit(color, |hsl| (hsl.0, hsl.1, fm::mul(hsl.2, val))),
            "lumOff" => hsl_edit(color, |hsl| (hsl.0, hsl.1, fm::add(hsl.2, val))),
            "satMod" => hsl_edit(color, |hsl| (hsl.0, fm::mul(hsl.1, val), hsl.2)),
            _ => color,
        }
    })
}

/// Map each RGB channel.
fn channels(color: Rgba, op: impl Fn(f64) -> f64) -> Rgba {
    Rgba { red: op(color.red), green: op(color.green), blue: op(color.blue), alpha: color.alpha }
}

/// Edit a colour in HSL space (hue in degrees, saturation/lightness `0..=1`).
fn hsl_edit(color: Rgba, op: impl Fn((f64, f64, f64)) -> (f64, f64, f64)) -> Rgba {
    let (hue, sat, lum) = op(to_hsl(color));
    from_hsl(hue, sat.clamp(0.0, 1.0), lum.clamp(0.0, 1.0), color.alpha)
}

/// RGB → HSL.
fn to_hsl(color: Rgba) -> (f64, f64, f64) {
    let max = color.red.max(color.green).max(color.blue);
    let min = color.red.min(color.green).min(color.blue);
    let lum = fm::mul(fm::add(max, min), 0.5);
    let delta = fm::sub(max, min);
    if delta < 1e-9f64 {
        return (0.0f64, 0.0f64, lum);
    }
    let sat =
        if lum > 0.5f64 { fm::div(delta, fm::sub(2.0, fm::add(max, min))) } else { fm::div(delta, fm::add(max, min)) };
    let sector = if fm::abs_diff(max, color.red) < 1e-9f64 {
        fm::div(fm::sub(color.green, color.blue), delta).rem_euclid(6.0)
    } else if fm::abs_diff(max, color.green) < 1e-9f64 {
        fm::add(fm::div(fm::sub(color.blue, color.red), delta), 2.0)
    } else {
        fm::add(fm::div(fm::sub(color.red, color.green), delta), 4.0)
    };
    (fm::mul(sector, 60.0), sat, lum)
}

/// Zero channel.
const ZERO: f64 = 0.0;

/// HSL → RGB.
fn from_hsl(hue: f64, sat: f64, lum: f64, alpha: f64) -> Rgba {
    let chroma = fm::mul(fm::sub(1.0, fm::abs_diff(fm::mul(lum, 2.0), 1.0)), sat);
    let sector = fm::div(hue.rem_euclid(360.0), 60.0);
    let second = fm::mul(chroma, fm::sub(1.0, fm::abs_diff(sector.rem_euclid(2.0), 1.0)));
    let base = fm::sub(lum, fm::mul(chroma, 0.5));
    let (red, green, blue) = match sector.floor().to_i64() {
        0 => (chroma, second, ZERO),
        1 => (second, chroma, ZERO),
        2 => (ZERO, chroma, second),
        3 => (ZERO, second, chroma),
        4 => (second, ZERO, chroma),
        _ => (chroma, ZERO, second),
    };
    Rgba { red: fm::add(red, base), green: fm::add(green, base), blue: fm::add(blue, base), alpha }
}

/// The common `a:prstClr` names; unknown names fall back to black.
fn preset(name: &str) -> Option<Rgba> {
    let hex = match name {
        "white" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "orange" => "FFA500",
        "gray" | "grey" => "808080",
        "ltGray" => "D3D3D3",
        "dkGray" => "A9A9A9",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "purple" => "800080",
        "navy" => "000080",
        _ => "000000",
    };
    Rgba::hex6(hex)
}

#[cfg(test)]
mod tests {
    use super::{Rgba, from_hsl, to_hsl, transform};
    use crate::transport::inspect::finder::slides::xml::parse;

    #[test]
    fn hex_round_trips() {
        assert_eq!(Rgba::hex6("6C5CE7").unwrap().css(), "#6c5ce7");
    }

    #[test]
    fn hsl_round_trips() {
        let color = Rgba::hex6("3366CC").unwrap();
        let (hue, sat, lum) = to_hsl(color);
        assert_eq!(from_hsl(hue, sat, lum, 1.0).css(), "#3366cc");
    }

    #[test]
    fn lum_mod_off_and_alpha() {
        let node = parse(br#"<a:schemeClr val="tx1"><a:lumMod val="50000"/><a:lumOff val="50000"/><a:alpha val="50000"/></a:schemeClr>"#).unwrap();
        let out = transform(Rgba::hex6("000000").unwrap(), &node);
        assert_eq!(out.css(), "#80808080");
    }
}
