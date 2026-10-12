//! Full-fidelity `xlsx`/`xlsm` → Univer `IWorkbookData` converter (T851).
//!
//! The Finder spreadsheet viewer is Univer; this module hands it the whole
//! workbook in its native snapshot format, so the frontend only mounts it:
//! values, formulas, interned styles (fonts, fills, borders, alignment, wrap,
//! rotation, number formats), merges, frozen panes, column widths, row
//! heights, hidden rows/columns/sheets, tab colours, gridlines and zoom.
//!
//! Parsing uses `umya-spreadsheet` (calamine is values-only). Other formats
//! (`xls`/`xlsb`/`ods`/`csv`) stay on the calamine path in `sheet.rs`.

mod cf;
mod grid;
mod notes;
mod rich;
mod save;
mod style;

pub(super) use save::save;

use std::path::Path;

use cp_base::cast::Safe as _;
use serde_json::{Map, Value, json};

use grid::Ctx;
use style::StyleRegistry;

/// Largest workbook file converted (25 MiB); bigger files get a 413 so the UI
/// shows "too large" with a download button instead of a stalled preview.
pub(super) const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;

/// A converted workbook.
pub(super) struct Converted {
    /// Univer `IWorkbookData` snapshot.
    pub workbook: Value,
    /// Univer id of the sheet Excel opens on (the file's active tab).
    pub active_sheet: String,
    /// `true` when any sheet was clipped to the row/column caps.
    pub truncated: bool,
}

/// Convert an `xlsx`/`xlsm` file; `None` when umya can't read it.
pub(super) fn convert(path: &Path) -> Option<Converted> {
    let book = umya_spreadsheet::reader::xlsx::read(path).ok()?;
    let theme = book.theme();
    let wss = book.sheet_collection();
    let order: Vec<String> = (0..wss.len()).map(|idx| format!("sheet-{idx}")).collect();
    let sheet_ids = wss.iter().zip(&order).map(|(ws, id)| (ws.name().to_owned(), id.clone())).collect();
    let mut ctx = Ctx { theme, styles: StyleRegistry::default(), sheet_ids };
    let mut sheets = Map::new();
    let (mut rules, mut notes) = (Map::new(), Map::new());
    let mut truncated = false;

    for (ws, id) in wss.iter().zip(&order) {
        let out = grid::sheet_json(ws, id, &mut ctx);
        truncated |= out.truncated;
        let _prev = sheets.insert(id.clone(), out.data);
        let sheet_rules = cf::sheet_rules(ws, theme);
        if !sheet_rules.is_empty() {
            let _r = rules.insert(id.clone(), json!(sheet_rules));
        }
        let sheet_notes = notes::sheet_notes(ws);
        if !sheet_notes.is_empty() {
            let _n = notes.insert(id.clone(), Value::Object(sheet_notes));
        }
    }
    let first = order.first()?.clone();
    let active_idx = book.workbook_view().active_tab().to_usize();
    let active_sheet = order.get(active_idx).cloned().unwrap_or(first);

    let workbook = json!({
        "id": "workbook",
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or("workbook"),
        "appVersion": "",
        "locale": "frFR",
        "sheetOrder": order,
        "styles": ctx.styles.into_table(),
        "sheets": sheets,
        "resources": resources(rules, notes),
    });
    Some(Converted { workbook, active_sheet, truncated })
}

/// Plugin resources; Univer expects each `data` as a JSON string.
fn resources(rules: Map<String, Value>, notes: Map<String, Value>) -> Vec<Value> {
    [("SHEET_CONDITIONAL_FORMATTING_PLUGIN", rules), ("SHEET_NOTE_PLUGIN", notes)]
        .into_iter()
        .filter(|entry| !entry.1.is_empty())
        .map(|(name, data)| json!({ "name": name, "data": Value::Object(data).to_string() }))
        .collect()
}

#[cfg(test)]
mod tests;
