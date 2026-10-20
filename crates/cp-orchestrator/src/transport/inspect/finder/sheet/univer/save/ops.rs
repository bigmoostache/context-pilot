//! Structural edits (row/column inserts and deletes) replayed on the original
//! sheet before the cell diff, so shifted cells keep their original styles.
//!
//! The browser sends them in order as `ops`: `{ sheet, kind, at, count }` with
//! `kind` one of `insertRows`, `removeRows`, `insertCols`, `removeCols` and
//! `at` 0-based. umya shifts the sheet's cells, styles, formulas, merges,
//! conditional formats, notes, drawings and sheet-scoped names; the converter
//! JSON of the original is shifted the same way, so the diff that follows only
//! sees real edits instead of every cell below the change.
//!
//! Only the edited sheet is touched: umya's workbook-level variants deserialize
//! every sheet, which drags in charts its writer can't regenerate.

use serde_json::{Map, Value};
use umya_spreadsheet::Worksheet;

/// One structural edit.
#[derive(Clone, Copy)]
pub(super) struct Op {
    /// Rows (`true`) or columns.
    rows: bool,
    /// Insert (`true`) or delete.
    insert: bool,
    /// First row/column affected, 0-based.
    at: u32,
    /// How many rows/columns.
    count: u32,
}

/// The ops of `sheet`, in the order the user made them.
pub(super) fn for_sheet(snapshot: &Value, sheet: &str) -> Vec<Op> {
    snapshot
        .get("ops")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|o| o.get("sheet").and_then(Value::as_str) == Some(sheet))
        .filter_map(parse)
        .collect()
}

/// `{ kind, at, count }` → [`Op`]; `None` when malformed or empty.
fn parse(o: &Value) -> Option<Op> {
    let (rows, insert) = match o.get("kind").and_then(Value::as_str)? {
        "insertRows" => (true, true),
        "removeRows" => (true, false),
        "insertCols" => (false, true),
        "removeCols" => (false, false),
        _ => return None,
    };
    let count = index(o, "count").filter(|c| *c > 0)?;
    Some(Op { rows, insert, at: index(o, "at")?, count })
}

/// A `u32` field of a JSON object.
fn index(o: &Value, key: &str) -> Option<u32> {
    u32::try_from(o.get(key)?.as_u64()?).ok()
}

/// Replay `op` on the umya sheet (its API is 1-based).
pub(super) fn apply(ws: &mut Worksheet, op: Op) {
    let at = op.at.saturating_add(1);
    match (op.rows, op.insert) {
        (true, true) => ws.insert_new_row(at, op.count),
        (true, false) => ws.remove_row(at, op.count),
        (false, true) => ws.insert_new_column_by_index(at, op.count),
        (false, false) => ws.remove_column_by_index(at, op.count),
    }
}

/// Shift the converter JSON of a sheet the way [`apply`] shifts the file.
pub(super) fn shift(sheet: &mut Value, op: Op) {
    let Some(obj) = sheet.as_object_mut() else { return };
    if let Some(cells) = obj.get_mut("cellData").and_then(Value::as_object_mut) {
        shift_cells(cells, op);
    }
    let dims = if op.rows { "rowData" } else { "columnData" };
    if let Some(d) = obj.get_mut(dims).and_then(Value::as_object_mut) {
        *d = rekey(d, op);
    }
    if let Some(m) = obj.get_mut("mergeData").and_then(Value::as_array_mut) {
        m.retain_mut(|merge| shift_merge(merge, op));
    }
}

/// `cellData` is `row → col → cell`: rows re-key the outer map, columns each row.
fn shift_cells(cells: &mut Map<String, Value>, op: Op) {
    if op.rows {
        *cells = rekey(cells, op);
        return;
    }
    for row in cells.values_mut().filter_map(Value::as_object_mut) {
        *row = rekey(row, op);
    }
}

/// Map keyed by index, moved past `op`; entries in a deleted range are dropped.
fn rekey(map: &Map<String, Value>, op: Op) -> Map<String, Value> {
    map.iter().filter_map(|(k, v)| Some((moved(k.parse::<u32>().ok()?, op)?.to_string(), v.clone()))).collect()
}

/// Where index `i` lands after `op`; `None` when it was deleted.
fn moved(i: u32, op: Op) -> Option<u32> {
    if i < op.at {
        return Some(i);
    }
    if op.insert {
        return Some(i.saturating_add(op.count));
    }
    (i >= op.at.saturating_add(op.count)).then(|| i.saturating_sub(op.count))
}

/// Move a merge's span; `false` when the delete removed it entirely.
fn shift_merge(merge: &mut Value, op: Op) -> bool {
    let (s, e) = if op.rows { ("startRow", "endRow") } else { ("startColumn", "endColumn") };
    let (Some(start), Some(end)) = (index(merge, s), index(merge, e)) else { return true };
    let Some(obj) = merge.as_object_mut() else { return true };
    let (first, last) = (moved(start, op), moved(end, op));
    if first.is_none() && last.is_none() {
        return false;
    }
    // A delete cutting one edge shrinks the merge to what is left.
    let from = first.unwrap_or(op.at);
    let to = last.unwrap_or_else(|| op.at.saturating_sub(1));
    let _s = obj.insert(s.to_owned(), from.into());
    let _e = obj.insert(e.to_owned(), to.into());
    to >= from
}
