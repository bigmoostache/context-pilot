//! Excel cell style → Univer `IStyleData` (T851).
//!
//! Styles are interned in a [`StyleRegistry`]: identical style objects share
//! one id (`s0`, `s1`, …) in the workbook-level `styles` table, so a sheet with
//! 100k identically formatted cells ships one style, not 100k copies.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use umya_spreadsheet::drawing::Theme;
use umya_spreadsheet::{
    Alignment, Border, BorderStyleValues, Color, Font, HorizontalAlignmentValues, NumberingFormat, PatternValues,
    Style, VerticalAlignmentValues,
};

/// Univer `BooleanNumber.TRUE`.
const TRUE: u8 = 1;

/// Interns style objects; hands out stable ids for the workbook `styles` table.
#[derive(Default)]
pub(super) struct StyleRegistry {
    /// Serialized style → its id, for dedup.
    ids: HashMap<String, String>,
    /// id → style object, emitted as the workbook `styles` map.
    table: Map<String, Value>,
}

impl StyleRegistry {
    /// Intern `style`; returns its id, or `None` when it carries no attribute.
    pub(super) fn intern(&mut self, style: &Style, theme: &Theme) -> Option<String> {
        let data = style_data(style, theme)?;
        let key = Value::Object(data.clone()).to_string();
        if let Some(id) = self.ids.get(&key) {
            return Some(id.clone());
        }
        let id = format!("s{}", self.table.len());
        let _prev = self.table.insert(id.clone(), Value::Object(data));
        let _prev_id = self.ids.insert(key, id.clone());
        Some(id)
    }

    /// The interned `styles` table, consumed into the workbook JSON.
    pub(super) fn into_table(self) -> Map<String, Value> {
        self.table
    }
}

/// Build one `IStyleData` object; `None` when the style sets nothing.
fn style_data(style: &Style, theme: &Theme) -> Option<Map<String, Value>> {
    let mut out = Map::new();
    if let Some(font) = style.font() {
        font_part(&mut out, font, theme);
    }
    if let Some(bg) = solid_fill(style).and_then(|c| color_hex(c, theme)) {
        let _prev = out.insert("bg".to_owned(), json!({ "rgb": bg }));
    }
    border_part(&mut out, style, theme);
    if let Some(alignment) = style.alignment() {
        align_part(&mut out, alignment);
    }
    if let Some(code) =
        style.number_format().map(NumberingFormat::format_code).filter(|c| !c.eq_ignore_ascii_case("general"))
    {
        let _prev = out.insert("n".to_owned(), json!({ "pattern": code }));
    }
    (!out.is_empty()).then_some(out)
}

/// Foreground colour of a solid pattern fill (Excel's plain cell background).
fn solid_fill(style: &Style) -> Option<&Color> {
    let pattern = style.fill()?.pattern_fill()?;
    match *pattern.pattern_type() {
        PatternValues::Solid => pattern.foreground_color(),
        PatternValues::DarkDown
        | PatternValues::DarkGray
        | PatternValues::DarkGrid
        | PatternValues::DarkHorizontal
        | PatternValues::DarkTrellis
        | PatternValues::DarkUp
        | PatternValues::DarkVertical
        | PatternValues::Gray0625
        | PatternValues::Gray125
        | PatternValues::LightDown
        | PatternValues::LightGray
        | PatternValues::LightGrid
        | PatternValues::LightHorizontal
        | PatternValues::LightTrellis
        | PatternValues::LightUp
        | PatternValues::LightVertical
        | PatternValues::MediumGray => pattern.foreground_color().or_else(|| pattern.background_color()),
        PatternValues::None => None,
    }
}

/// Font face (family, size, colour) then marks (weight, slant, lines).
/// Shared with rich-text runs, whose `ts` uses the same keys.
pub(super) fn font_part(out: &mut Map<String, Value>, font: &Font, theme: &Theme) {
    if !font.name().is_empty() {
        let _prev = out.insert("ff".to_owned(), json!(font.name()));
    }
    if font.size() > 0.0f64 {
        let _prev = out.insert("fs".to_owned(), json!(font.size()));
    }
    if let Some(c) = color_hex(font.color(), theme) {
        let _prev = out.insert("cl".to_owned(), json!({ "rgb": c }));
    }
    font_marks(out, font);
}

/// Bold, italic, underline, strikethrough flags.
fn font_marks(out: &mut Map<String, Value>, font: &Font) {
    let underline = !matches!(font.underline(), "" | "none");
    for (key, on, value) in [
        ("bl", font.bold(), json!(TRUE)),
        ("it", font.italic(), json!(TRUE)),
        ("ul", underline, json!({ "s": TRUE })),
        ("st", font.strikethrough(), json!({ "s": TRUE })),
    ] {
        if on {
            let _prev = out.insert(key.to_owned(), value);
        }
    }
}

