//! One Excel worksheet → Univer `IWorksheetData` (T851).
//!
//! Sizes are converted from Excel units to the pixels Univer draws with:
//! column width in characters (`px ≈ chars × 7 + 5`, Calibri 11 max-digit
//! width) and row height in points (`px = pt × 4/3`).

use std::collections::HashMap;

use cp_base::cast::Safe as _;
use serde_json::{Map, Value, json};
use umya_spreadsheet::drawing::Theme;
use umya_spreadsheet::{Cell, CellRawValue, PaneStateValues, Range, SheetStateValues, Worksheet};

use super::rich::{LinkCtx, add_link, rich_doc};
use super::style::{StyleRegistry, color_hex};

/// Rows kept per sheet; deeper rows are dropped and the sheet flagged truncated.
pub(super) const MAX_ROWS: u32 = 100_000;

/// Columns kept per sheet; wider columns are dropped and flagged truncated.
pub(super) const MAX_COLS: u32 = 500;

/// Points → pixels (96 dpi / 72 pt).
const PT_TO_PX: f64 = 1.333_333_333_333_333_3;

/// Excel's default column width (characters) when the sheet declares none.
const DEFAULT_COL_CHARS: f64 = 8.43;

/// Excel's default row height (points) when the sheet declares none.
const DEFAULT_ROW_PT: f64 = 15.0;

/// Column width in characters → pixels.
fn col_px(chars: f64) -> i64 {
    chars.mul_add(7.0, 5.0).round().to_i64()
}

/// Row height in points → pixels.
fn row_px(points: f64) -> i64 {
    points.mul_add(PT_TO_PX, 0.0).round().to_i64()
}

/// A converted sheet plus whether any cap clipped it.
pub(super) struct SheetOut {
    /// The `IWorksheetData` object.
    pub data: Value,
    /// `true` when rows/columns beyond [`MAX_ROWS`] × [`MAX_COLS`] were dropped.
    pub truncated: bool,
}

/// Workbook-wide inputs shared by every sheet conversion.
pub(super) struct Ctx<'ctx> {
    /// Workbook theme, for theme-relative colours.
    pub theme: &'ctx Theme,
    /// Interned style table.
    pub styles: StyleRegistry,
    /// Sheet name → Univer sheet id, for links inside the workbook.
    pub sheet_ids: HashMap<String, String>,
}

/// Convert one worksheet. `id` is the Univer sheet id (`sheet-<n>`).
pub(super) fn sheet_json(ws: &Worksheet, id: &str, ctx: &mut Ctx<'_>) -> SheetOut {
    let theme = ctx.theme;
    let (max_col, max_row) = ws.highest_column_and_row();
    let truncated = max_row > MAX_ROWS || max_col > MAX_COLS;
    let (default_w, default_h) = default_sizes(ws);
    let mut data = json!({
        "id": id,
        "name": ws.name(),
        "hidden": u8::from(!matches!(*ws.state(), SheetStateValues::Visible)),
        "rowCount": max_row.min(MAX_ROWS).saturating_add(100).max(1000),
        "columnCount": max_col.min(MAX_COLS).saturating_add(10).max(26),
        "defaultColumnWidth": default_w,
        "defaultRowHeight": default_h,
        "freeze": freeze(ws),
        "mergeData": merges(ws),
        "rowData": row_data(ws),
        "columnData": column_data(ws),
        "cellData": cell_data(ws, id, ctx),
    });
    if let Some(obj) = data.as_object_mut() {
        view_part(obj, ws, theme);
    }
    SheetOut { data, truncated }
}

/// Default column width / row height in pixels.
fn default_sizes(ws: &Worksheet) -> (i64, i64) {
    let fmt = ws.sheet_format_properties();
    let w = Some(fmt.default_column_width()).filter(|w| *w > 0.0f64).unwrap_or(DEFAULT_COL_CHARS);
    let h = Some(fmt.default_row_height()).filter(|h| *h > 0.0f64).unwrap_or(DEFAULT_ROW_PT);
    (col_px(w), row_px(h))
}

/// Tab colour, gridlines and zoom from the first sheet view.
fn view_part(obj: &mut Map<String, Value>, ws: &Worksheet, theme: &Theme) {
    if let Some(tab) = ws.tab_color().and_then(|c| color_hex(c, theme)) {
        let _prev = obj.insert("tabColor".to_owned(), json!(tab));
    }
    let Some(view) = ws.sheets_views().sheet_view_list().first() else {
        return;
    };
    let _grid = obj.insert("showGridlines".to_owned(), json!(u8::from(view.show_grid_lines())));
    let zoom = view.zoom_scale();
    if zoom > 0 && zoom != 100 {
        let _zoom = obj.insert("zoomRatio".to_owned(), json!(f64::from(zoom).mul_add(0.01, 0.0)));
    }
}

/// Frozen panes → Univer `IFreeze` (`startRow/startColumn = -1` when none).
fn freeze(ws: &Worksheet) -> Value {
    let pane = ws
        .sheets_views()
        .sheet_view_list()
        .first()
        .and_then(|v| v.pane())
        .filter(|p| matches!(*p.state(), PaneStateValues::Frozen | PaneStateValues::FrozenSplit));
    let Some(frozen) = pane else {
        return json!({ "xSplit": 0, "ySplit": 0, "startRow": -1, "startColumn": -1 });
    };
    let cols = frozen.horizontal_split().round().to_i64();
    let rows = frozen.vertical_split().round().to_i64();
    json!({
        "xSplit": cols,
        "ySplit": rows,
        "startRow": if rows > 0 { rows } else { -1 },
        "startColumn": if cols > 0 { cols } else { -1 },
    })
}

