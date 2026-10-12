//! Converter round-trip: write a styled workbook with umya, convert it, check
//! the Univer snapshot carries every attribute phase 1 promises.

use serde_json::Value;
use umya_spreadsheet::{Color, Pane, PaneStateValues, RichText, SheetView, TextElement};

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
    phase2_cells(&mut book);
    let _written = umya_spreadsheet::writer::xlsx::write(&book, path);
}

/// Rich text in A6, builtin date format (id 14) in B6, indent 2 in C6.
fn phase2_cells(book: &mut umya_spreadsheet::Workbook) {
    let Ok(ws) = book.sheet_mut(0) else { return };
    let mut rt = RichText::default();
    let mut bold = TextElement::default();
    let _t = bold.set_text("Big");
    let _b = bold.font_mut().set_bold(true);
    let mut plain = TextElement::default();
    let _p = plain.set_text(" deal");
    let _r = rt.add_rich_text_elements(bold).add_rich_text_elements(plain);
    let _a6 = ws.cell_mut("A6").set_rich_text(rt);
    let _b6 = ws.cell_mut("B6").set_value_number(45000.0f64);
    let _fmt = ws.style_mut("B6").number_format_mut().set_number_format_id(14);
    let _c6 = ws.cell_mut("C6").set_value("indented");
    ws.style_mut("C6").alignment_mut().set_indent(2);
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

/// Style id of a cell, resolved to its style object.
fn style_of<'json>(wb: &'json Value, row: &str, col: &str) -> &'json Value {
    let sid = at(wb, &["sheets", "sheet-0", "cellData", row, col, "s"]).as_str().unwrap_or("");
    at(wb, &["styles", sid])
}

#[test]
fn converts_text_layout_and_formats() {
    let out = converted();
    let wb = &out.workbook;
    let body = at(wb, &["sheets", "sheet-0", "cellData", "5", "0", "p", "body"]);
    assert_eq!(at(body, &["dataStream"]), "Big deal\r\n");
    assert_eq!(at(body, &["textRuns"]).get(0).and_then(|r| r.get("ed")), Some(&Value::from(3u8)));
    assert_eq!(at(body, &["textRuns"]).get(0).and_then(|r| r.pointer("/ts/bl")), Some(&Value::from(1u8)));
    assert!(at(style_of(wb, "5", "1"), &["n", "pattern"]).as_str().is_some_and(|p| p.contains("yy")));
    assert_eq!(at(style_of(wb, "5", "2"), &["pd", "l"]), 18u32);
}