/// Top/bottom/left/right edges as Univer `IBorderData` (umya's `Borders`
/// type is crate-private, so the style is passed and read here).
fn border_part(out: &mut Map<String, Value>, style: &Style, theme: &Theme) {
    let Some(borders) = style.borders() else {
        return;
    };
    let mut bd = Map::new();
    for (key, edge) in [("t", borders.top()), ("b", borders.bottom()), ("l", borders.left()), ("r", borders.right())] {
        if let Some(v) = edge_data(edge, theme) {
            let _prev = bd.insert(key.to_owned(), v);
        }
    }
    if !bd.is_empty() {
        let _prev = out.insert("bd".to_owned(), Value::Object(bd));
    }
}

/// One border edge `{ s, cl }`; `None` when the edge is not drawn.
fn edge_data(edge: &Border, theme: &Theme) -> Option<Value> {
    let s = border_style(edge.style())?;
    let cl = edge.color().and_then(|c| color_hex(&c, theme)).unwrap_or_else(|| "#000000".to_owned());
    Some(json!({ "s": s, "cl": { "rgb": cl } }))
}

/// Excel border style → Univer `BorderStyleTypes`; `None` for no border.
const fn border_style(style: &BorderStyleValues) -> Option<u8> {
    match *style {
        BorderStyleValues::None => None,
        BorderStyleValues::Thin => Some(1),
        BorderStyleValues::Hair => Some(2),
        BorderStyleValues::Dotted => Some(3),
        BorderStyleValues::Dashed => Some(4),
        BorderStyleValues::DashDot => Some(5),
        BorderStyleValues::DashDotDot => Some(6),
        BorderStyleValues::Double => Some(7),
        BorderStyleValues::Medium => Some(8),
        BorderStyleValues::MediumDashed => Some(9),
        BorderStyleValues::MediumDashDot => Some(10),
        BorderStyleValues::MediumDashDotDot => Some(11),
        BorderStyleValues::SlantDashDot => Some(12),
        BorderStyleValues::Thick => Some(13),
    }
}

/// Horizontal/vertical alignment, wrap, rotation.
fn align_part(out: &mut Map<String, Value>, alignment: &Alignment) {
    let mut put = |k: &str, v: Value| {
        let _prev = out.insert(k.to_owned(), v);
    };
    if let Some(ht) = horizontal(alignment.horizontal()) {
        put("ht", json!(ht));
    }
    put("vt", json!(vertical(alignment.vertical())));
    if alignment.wrap_text() {
        put("tb", json!(3u8)); // WrapStrategy.WRAP
    }
    if let Some(tr) = rotation(alignment.text_rotation()) {
        put("tr", tr);
    }
    let indent = alignment.indent();
    if indent > 0 {
        // One Excel indent level ≈ 3 spaces of Calibri 11 ≈ 9 px.
        put("pd", json!({ "l": indent.saturating_mul(INDENT_PX) }));
    }
}

/// Pixels per Excel indent level.
const INDENT_PX: u32 = 9;

/// Excel horizontal alignment → Univer `HorizontalAlign`; `None` = General.
const fn horizontal(h: &HorizontalAlignmentValues) -> Option<u8> {
    match *h {
        HorizontalAlignmentValues::General | HorizontalAlignmentValues::Fill => None,
        HorizontalAlignmentValues::Left => Some(1),
        HorizontalAlignmentValues::Center | HorizontalAlignmentValues::CenterContinuous => Some(2),
        HorizontalAlignmentValues::Right => Some(3),
        HorizontalAlignmentValues::Justify => Some(4),
        HorizontalAlignmentValues::Distributed => Some(6),
    }
}

/// Excel vertical alignment → Univer `VerticalAlign` (Excel defaults to bottom).
const fn vertical(v: &VerticalAlignmentValues) -> u8 {
    match *v {
        VerticalAlignmentValues::Top => 1,
        VerticalAlignmentValues::Center | VerticalAlignmentValues::Justify | VerticalAlignmentValues::Distributed => 2,
        VerticalAlignmentValues::Bottom => 3,
    }
}

/// Excel `textRotation` → Univer `ITextRotation`.
///
/// Excel: 0–90 = counter-clockwise degrees, 91–180 = clockwise (`90 - x`),
/// 255 = vertical stacked text.
fn rotation(raw: u32) -> Option<Value> {
    match raw {
        0 => None,
        255 => Some(json!({ "a": 0i64, "v": TRUE })),
        1..=90 => Some(json!({ "a": raw })),
        _ => Some(json!({ "a": 0i64.saturating_sub(i64::from(raw.saturating_sub(90))) })),
    }
}

/// Resolve an Excel colour (rgb / indexed / theme + tint) to `#RRGGBB`.
/// `None` for automatic or unset colours, so Univer keeps its default.
pub(super) fn color_hex(color: &Color, theme: &Theme) -> Option<String> {
    if color.automatic() {
        return None;
    }
    let raw = color.argb_with_theme(theme);
    if raw.is_empty() || raw == "00000000" {
        return None;
    }
    // Theme colours come back as RRGGBB, explicit ones as AARRGGBB.
    let rgb: String = raw.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
    (rgb.len() == 6).then(|| format!("#{rgb}"))
}
