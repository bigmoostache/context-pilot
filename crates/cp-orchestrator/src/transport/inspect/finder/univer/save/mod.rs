//! Formatting-preserving save: patch the original `xlsx`/`xlsm` with the
//! edits found in a Univer snapshot (T851 phase 4).
//!
//! The file is never rebuilt from the snapshot. The original is re-converted
//! (the exact JSON the viewer received), diffed against the incoming snapshot,
//! and only what changed is written into the umya workbook: cell values and
//! formulas, the style attributes that differ, merges, column widths, row
//! heights, hidden rows/columns, sheet names, added and deleted sheets.
//! Everything Univer doesn't model (charts, images, data validation, defined
//! names, untouched styles, conditional formats, notes) survives untouched.
//! The result is written to a sibling temp file, then renamed over the original.

mod cells;
mod layout;
mod lazy;
mod style;

use std::path::Path;
use std::sync::LazyLock;

use serde_json::{Map, Value};
use umya_spreadsheet::Workbook;

use cells::Styles;

/// Shared empty object for absent sheets / tables.
static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);

/// Univer sheet id paired with its umya sheet index.
type Slots = Vec<(String, usize)>;

/// Shared `null` for absent JSON nodes.
static NULL: Value = Value::Null;

/// Patch `path` with `snapshot`; returns the number of cells changed.
pub(in crate::transport::inspect::finder) fn save(path: &Path, snapshot: &Value) -> Result<usize, &'static str> {
    let base = super::convert_cached(path).ok_or("could not read the original workbook")?;
    let mut book =
        umya_spreadsheet::reader::xlsx::lazy_read(path).map_err(|_err| "could not read the original workbook")?;
    let patched = patch_book(&mut book, &base.workbook, snapshot)?;
    lazy::load_chart_sources(&mut book)?;
    write_atomic(&book, path)?;
    Ok(patched)
}

/// Apply every sheet's diff to `book`.
fn patch_book(book: &mut Workbook, base: &Value, new: &Value) -> Result<usize, &'static str> {
    let order: Vec<&str> = new
        .get("sheetOrder")
        .and_then(Value::as_array)
        .ok_or("snapshot has no sheetOrder")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let styles = Styles { base: table(base, "styles"), new: table(new, "styles") };
    let (base_sheets, new_sheets) = (table(base, "sheets"), table(new, "sheets"));
    let mut patched = 0usize;
    for (id, idx) in sheet_slots(book, &order, new_sheets)? {
        let old = base_sheets.get(&id).unwrap_or(&NULL);
        let sheet = new_sheets.get(&id).unwrap_or(&NULL);
        if !edited(old, sheet, &styles) {
            continue; // untouched: stays raw and is written back verbatim
        }
        let ws = book.sheet_mut(idx).map_err(|_err| "sheet vanished while saving")?;
        rename(ws, old, sheet);
        layout::patch(ws, old, sheet);
        patched = patched.saturating_add(cells::patch(ws, (old, sheet), &styles));
    }
    Ok(patched)
}

/// Whether the user changed anything the patch writes.
///
/// Raw JSON equality is useless here: Univer re-encodes every sheet (view
/// keys, recalculated formula results, filled-in line colours, auto heights),
/// and a sheet wrongly seen as edited gets deserialized and rewritten by umya.
/// A new sheet (no `old`) always counts.
fn edited(old: &Value, new: &Value, styles: &Styles<'_>) -> bool {
    old.is_null()
        || old.get("name") != new.get("name")
        || layout::edited(old, new)
        || cells::changed((old, new), styles)
}

/// Object field of a JSON node; empty when absent.
fn table<'json>(node: &'json Value, key: &str) -> &'json Map<String, Value> {
    node.get(key).and_then(Value::as_object).unwrap_or(&*EMPTY)
}

/// Rename the sheet when the user did.
fn rename(ws: &mut umya_spreadsheet::Worksheet, old: &Value, new: &Value) {
    let name = new.get("name").and_then(Value::as_str);
    if let Some(n) = name.filter(|n| !n.is_empty() && name != old.get("name").and_then(Value::as_str)) {
        let _ws = ws.set_name(n);
    }
}

/// Univer sheet id → current umya index, after deleting the sheets the user
/// removed and appending the ones they added. Original sheets carry the
/// converter's `sheet-<n>` ids; any other id is a sheet created in Univer.
fn sheet_slots(book: &mut Workbook, order: &[&str], new_sheets: &Map<String, Value>) -> Result<Slots, &'static str> {
    let kept = remove_deleted(book, order)?;
    let mut slots = Vec::new();
    for id in order {
        let existing = base_index(id).and_then(|b| kept.iter().position(|k| *k == b));
        let slot = if let Some(pos) = existing {
            pos
        } else {
            let name = new_sheets.get(*id).and_then(|s| s.get("name")).and_then(Value::as_str).unwrap_or(id);
            let _ws = book.new_sheet(name).map_err(|_err| "could not add a sheet (duplicate name?)")?;
            book.sheet_count().saturating_sub(1)
        };
        slots.push(((*id).to_owned(), slot));
    }
    Ok(slots)
}

/// Delete original sheets absent from `order`; returns the kept original indices.
///
/// Refuses a snapshot that keeps none of the original sheets: it was built from
/// another file (or a stale cache), and applying it would wipe the workbook.
fn remove_deleted(book: &mut Workbook, order: &[&str]) -> Result<Vec<usize>, &'static str> {
    let count = book.sheet_count();
    let kept: Vec<usize> = (0..count).filter(|i| order.iter().any(|id| base_index(id) == Some(*i))).collect();
    if kept.is_empty() && count > 0 {
        return Err("snapshot does not match this workbook; reload the file");
    }
    for idx in (0..count).rev().filter(|i| !kept.contains(i)) {
        book.remove_sheet(idx).map_err(|_err| "could not remove a sheet")?;
    }
    Ok(kept)
}

/// Original sheet index from a converter id (`sheet-3` → 3).
fn base_index(id: &str) -> Option<usize> {
    id.strip_prefix("sheet-")?.parse().ok()
}

/// Write next to the original, then rename over it (no half-written file).
fn write_atomic(book: &Workbook, path: &Path) -> Result<(), &'static str> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("workbook");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("xlsx");
    let staging = path.with_file_name(format!(".{stem}.saving.{ext}"));
    if let Err(err) = lazy::write_guarded(book, &staging) {
        let _gone = std::fs::remove_file(&staging);
        let _tmp = std::fs::remove_file(format!("{}tmp", staging.display()));
        return Err(err);
    }
    std::fs::rename(&staging, path).map_err(|_err| "could not replace the original file")
}
