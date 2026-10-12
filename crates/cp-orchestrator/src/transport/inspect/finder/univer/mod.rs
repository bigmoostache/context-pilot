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

mod grid;
mod rich;
mod style;

use std::path::Path;

use cp_base::cast::Safe as _;
use serde_json::{Value, json};

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
    let mut styles = StyleRegistry::default();
    let mut order: Vec<String> = Vec::new();
    let mut sheets = serde_json::Map::new();
    let mut truncated = false;

    for (idx, ws) in book.sheet_collection().iter().enumerate() {
        let id = format!("sheet-{idx}");
        let out = grid::sheet_json(ws, &id, theme, &mut styles);
        truncated |= out.truncated;
        let _prev = sheets.insert(id.clone(), out.data);
        order.push(id);
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
        "styles": styles.into_table(),
        "sheets": sheets,
    });
    Some(Converted { workbook, active_sheet, truncated })
}

#[cfg(test)]
mod tests;
