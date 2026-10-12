//! Univer `IStyleData` → umya `Style`, applied attribute by attribute.
//!
//! Only the attributes that differ between the original conversion and the
//! snapshot are written, so whatever Univer can't express (theme colours,
//! pattern fills, diagonal borders, protection) stays as the file had it.

use cp_base::cast::Safe as _;
use serde_json::{Map, Value};
use umya_spreadsheet::{BorderStyleValues, Color, HorizontalAlignmentValues, Style, VerticalAlignmentValues};

/// Pixels per indent level (matches the converter's `pd.l`).
const INDENT_PX: f64 = 9.0;

/// Write every attribute of `new` that differs from `old` (absent = reset).
pub(super) fn patch(style: &mut Style, old: &Map<String, Value>, new: &Map<String, Value>) {
    let keys: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for key in keys {
        let value = new.get(key.as_str());
        if old.get(key.as_str()) != value {
            apply(style, key, value.unwrap_or(&Value::Null));
        }
    }
}

/// Apply one attribute; `Null` resets it to Excel's default.
fn apply(style: &mut Style, key: &str, v: &Value) {
    match key {
        "ff" | "fs" | "cl" | "bl" | "it" | "ul" | "st" => font(style, key, v),
        "bg" => fill(style, v),
        "bd" => borders(style, v),
        "ht" | "vt" | "tb" | "tr" | "pd" => alignment(style, key, v),
        "n" => {
            let code = v.get("pattern").and_then(Value::as_str).unwrap_or("General");
            let _n = style.number_format_mut().set_format_code(code);
        }
        _ => {}
    }
}

/// Font face, size, colour and marks.
fn font(style: &mut Style, key: &str, v: &Value) {
    let f = style.font_mut();
    if key == "cl" {
        let _c = f.color_mut().set_argb_str(argb(v).unwrap_or_else(|| "FF000000".to_owned()));
        return;
    }
    let on = flag(v);
    let _f = match key {
        "ff" => f.set_name(v.as_str().unwrap_or("Calibri")),
        "fs" => f.set_size(v.as_f64().unwrap_or(11.0f64)),
        "bl" => f.set_bold(on),
        "it" => f.set_italic(on),
        "ul" => f.set_underline(if on { "single" } else { "none" }),
        _ => f.set_strikethrough(on),
    };
}

/// `1` / `{ s: 1 }` → true.
fn flag(v: &Value) -> bool {
    v.get("s").unwrap_or(v).as_u64() == Some(1)
}

/// `{ rgb: "#RRGGBB" }` → `FFRRGGBB`.
fn argb(v: &Value) -> Option<String> {
    let rgb = v.get("rgb").and_then(Value::as_str)?.trim_start_matches('#');
    (rgb.len() == 6).then(|| format!("FF{}", rgb.to_uppercase()))
}

/// Solid background, or no fill.
fn fill(style: &mut Style, v: &Value) {
    let _s = match argb(v) {
        Some(color) => style.set_background_color_solid(color),
        None => style.remove_fill(),
    };
}

/// Top/bottom/left/right edges.
fn borders(style: &mut Style, v: &Value) {
    let b = style.borders_mut();
    for key in ["t", "b", "l", "r"] {
        let edge = match key {
            "t" => b.top_mut(),
            "b" => b.bottom_mut(),
            "l" => b.left_mut(),
            _ => b.right_mut(),
        };
        let spec = v.get(key).unwrap_or(&Value::Null);
        let _s = edge.set_style(border_style(spec.get("s").and_then(Value::as_u64).unwrap_or(0)));
        let mut color = Color::default();
        let _c =
            color.set_argb_str(argb(spec.get("cl").unwrap_or(&Value::Null)).unwrap_or_else(|| "FF000000".to_owned()));
        let _e = edge.set_color(color);
    }
}

/// Univer `BorderStyleTypes` → Excel border style (inverse of the converter).
const fn border_style(s: u64) -> BorderStyleValues {
    match s {
        1 => BorderStyleValues::Thin,
        2 => BorderStyleValues::Hair,
        3 => BorderStyleValues::Dotted,
        4 => BorderStyleValues::Dashed,
        5 => BorderStyleValues::DashDot,
        6 => BorderStyleValues::DashDotDot,
        7 => BorderStyleValues::Double,
        8 => BorderStyleValues::Medium,
        9 => BorderStyleValues::MediumDashed,
        10 => BorderStyleValues::MediumDashDot,
        11 => BorderStyleValues::MediumDashDotDot,
        12 => BorderStyleValues::SlantDashDot,
        13 => BorderStyleValues::Thick,
        _ => BorderStyleValues::None,
    }
}

/// Horizontal/vertical alignment, wrap, rotation, indent.
fn alignment(style: &mut Style, key: &str, v: &Value) {
    let a = style.alignment_mut();
    match key {
        "ht" => a.set_horizontal(horizontal(v.as_u64().unwrap_or(0))),
        "vt" => a.set_vertical(vertical(v.as_u64().unwrap_or(3))),
        "tb" => a.set_wrap_text(v.as_u64() == Some(3)),
        "tr" => a.set_text_rotation(rotation(v)),
        _ => a.set_indent(indent(v)),
    }
}

/// Univer `HorizontalAlign` → Excel.
const fn horizontal(h: u64) -> HorizontalAlignmentValues {
    match h {
        1 => HorizontalAlignmentValues::Left,
        2 => HorizontalAlignmentValues::Center,
        3 => HorizontalAlignmentValues::Right,
        4 => HorizontalAlignmentValues::Justify,
        6 => HorizontalAlignmentValues::Distributed,
        _ => HorizontalAlignmentValues::General,
    }
}

/// Univer `VerticalAlign` → Excel (bottom by default).
const fn vertical(v: u64) -> VerticalAlignmentValues {
    match v {
        1 => VerticalAlignmentValues::Top,
        2 => VerticalAlignmentValues::Center,
        _ => VerticalAlignmentValues::Bottom,
    }
}

/// Univer `{ a, v }` → Excel `textRotation` (255 = stacked, 91–180 = clockwise).
fn rotation(v: &Value) -> u32 {
    if v.get("v").and_then(Value::as_u64) == Some(1) {
        return 255;
    }
    let angle = v.get("a").and_then(Value::as_i64).unwrap_or(0).clamp(-90, 90);
    let excel = if angle < 0 { 90i64.saturating_sub(angle) } else { angle };
    u32::try_from(excel).unwrap_or(0)
}

/// Left padding in pixels → indent levels.
fn indent(v: &Value) -> u32 {
    let px = v.get("l").and_then(Value::as_f64).unwrap_or(0.0f64);
    u32::try_from(px.mul_add(INDENT_PX.recip(), 0.0).round().to_i64()).unwrap_or(0)
}
