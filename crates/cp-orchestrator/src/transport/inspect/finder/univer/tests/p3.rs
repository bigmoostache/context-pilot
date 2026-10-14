//! Phase 3 round-trip: conditional formats and notes land in workbook
//! `resources`; hyperlinks land in the cell `p` document as `customRanges`.

use serde_json::Value;
use umya_spreadsheet::{
    Comment, ConditionalFormatValues, ConditionalFormatting, ConditionalFormattingOperatorValues,
    ConditionalFormattingRule, Formula, Hyperlink, SequenceOfReferences,
};

use super::super::convert;

/// Fixture: a `> 10` rule on A1:A5, a note on B2, an external and an internal link.
fn fixture(path: &std::path::Path) {
    let mut book = umya_spreadsheet::new_file();
    let _second = book.new_sheet("Data");
    let Ok(ws) = book.sheet_mut(0) else { return };
    let _a1 = ws.cell_mut("A1").set_value_number(42.0f64);
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
    let _n = note.new_comment("B2").set_text_string("Check this").set_author("me");
    ws.add_comments(note);

    let mut external = Hyperlink::default();
    let _u = external.set_url("https://example.com");
    let _c3 = ws.cell_mut("C3").set_value("site").set_hyperlink(external);
    let mut internal = Hyperlink::default();
    let _l = internal.set_url("Data!B4").set_location(true);
    let _c4 = ws.cell_mut("C4").set_value("jump").set_hyperlink(internal);
    let _written = umya_spreadsheet::writer::xlsx::write(&book, path);
}

/// Index a JSON path; `Null` when absent.
fn at<'json>(v: &'json Value, keys: &[&str]) -> &'json Value {
    keys.iter().fold(v, |acc, k| acc.get(*k).unwrap_or(&Value::Null))
}

/// Convert the fixture in a temp dir.
fn workbook() -> Value {
    let Ok(tmp) = tempfile::tempdir() else {
        panic!("no temp dir");
    };
    let path = tmp.path().join("p3.xlsx");
    fixture(&path);
    let Some(out) = convert(&path) else {
        panic!("converter failed on fixture");
    };
    out.workbook
}

/// Parse the JSON-string `data` of the named resource.
fn resource(wb: &Value, name: &str) -> Value {
    let found = at(wb, &["resources"])
        .as_array()
        .and_then(|list| list.iter().find(|r| at(r, &["name"]) == name))
        .and_then(|r| at(r, &["data"]).as_str())
        .and_then(|s| serde_json::from_str(s).ok());
    let Some(data) = found else {
        panic!("resource {name} missing");
    };
    data
}

#[test]
fn converts_conditional_format() {
    let data = resource(&workbook(), "SHEET_CONDITIONAL_FORMATTING_PLUGIN");
    let rule = at(&data, &["sheet-0"]).get(0).unwrap_or(&Value::Null);
    assert_eq!(at(rule, &["rule", "type"]), "highlightCell");
    assert_eq!(at(rule, &["rule", "subType"]), "number");
    assert_eq!(at(rule, &["rule", "operator"]), "greaterThan");
    assert_eq!(at(rule, &["rule", "value"]), 10.0f64);
    assert_eq!(at(rule, &["ranges"]).get(0).map(|r| at(r, &["endRow"]).clone()), Some(4u32.into()));
}

#[test]
fn converts_notes() {
    let data = resource(&workbook(), "SHEET_NOTE_PLUGIN");
    assert_eq!(at(&data, &["sheet-0", "1", "1", "note"]), "Check this");
}

#[test]
fn converts_hyperlinks() {
    let wb = workbook();
    let cells = at(&wb, &["sheets", "sheet-0", "cellData"]);
    let link = |row: &str| at(cells, &[row, "2", "p", "body", "customRanges"]).get(0).cloned().unwrap_or_default();
    assert_eq!(at(&link("2"), &["properties", "url"]), "https://example.com");
    assert_eq!(at(&link("2"), &["endIndex"]), 3u32);
    assert_eq!(at(&link("3"), &["properties", "url"]), "#gid=sheet-1&range=B4");
}
