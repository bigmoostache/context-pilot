//! Converter round-trip: write a styled workbook with umya, convert it, check
//! the Univer snapshot carries every attribute phase 1 promises.

use serde_json::Value;
use umya_spreadsheet::{Color, Pane, PaneStateValues, SheetView};

use super::convert;

/// Build a fixture workbook covering values, formula, style, merge, sizes,
/// hidden row, freeze, tab colour and a second (active) sheet.
fn fixture(path: &std::path::Path) {
    let mut book = umya_spreadsheet::new_file();
    let Ok(ws) = book.sheet_mut(0) else { return };
    let _a1 = ws.cell_mut("A1").set_value("Revenue");
    let _b1 = ws.cell_mut("B1").set_value_number(1234.5f64);
    let _c1 = ws.cell_mut("C1").set_formula("B1*2").set_formula_result_number(2469.0f64);
    let _bold = ws.style_mut("A1").font_mut().set_bold(true);
    let _fill = ws.style_mut("A1").set_background_color_solid("FFFFFF00");
    let _merge = ws.add_merge_cells("A3:C4");
    let _width = ws.column_dimension_mut("A").set_width(20.0);
    let _hidden = ws.row_dimension_mut(2).set_hidden(true);
    let mut tab = Color::default();
    let _argb = tab.set_argb_str("FFFF0000");
    let _tab = ws.set_tab_color(tab);
    let mut pane = Pane::default();
    let _x = pane.set_horizontal_split(1.0).set_vertical_split(1.0).set_state(PaneStateValues::Frozen);
    let views = ws.sheet_views_mut().sheet_view_list_mut();
    if let Some(first) = views.first_mut() {
        let _pane = first.set_pane(pane);
    } else {
        let mut view = SheetView::default();
        let _pane = view.set_pane(pane);
        views.push(view);
    }
    let _second = book.new_sheet("Second");
    let _active = book.workbook_view_mut().set_active_tab(1);
    let _written = umya_spreadsheet::writer::xlsx::write(&book, path);
}

/// Index a JSON path; `Null` when absent.
fn at<'json>(v: &'json Value, keys: &[&str]) -> &'json Value {
    keys.iter().fold(v, |acc, k| acc.get(*k).unwrap_or(&Value::Null))
}

/// Convert the fixture in a temp dir.
fn converted() -> super::Converted {
    let Ok(tmp) = tempfile::tempdir() else {
        panic!("no temp dir");
    };
    let path = tmp.path().join("fixture.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else {
        panic!("converter failed on fixture");
    };
    out
}

#[test]
fn converts_values_and_active_sheet() {
    let out = converted();
    let s0 = at(&out.workbook, &["sheets", "sheet-0"]);
    assert_eq!(out.active_sheet, "sheet-1");
    assert!(!out.truncated);
    assert_eq!(at(s0, &["cellData", "0", "0", "v"]), "Revenue");
    assert_eq!(at(s0, &["cellData", "0", "1", "v"]), 1234.5f64);
    assert_eq!(at(s0, &["cellData", "0", "2", "f"]), "=B1*2");
}

#[test]
fn converts_styles() {
    let out = converted();
    let wb = &out.workbook;
    let Some(sid) = at(wb, &["sheets", "sheet-0", "cellData", "0", "0", "s"]).as_str() else {
        panic!("A1 has no style");
    };
    assert_eq!(at(wb, &["styles", sid, "bl"]), 1u8);
    assert_eq!(at(wb, &["styles", sid, "bg", "rgb"]), "#FFFF00");
}

#[test]
fn converts_layout() {
    let out = converted();
    let s0 = at(&out.workbook, &["sheets", "sheet-0"]);
    assert_eq!(at(s0, &["mergeData"]).as_array().map(Vec::len), Some(1));
    assert_eq!(at(s0, &["columnData", "0", "w"]), 145i64);
    assert_eq!(at(s0, &["rowData", "1", "hd"]), 1u8);
    assert_eq!(at(s0, &["tabColor"]), "#FF0000");
    assert_eq!(at(s0, &["freeze", "startRow"]), 1i64);
    assert_eq!(at(s0, &["freeze", "startColumn"]), 1i64);
}
