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

/// A partial snapshot (only the edited sheet in `sheets`) patches that sheet;
/// one listing no sheet bodies leaves the file alone.
#[test]
fn partial_snapshot_patches_only_sent_sheets() {
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("save.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else { panic!("convert failed") };
    let order = out.workbook.get("sheetOrder").cloned().unwrap_or_default();
    let empty = json!({ "sheetOrder": order, "styles": {}, "sheets": {} });
    assert_eq!(save(&path, &empty), Ok(0));
    let mut snap = out.workbook;
    edit(&mut snap);
    if let Some(res) = snap.as_object_mut() {
        let _gone = res.remove("resources");
    }
    assert_eq!(save(&path, &snap), Ok(3));
    check(&path);
}

/// A cell naming a style id absent from `styles` (stale browser state) keeps
/// its formatting instead of being reset to Excel's default.
#[test]
fn dangling_style_id_keeps_formatting() {
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("save.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else { panic!("convert failed") };
    let mut snap = out.workbook;
    *slot(&mut snap, &["sheets", "sheet-0", "cellData", "0", "0", "s"]) = json!("gone42");
    *slot(&mut snap, &["sheets", "sheet-0", "cellData", "0", "1"]) = json!({ "v": 99u32, "t": 2u8 });
    assert_eq!(save(&path, &snap), Ok(1));
    let Ok(book) = umya_spreadsheet::reader::xlsx::read(&path) else { panic!("re-read failed") };
    let Ok(ws) = book.sheet(0) else { panic!("no sheet") };
    assert_eq!(ws.value("B1"), "99");
    assert!(ws.style("A1").font().is_some_and(umya_spreadsheet::Font::bold));
    assert!(ws.style("A1").fill().is_some());
}

/// Saving the unedited snapshot (e.g. after edit + undo) doesn't touch the file.
#[test]
fn unchanged_snapshot_leaves_file_untouched() {
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("save.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else { panic!("convert failed") };
    let before = std::fs::read(&path).ok();
    assert_eq!(save(&path, &out.workbook), Ok(0));
    assert_eq!(std::fs::read(&path).ok(), before);
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

/// Browser replay: `CP_SHEET_SAMPLE=<xlsx> CP_SHEET_SNAPSHOT=<json>` saves a
/// real Univer snapshot on a copy; Univer's re-encoding must not count as edits.
#[test]
#[ignore = "needs CP_SHEET_SAMPLE + CP_SHEET_SNAPSHOT"]
fn browser_snapshot_patches_only_edits() {
    let (Ok(sample), Ok(snap_path)) = (std::env::var("CP_SHEET_SAMPLE"), std::env::var("CP_SHEET_SNAPSHOT")) else {
        return;
    };
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("sample.xlsx");
    let Ok(_bytes) = std::fs::copy(&sample, &path) else { panic!("copy failed") };
    let Ok(text) = std::fs::read_to_string(&snap_path) else { panic!("snapshot unreadable") };
    let Ok(snap) = serde_json::from_str::<Value>(&text) else { panic!("snapshot not JSON") };
    match save(&path, &snap) {
        Ok(n) => assert!(n >= 1, "nothing patched"),
        Err(msg) => panic!("refused: {msg}"),
    }
}

/// Real-file check: `CP_SHEET_SAMPLE=<xlsx> cargo test ... -- --ignored`.
/// Edits A1 of every sheet in turn on a copy; save must never panic, and a
/// refusal must leave the copy byte-identical.
#[test]
#[ignore = "needs CP_SHEET_SAMPLE pointing at a real workbook"]
fn real_workbook_saves_without_panic() {
    let Ok(sample) = std::env::var("CP_SHEET_SAMPLE") else { return };
    let Ok(dir) = tempfile::tempdir() else { panic!("no temp dir") };
    let path = dir.path().join("sample.xlsx");
    let Ok(_bytes) = std::fs::copy(&sample, &path) else { panic!("copy failed") };
    let Some(out) = convert(&path) else { panic!("convert failed") };
    let only = std::env::var("CP_SHEET_NAMES").unwrap_or_default();
    let wanted = |id: &str| {
        let name = out.workbook.get("sheets").and_then(|s| s.get(id)).and_then(|s| s.get("name"));
        only.is_empty() || name.and_then(Value::as_str).is_some_and(|n| only.split(',').any(|o| o == n))
    };
    let ids: Vec<String> = out
        .workbook
        .get("sheetOrder")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|id| wanted(id)).map(str::to_owned).collect())
        .unwrap_or_default();
    assert!(!ids.is_empty(), "no sheet matched CP_SHEET_NAMES");
    for id in ids {
        let Some(base) = convert(&path) else { panic!("re-convert failed") };
        // Same trip as the browser: JSON text out, JSON text back in.
        let Ok(text) = serde_json::to_string(&base.workbook) else { panic!("serialize failed") };
        let Ok(mut snap) = serde_json::from_str::<Value>(&text) else { panic!("parse failed") };
        *slot(&mut snap, &["sheets", &id, "cellData", "0", "0"]) = json!({ "v": "edited", "t": 1u8 });
        let before = std::fs::read(&path).ok();
        match save(&path, &snap) {
            Ok(n) => assert!(n >= 1, "{id}: nothing patched"),
            Err(msg) => {
                assert_eq!(std::fs::read(&path).ok(), before, "{id}: file changed on refusal");
                assert!(std::env::var("CP_SHEET_MAY_REFUSE").is_ok(), "{id}: refused: {msg}");
            }
        }
    }
}
