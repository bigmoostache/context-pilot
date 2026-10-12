//! Phase 4 round-trip: convert, edit the snapshot, patch the file, re-read it.
//! Edited cells change; everything else (fill, CF rules, notes) survives.

use serde_json::{Value, json};
use umya_spreadsheet::{
    Comment, ConditionalFormatValues, ConditionalFormatting, ConditionalFormattingOperatorValues,
    ConditionalFormattingRule, Formula, SequenceOfReferences,
};

use super::super::{convert, save};

/// Fixture: bold yellow A1, B1 = 1, a `> 10` rule on A1:A5, a note on B2.
fn fixture(path: &std::path::Path) {
    let mut book = umya_spreadsheet::new_file();
    let Ok(ws) = book.sheet_mut(0) else { return };
    let _a1 = ws.cell_mut("A1").set_value("Title");
    let _bold = ws.style_mut("A1").font_mut().set_bold(true);
    let _fill = ws.style_mut("A1").set_background_color_solid("FFFFFF00");
    let _b1 = ws.cell_mut("B1").set_value_number(1.0f64);
    let mut formula = Formula::default();
    let _f = formula.set_string_value("10");
    let mut rule = ConditionalFormattingRule::default();
    let _r = rule
        .set_type(ConditionalFormatValues::CellIs)
        .set_operator(ConditionalFormattingOperatorValues::GreaterThan)
        .set_priority(1)
        .set_formula(formula);
    let mut refs = SequenceOfReferences::default();
    let _s = refs.set_sqref("A1:A5");
    let mut block = ConditionalFormatting::default();
    let _b = block.set_sequence_of_references(refs).add_conditional_collection(rule);
    ws.add_conditional_formatting_collection(block);
    let mut note = Comment::default();
    let _n = note.new_comment("B2").set_text_string("keep me").set_author("me");
    ws.add_comments(note);
    let _written = umya_spreadsheet::writer::xlsx::write(&book, path);
}

/// Mutable handle on a JSON object path, created as needed.
fn slot<'json>(v: &'json mut Value, keys: &[&str]) -> &'json mut Value {
    keys.iter().fold(v, |acc, k| {
        if !acc.is_object() {
            *acc = json!({});
        }
        &mut acc[*k]
    })
}

#[test]
fn patches_edits_and_keeps_the_rest() {
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("save.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else { panic!("convert failed") };
    let mut snap = out.workbook;
    edit(&mut snap);
    assert_eq!(save(&path, &snap), Ok(3));
    check(&path);
}

/// Italicise A1 (keeping its style), set B1 = 99, add C2 = "new".
fn edit(snap: &mut Value) {
    let cells = ["sheets", "sheet-0", "cellData"];
    let a1_style = snap.pointer("/sheets/sheet-0/cellData/0/0/s").and_then(Value::as_str).map(str::to_owned);
    let mut style = a1_style.and_then(|id| snap.pointer(&format!("/styles/{id}")).cloned()).unwrap_or_default();
    *slot(&mut style, &["it"]) = json!(1u8);
    *slot(snap, &[cells[0], cells[1], cells[2], "0", "0", "s"]) = style;
    *slot(snap, &[cells[0], cells[1], cells[2], "0", "1"]) = json!({ "v": 99u32, "t": 2u8 });
    *slot(snap, &[cells[0], cells[1], cells[2], "1", "2"]) = json!({ "v": "new", "t": 1u8 });
}

/// The patched file has the edits and kept the fill, CF rule and note.
fn check(path: &std::path::Path) {
    let Ok(book) = umya_spreadsheet::reader::xlsx::read(path) else { panic!("re-read failed") };
    let Ok(ws) = book.sheet(0) else { panic!("no sheet") };
    assert_eq!(ws.value("B1"), "99");
    assert_eq!(ws.value("C2"), "new");
    assert_eq!(ws.value("A1"), "Title");
    check_kept(ws);
}

/// A1 is bold + italic and still filled; the CF rule and the note survived.
fn check_kept(ws: &umya_spreadsheet::Worksheet) {
    let font = ws.style("A1").font().cloned().unwrap_or_default();
    assert!(font.bold() && font.italic());
    assert!(ws.style("A1").fill().is_some());
    assert_eq!(ws.conditional_formatting_collection().len(), 1);
    assert_eq!(ws.comments().len(), 1);
}

#[test]
fn refuses_a_foreign_snapshot() {
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("save.xlsx");
    fixture(&path);
    let before = std::fs::read(&path).ok();
    let snap = json!({ "sheetOrder": ["other"], "sheets": { "other": { "name": "X" } } });
    assert_eq!(save(&path, &snap), Err("snapshot does not match this workbook; reload the file"));
    assert_eq!(std::fs::read(&path).ok(), before);
}
