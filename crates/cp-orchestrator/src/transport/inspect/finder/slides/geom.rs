//! Shape outlines as SVG path data, in the shape's own EMU box (`0..w`, `0..h`).
//!
//! Covers the preset geometries decks actually use (rectangles, rounded and
//! snipped rectangles, ellipses, triangles, arrows, chevrons, polygons, lines)
//! honouring their `adj` handles, plus arbitrary `a:custGeom` paths. An unknown
//! preset falls back to its bounding rectangle, which is what `PowerPoint` shows
//! as the shape's frame anyway. Integer math only, saturating.

use std::fmt::Write as _;

use cp_base::cast::Safe as _;

use super::xml::Node;

/// `adj` values are in 1/100000 of the reference length.
const ADJ: i64 = 100_000;

/// Box size and adjust handles of one preset shape.
struct Frame {
    /// Width (EMU).
    w: i64,
    /// Height (EMU).
    h: i64,
    /// `adj`/`adj1` … handles, as written (1/100000 units).
    adj: [Option<i64>; 2],
}

impl Frame {
    /// The shorter side, the reference length of most handles.
    fn short(&self) -> i64 {
        self.w.min(self.h)
    }

    /// Handle `slot` applied to `base` (`base * adj / 100000`), default `dflt`.
    fn part(&self, slot: usize, dflt: i64, base: i64) -> i64 {
        let raw = self.adj.get(slot).copied().flatten().unwrap_or(dflt);
        frac(base, raw, ADJ)
    }
}

/// `value * num / den`, saturating, 0 on a zero denominator.
fn frac(value: i64, num: i64, den: i64) -> i64 {
    value.saturating_mul(num).checked_div(den).unwrap_or(0)
}

/// SVG path for `p:spPr` (preset or custom geometry) in a `w`×`h` box.
pub(super) fn outline(sp_pr: &Node, w: i64, h: i64) -> String {
    if let Some(cust) = sp_pr.child("a:custGeom") {
        return custom(cust, w, h);
    }
    let prst = sp_pr.child("a:prstGeom");
    let name = prst.and_then(|node| node.attr("prst")).unwrap_or("rect");
    let mut adj = [None, None];
    for gd in
        prst.and_then(|node| node.child("a:avLst")).map(|lst| lst.all("a:gd").collect::<Vec<_>>()).unwrap_or_default()
    {
        let slot = match gd.attr("name") {
            Some("adj" | "adj1") => 0,
            Some("adj2") => 1,
            _ => continue,
        };
        let val = gd.attr("fmla").and_then(|fmla| fmla.strip_prefix("val ")).and_then(|raw| raw.trim().parse().ok());
        if let Some(cell) = adj.get_mut(slot) {
            *cell = val;
        }
    }
    preset(name, &Frame { w, h, adj })
}

/// `true` for presets drawn as an open stroke (no fill area).
pub(super) fn is_line(sp_pr: &Node) -> bool {
    let name = sp_pr.path("a:prstGeom").and_then(|node| node.attr("prst")).unwrap_or("");
    name == "line" || name.contains("Connector")
}

/// Closed polygon through `pts`.
fn poly(pts: &[(i64, i64)]) -> String {
    let mut out = String::new();
    for (idx, pt) in pts.iter().enumerate() {
        let _w = write!(out, "{}{} {} ", if idx == 0 { "M" } else { "L" }, pt.0, pt.1);
    }
    out.push('Z');
    out
}

/// Preset name → path; shapes grouped by family to keep each table short.
fn preset(name: &str, fr: &Frame) -> String {
    basic(name, fr).or_else(|| arrows(name, fr)).unwrap_or_else(|| poly(&[(0, 0), (fr.w, 0), (fr.w, fr.h), (0, fr.h)]))
}

/// Rectangles, ellipses, triangles, polygons, lines.
fn basic(name: &str, fr: &Frame) -> Option<String> {
    let (w, h) = (fr.w, fr.h);
    let (hw, hh) = (frac(w, 1, 2), frac(h, 1, 2));
    Some(match name {
        "roundRect" => rounded(fr, fr.part(0, 16_667, fr.short())),
        "ellipse" | "flowChartConnector" => {
            format!("M0 {hh} A{hw} {hh} 0 1 0 {w} {hh} A{hw} {hh} 0 1 0 0 {hh} Z")
        }
        "triangle" | "flowChartExtract" => poly(&[(fr.part(0, 50_000, w), 0), (w, h), (0, h)]),
        "rtTriangle" => poly(&[(0, 0), (w, h), (0, h)]),
        "diamond" | "flowChartDecision" => poly(&[(hw, 0), (w, hh), (hw, h), (0, hh)]),
        "parallelogram" | "flowChartInputOutput" => {
            let dx = fr.part(0, 25_000, fr.short());
            poly(&[(dx, 0), (w, 0), (w.saturating_sub(dx), h), (0, h)])
        }
        "trapezoid" => {
            let dx = fr.part(0, 25_000, fr.short());
            poly(&[(dx, 0), (w.saturating_sub(dx), 0), (w, h), (0, h)])
        }
        "hexagon" => {
            let dx = fr.part(0, 25_000, fr.short());
            poly(&[(dx, 0), (w.saturating_sub(dx), 0), (w, hh), (w.saturating_sub(dx), h), (dx, h), (0, hh)])
        }
        "octagon" => octagon(fr),
        "plus" | "flowChartSummingJunction" => cross(fr),
        "snip1Rect" => {
            let cut = fr.part(0, 16_667, fr.short());
            poly(&[(0, 0), (w.saturating_sub(cut), 0), (w, cut), (w, h), (0, h)])
        }
        "line" | "straightConnector1" | "bentConnector2" | "bentConnector3" | "curvedConnector3" => {
            format!("M0 0 L{w} {h}")
        }
        _ => return None,
    })
}

