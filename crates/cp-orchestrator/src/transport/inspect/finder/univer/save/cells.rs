//! Cell diff: values, formulas and styles that changed between the original
//! conversion and the Univer snapshot.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use umya_spreadsheet::{Cell, Worksheet};

use super::{EMPTY, NULL};

/// Workbook-level style tables, to resolve `s` ids on both sides.
pub(super) struct Styles<'json> {
    /// `styles` of the original conversion.
    pub base: &'json Map<String, Value>,
    /// `styles` of the incoming snapshot.
    pub new: &'json Map<String, Value>,
}

/// Patch every changed cell of one sheet; returns how many changed.
pub(super) fn patch(ws: &mut Worksheet, sheets: (&Value, &Value), styles: &Styles<'_>) -> usize {
    let old_rows = sheets.0.get("cellData").and_then(Value::as_object).unwrap_or(&*EMPTY);
    let new_rows = sheets.1.get("cellData").and_then(Value::as_object).unwrap_or(&*EMPTY);
    let mut all = coords(old_rows);
    all.extend(coords(new_rows));
    let mut changed = 0usize;
    for (r, c) in all {
        let pair = (cell_at(old_rows, r, c), cell_at(new_rows, r, c));
        if patch_cell(ws, (c.saturating_add(1), r.saturating_add(1)), pair, styles) {
            changed = changed.saturating_add(1);
        }
    }
    changed
}

/// Every `(row, col)` present in a `cellData` matrix (0-based).
fn coords(rows: &Map<String, Value>) -> BTreeSet<(u32, u32)> {
    let mut out = BTreeSet::new();
    for (r, cols) in rows {
        let (Ok(row), Some(col_map)) = (r.parse::<u32>(), cols.as_object()) else { continue };
        out.extend(col_map.keys().filter_map(|c| c.parse::<u32>().ok()).map(|col| (row, col)));
    }
    out
}

/// The cell at `(r, c)`; `null` when absent.
fn cell_at(rows: &Map<String, Value>, r: u32, c: u32) -> &Value {
    rows.get(&r.to_string()).and_then(|cols| cols.get(c.to_string())).unwrap_or(&NULL)
}

/// Patch one cell (`coord` = 1-based `(col, row)`); `true` when anything changed.
fn patch_cell(ws: &mut Worksheet, coord: (u32, u32), pair: (&Value, &Value), styles: &Styles<'_>) -> bool {
    let (old, new) = pair;
    let value_changed = formula(old) != formula(new) || !same_value(&value(old), &value(new));
    if value_changed {
        write_value(ws.cell_mut(coord), new);
    }
    let (old_style, new_style) = (resolve(old, styles.base), resolve(new, styles.new));
    let style_changed = old_style != new_style;
    if style_changed {
        super::style::patch(ws.style_mut(coord), &old_style, &new_style);
    }
    value_changed || style_changed
}

/// Formula text without the leading `=`; `None` for plain values.
fn formula(cell: &Value) -> Option<&str> {
    cell.get("f").and_then(Value::as_str).map(|f| f.trim_start_matches('=')).filter(|f| !f.is_empty())
}

/// The cell value; falls back to the rich-text document's plain text.
fn value(cell: &Value) -> Value {
    if let Some(v) = cell.get("v").filter(|v| !v.is_null()) {
        return v.clone();
    }
    cell.get("p")
        .and_then(|p| p.get("body"))
        .and_then(|b| b.get("dataStream"))
        .and_then(Value::as_str)
        .map_or(Value::Null, |s| Value::String(s.trim_end_matches(['\r', '\n']).to_owned()))
}

/// Value equality where `42` and `42.0` are the same number.
fn same_value(lhs: &Value, rhs: &Value) -> bool {
    match (lhs.as_f64(), rhs.as_f64()) {
        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
        _ => lhs == rhs,
    }
}

/// Resolve the cell's style: an id into the table, or an inline object.
fn resolve(cell: &Value, table: &Map<String, Value>) -> Map<String, Value> {
    let s = cell.get("s");
    let found = s
        .and_then(Value::as_str)
        .map_or_else(|| s.and_then(Value::as_object), |id| table.get(id).and_then(Value::as_object));
    found.cloned().unwrap_or_default()
}

/// Write the snapshot's value (or formula + cached result) into `cell`.
fn write_value(cell: &mut Cell, new: &Value) {
    let v = value(new);
    let _blank = cell.set_blank();
    if let Some(f) = formula(new) {
        let _f = cell.set_formula(f);
        match (v.as_f64(), v.as_str()) {
            (Some(n), _) => {
                let _r = cell.set_formula_result_number(n);
            }
            (None, Some(s)) => {
                let _r = cell.set_formula_result_default(s);
            }
            (None, None) => {}
        }
        return;
    }
    let is_bool = new.get("t").and_then(Value::as_u64) == Some(3);
    match (is_bool, v.as_f64(), v.as_str().filter(|s| !s.is_empty())) {
        (true, ..) => {
            let _b = cell.set_value_bool(v.as_f64().map_or_else(|| v.as_bool().unwrap_or(false), |n| n != 0.0f64));
        }
        (false, Some(n), _) => {
            let _n = cell.set_value_number(n);
        }
        (false, None, Some(s)) => {
            let _s = cell.set_value_string(s);
        }
        (false, None, None) => {}
    }
}