/// Merged ranges → Univer `IRange` list (0-based, inclusive).
fn merges(ws: &Worksheet) -> Vec<Value> {
    ws.merge_cells().iter().filter_map(merge_range).collect()
}

/// One merged range; `None` for whole-row/column or out-of-bounds merges.
fn merge_range(range: &Range) -> Option<Value> {
    let r0 = range.coordinate_start_row()?.num();
    let c0 = range.coordinate_start_col()?.num();
    let r1 = range.coordinate_end_row()?.num();
    let c1 = range.coordinate_end_col()?.num();
    if r0 == 0 || c0 == 0 || r0 > MAX_ROWS || c0 > MAX_COLS {
        return None;
    }
    Some(json!({
        "startRow": r0.saturating_sub(1),
        "startColumn": c0.saturating_sub(1),
        "endRow": r1.min(MAX_ROWS).saturating_sub(1),
        "endColumn": c1.min(MAX_COLS).saturating_sub(1),
    }))
}

/// Custom heights and hidden rows, keyed by 0-based row index.
fn row_data(ws: &Worksheet) -> Map<String, Value> {
    let mut out = Map::new();
    for row in ws.row_dimensions() {
        let n = row.row_num();
        if n == 0 || n > MAX_ROWS {
            continue;
        }
        let mut entry = Map::new();
        if row.custom_height() && row.height() > 0.0f64 {
            let _h = entry.insert("h".to_owned(), json!(row_px(row.height())));
        }
        if row.hidden() {
            let _hd = entry.insert("hd".to_owned(), json!(1u8));
        }
        if !entry.is_empty() {
            let _prev = out.insert(n.saturating_sub(1).to_string(), Value::Object(entry));
        }
    }
    out
}

/// Column widths and hidden columns, keyed by 0-based column index.
fn column_data(ws: &Worksheet) -> Map<String, Value> {
    let mut out = Map::new();
    for col in ws.column_dimensions() {
        let n = col.col_num();
        if n == 0 || n > MAX_COLS {
            continue;
        }
        let mut entry = Map::new();
        if col.width() > 0.0f64 {
            let _w = entry.insert("w".to_owned(), json!(col_px(col.width())));
        }
        if col.hidden() {
            let _hd = entry.insert("hd".to_owned(), json!(1u8));
        }
        if !entry.is_empty() {
            let _prev = out.insert(n.saturating_sub(1).to_string(), Value::Object(entry));
        }
    }
    out
}

/// Sparse `cellData` matrix: `{ row: { col: ICellData } }`, 0-based.
///
/// Cells are walked in (row, col) order: umya stores them in a `HashMap`, and
/// style ids are handed out on first sight, so an unordered walk would give a
/// different `styles` table on every read and the save diff would see every
/// sheet as edited.
fn cell_data(ws: &Worksheet, sheet_id: &str, ctx: &mut Ctx<'_>) -> Map<String, Value> {
    let mut rows: Map<String, Value> = Map::new();
    let mut cells = ws.cells();
    cells.sort_by_key(|cell| (cell.coordinate().row_num(), cell.coordinate().col_num()));
    for cell in cells {
        let coord = cell.coordinate();
        let (r, c) = (coord.row_num(), coord.col_num());
        if r == 0 || c == 0 || r > MAX_ROWS || c > MAX_COLS {
            continue;
        }
        let entry = cell_entry(cell, &format!("link-{sheet_id}-{r}-{c}"), ctx);
        if entry.is_empty() {
            continue;
        }
        let row = rows.entry(r.saturating_sub(1).to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = row.as_object_mut() {
            let _prev = obj.insert(c.saturating_sub(1).to_string(), Value::Object(entry));
        }
    }
    rows
}

/// One cell's `ICellData`: value, formula, style id and hyperlink.
fn cell_entry(cell: &Cell, link_id: &str, ctx: &mut Ctx<'_>) -> Map<String, Value> {
    let mut entry = value_part(cell.cell_value().raw_value(), ctx.theme);
    let formula = cell.formula();
    if !formula.is_empty() {
        let _f = entry.insert("f".to_owned(), json!(format!("={formula}")));
    }
    if let Some(sid) = ctx.styles.intern(cell.style(), ctx.theme) {
        let _s = entry.insert("s".to_owned(), json!(sid));
    }
    if let Some(link) = cell.hyperlink() {
        add_link(&mut entry, link, &LinkCtx { sheet_ids: &ctx.sheet_ids, range_id: link_id.to_owned() });
    }
    entry
}

/// Cached value → `{ v, t }` (Univer `CellValueType`: 1 string, 2 number, 3 bool);
/// rich text also gets its per-run document `p`.
fn value_part(raw: &CellRawValue, theme: &Theme) -> Map<String, Value> {
    let mut out = Map::new();
    let (v, t) = cp_macros::deref_match!(raw, {
        CellRawValue::Empty => return out,
        CellRawValue::Numeric(n) => (json!(n), 2u8),
        CellRawValue::Bool(b) => (json!(u8::from(b)), 3u8),
        CellRawValue::String(ref s) | CellRawValue::Lazy(ref s) => (json!(s), 1u8),
        CellRawValue::RichText(ref rt) => {
            let _p = out.insert("p".to_owned(), rich_doc(rt, theme));
            (json!(rt.text()), 1u8)
        }
        CellRawValue::Error(ref e) => (json!(e.to_string()), 1u8),
    });
    let _v = out.insert("v".to_owned(), v);
    let _t = out.insert("t".to_owned(), json!(t));
    out
}