/// Arrows, chevrons, pentagons.
fn arrows(name: &str, fr: &Frame) -> Option<String> {
    let (w, h) = (fr.w, fr.h);
    let hh = frac(h, 1, 2);
    Some(match name {
        "rightArrow" => horizontal_arrow(fr, false),
        "leftArrow" => horizontal_arrow(fr, true),
        "upArrow" | "downArrow" => vertical_arrow(fr, name == "upArrow"),
        "homePlate" | "pentagon" => {
            let tip = w.saturating_sub(fr.part(0, 50_000, fr.short()));
            poly(&[(0, 0), (tip, 0), (w, hh), (tip, h), (0, h)])
        }
        "chevron" => {
            let depth = fr.part(0, 50_000, fr.short());
            let tip = w.saturating_sub(depth);
            poly(&[(0, 0), (tip, 0), (w, hh), (tip, h), (0, h), (depth, hh)])
        }
        _ => return None,
    })
}

/// Rounded rectangle with corner radius `rad`.
fn rounded(fr: &Frame, rad: i64) -> String {
    let (w, h) = (fr.w, fr.h);
    let (right, bottom) = (w.saturating_sub(rad), h.saturating_sub(rad));
    format!(
        "M{rad} 0 L{right} 0 A{rad} {rad} 0 0 1 {w} {rad} L{w} {bottom} A{rad} {rad} 0 0 1 {right} {h} \
         L{rad} {h} A{rad} {rad} 0 0 1 0 {bottom} L0 {rad} A{rad} {rad} 0 0 1 {rad} 0 Z"
    )
}

/// Octagon with corner cut `adj` (default 29289).
fn octagon(fr: &Frame) -> String {
    let (w, h) = (fr.w, fr.h);
    let cut = fr.part(0, 29_289, fr.short());
    let (right, bottom) = (w.saturating_sub(cut), h.saturating_sub(cut));
    poly(&[(cut, 0), (right, 0), (w, cut), (w, bottom), (right, h), (cut, h), (0, bottom), (0, cut)])
}

/// Plus sign with arm inset `adj` (default 25000).
fn cross(fr: &Frame) -> String {
    let (w, h) = (fr.w, fr.h);
    let arm = fr.part(0, 25_000, fr.short());
    let (right, bottom) = (w.saturating_sub(arm), h.saturating_sub(arm));
    poly(&[
        (arm, 0),
        (right, 0),
        (right, arm),
        (w, arm),
        (w, bottom),
        (right, bottom),
        (right, h),
        (arm, h),
        (arm, bottom),
        (0, bottom),
        (0, arm),
        (arm, arm),
    ])
}

/// Left/right block arrow: `adj1` shaft thickness (of h), `adj2` head length.
fn horizontal_arrow(fr: &Frame, left: bool) -> String {
    let (w, h) = (fr.w, fr.h);
    let shaft = fr.part(0, 50_000, h);
    let head = fr.part(1, 50_000, fr.short());
    let top = frac(h.saturating_sub(shaft), 1, 2);
    let bottom = top.saturating_add(shaft);
    let hh = frac(h, 1, 2);
    let neck = w.saturating_sub(head);
    let pts = [(0, top), (neck, top), (neck, 0), (w, hh), (neck, h), (neck, bottom), (0, bottom)];
    let mirrored: Vec<(i64, i64)> =
        pts.iter().map(|pt| (if left { w.saturating_sub(pt.0) } else { pt.0 }, pt.1)).collect();
    poly(&mirrored)
}

