//! Sheet layout diff: merges, column widths, row heights, hidden rows/columns.
//! Inverse of the converter's unit conversions (`px = chars × 7 + 5`,
//! `px = pt × 4/3`).

use serde_json::{Map, Value};
use umya_spreadsheet::Worksheet;
use umya_spreadsheet::helper::coordinate::{coordinate_from_index, string_from_column_index};

use super::EMPTY;

/// Pixels per width character (converter: `px = chars × 7 + 5`).
const CHAR_PX: f64 = 7.0;

/// Fixed padding in characters (`-5 / 7`).
const PAD_CHARS: f64 = -0.714_285_714_285_714_3;

/// Patch merges and row/column dimensions that changed.
pub(super) fn patch(ws: &mut Worksheet, old: &Value, new: &Value) {
    if old.get("mergeData") != new.get("mergeData") {
        merges(ws, new);
    }
    columns(ws, field(old, "columnData"), field(new, "columnData"));
    rows(ws, field(old, "rowData"), field(new, "rowData"));
}

/// Object field; empty when absent.
fn field<'json>(node: &'json Value, key: &str) -> &'json Map<String, Value> {
    node.get(key).and_then(Value::as_object).unwrap_or(&*EMPTY)
}

/// Replace the merge list with the snapshot's.
fn merges(ws: &mut Worksheet, new: &Value) {
    ws.merge_cells_mut().clear();
    for m in new.get("mergeData").and_then(Value::as_array).into_iter().flatten() {
        let n =
            |k: &str| m.get(k).and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).map(|x| x.saturating_add(1));
        let (Some(r0), Some(c0), Some(r1), Some(c1)) = (n("startRow"), n("startColumn"), n("endRow"), n("endColumn"))
        else {
            continue;
        };
        let _ws = ws.add_merge_cells(format!("{}:{}", coordinate_from_index(c0, r0), coordinate_from_index(c1, r1)));
    }
}

/// Changed column entries (width / hidden), keyed by 0-based index.
fn columns(ws: &mut Worksheet, old: &Map<String, Value>, new: &Map<String, Value>) {
    for (key, entry) in changed(old, new) {
        let col = ws.column_dimension_mut(&string_from_column_index(key.saturating_add(1)));
        if let Some(px) = entry.get("w").and_then(Value::as_f64) {
            let _w = col.set_width(px.mul_add(CHAR_PX.recip(), PAD_CHARS).max(0.0f64));
        }
        let _h = col.set_hidden(entry.get("hd").and_then(Value::as_u64) == Some(1));
    }
}

/// Changed row entries (height / hidden), keyed by 0-based index.
fn rows(ws: &mut Worksheet, old: &Map<String, Value>, new: &Map<String, Value>) {
    for (key, entry) in changed(old, new) {
        let row = ws.row_dimension_mut(key.saturating_add(1));
        if let Some(px) = entry.get("h").and_then(Value::as_f64) {
            let _h = row.set_height(px.mul_add(0.75, 0.0)).set_custom_height(true);
        }
        let _hd = row.set_hidden(entry.get("hd").and_then(Value::as_u64) == Some(1));
    }
}

/// Entries whose value differs (a removed entry yields `null` = defaults).
fn changed<'json>(old: &'json Map<String, Value>, new: &'json Map<String, Value>) -> Vec<(u32, &'json Value)> {
    let keys: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    keys.into_iter()
        .filter(|k| old.get(k.as_str()) != new.get(k.as_str()))
        .filter_map(|k| Some((k.parse::<u32>().ok()?, new.get(k.as_str()).unwrap_or(&Value::Null))))
        .collect()
}