/// Up/down block arrow (the horizontal arrow, transposed).
fn vertical_arrow(fr: &Frame, up: bool) -> String {
    let (w, h) = (fr.w, fr.h);
    let shaft = fr.part(0, 50_000, w);
    let head = fr.part(1, 50_000, fr.short());
    let left = frac(w.saturating_sub(shaft), 1, 2);
    let right = left.saturating_add(shaft);
    let hw = frac(w, 1, 2);
    let neck = h.saturating_sub(head);
    let pts = [(left, 0), (left, neck), (0, neck), (hw, h), (w, neck), (right, neck), (right, 0)];
    let mirrored: Vec<(i64, i64)> =
        pts.iter().map(|pt| (pt.0, if up { h.saturating_sub(pt.1) } else { pt.1 })).collect();
    poly(&mirrored)
}

/// `a:custGeom` paths, each scaled from its own `w`/`h` space into the box.
fn custom(cust: &Node, w: i64, h: i64) -> String {
    let mut out = String::new();
    for path in cust.path("a:pathLst").map(|lst| lst.all("a:path").collect::<Vec<_>>()).unwrap_or_default() {
        let scale = Scale { sx: (w, path.int("w").unwrap_or(w).max(1)), sy: (h, path.int("h").unwrap_or(h).max(1)) };
        for cmd in &path.children {
            emit(&mut out, cmd, &scale);
        }
    }
    out
}

/// Path-space → box-space ratios as (box, path) pairs.
struct Scale {
    /// Horizontal ratio.
    sx: (i64, i64),
    /// Vertical ratio.
    sy: (i64, i64),
}

impl Scale {
    /// Map an `a:pt` into box coordinates as `x y`.
    fn point(&self, pt: &Node) -> String {
        let x = frac(pt.int("x").unwrap_or(0), self.sx.0, self.sx.1);
        let y = frac(pt.int("y").unwrap_or(0), self.sy.0, self.sy.1);
        format!("{x} {y}")
    }
}

/// Append one custom-geometry command.
fn emit(out: &mut String, cmd: &Node, scale: &Scale) {
    let pts: Vec<String> = cmd.all("a:pt").map(|pt| scale.point(pt)).collect();
    let letter = match cmd.local() {
        "moveTo" => "M",
        "lnTo" => "L",
        "cubicBezTo" => "C",
        "quadBezTo" => "Q",
        "close" => {
            out.push_str("Z ");
            return;
        }
        "arcTo" => {
            arc(out, cmd, scale);
            return;
        }
        _ => return,
    };
    let _w = write!(out, "{letter}{} ", pts.join(" "));
}

/// `a:arcTo` as a relative SVG arc: the end point is computed from the radii
/// and the start/sweep angles (60000ths of a degree).
fn arc(out: &mut String, cmd: &Node, scale: &Scale) {
    let wr = frac(cmd.int("wR").unwrap_or(0), scale.sx.0, scale.sx.1);
    let hr = frac(cmd.int("hR").unwrap_or(0), scale.sy.0, scale.sy.1);
    let start = cmd.int("stAng").unwrap_or(0);
    let sweep = cmd.int("swAng").unwrap_or(0);
    let (sx, sy) = unit(start);
    let (ex, ey) = unit(start.saturating_add(sweep));
    let dx = frac(ex.saturating_sub(sx), wr, 1_000_000);
    let dy = frac(ey.saturating_sub(sy), hr, 1_000_000);
    let large = u8::from(sweep.saturating_abs() > 10_800_000);
    let dir = u8::from(sweep > 0);
    let _w = write!(out, "a{wr} {hr} 0 {large} {dir} {dx} {dy} ");
}

/// Point on the unit circle at an OOXML angle, scaled by 1e6.
fn unit(angle: i64) -> (i64, i64) {
    let degrees = angle.to_f64().to_radians().clamp(-1e9, 1e9);
    let rad = cp_base::cast::float_math::div(degrees, 60_000.0);
    let scale = |val: f64| cp_base::cast::float_math::mul(val, 1_000_000.0).round().to_i64();
    (scale(rad.cos()), scale(rad.sin()))
}

#[cfg(test)]
mod tests {
    use super::outline;
    use crate::transport::inspect::finder::slides::xml::parse;

    #[test]
    fn rect_and_round_rect() {
        let rect = parse(br#"<p:spPr><a:prstGeom prst="rect"/></p:spPr>"#).unwrap();
        assert_eq!(outline(&rect, 10, 20), "M0 0 L10 0 L10 20 L0 20 Z");
        let rr = parse(br#"<p:spPr><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 50000"/></a:avLst></a:prstGeom></p:spPr>"#).unwrap();
        assert!(outline(&rr, 100, 40).starts_with("M20 0 L80 0 A20 20"));
    }

    #[test]
    fn custom_path_scales() {
        let cust = parse(br#"<p:spPr><a:custGeom><a:pathLst><a:path w="2" h="2"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="2" y="1"/></a:lnTo><a:close/></a:path></a:pathLst></a:custGeom></p:spPr>"#).unwrap();
        assert_eq!(outline(&cust, 100, 50), "M0 0 L100 25 Z ");
    }
}
