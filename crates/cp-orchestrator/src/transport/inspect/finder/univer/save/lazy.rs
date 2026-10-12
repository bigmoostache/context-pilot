//! Lazy-load guards for the save path.
//!
//! The workbook is read with `lazy_read`: a sheet left undeserialized is
//! written back from its raw XML, charts and all. Only edited sheets are
//! deserialized, which keeps umya's chart writer away from charts it can't
//! regenerate (a `numLit` series parses as a reference to sheet `""` and the
//! writer panics on `sheet_by_name("").unwrap()`).

use std::panic::{AssertUnwindSafe, catch_unwind};

use umya_spreadsheet::Workbook;

/// Refusal when an edited sheet carries a chart umya can't rewrite.
const UNSAVABLE_CHART: &str =
    "an edited sheet has a chart with values typed into it, which can't be saved; the original file is untouched";

/// Make the writer's chart-cache refresh safe for every deserialized sheet.
///
/// The writer re-reads each chart series from the sheet it names, and unwraps:
/// a sheet that is missing or still raw panics. Deserialize every referenced
/// sheet (to a fixed point, since those may hold charts too) and refuse a chart
/// naming a sheet that doesn't exist.
pub(super) fn load_chart_sources(book: &mut Workbook) -> Result<(), &'static str> {
    loop {
        let mut wanted: Vec<usize> = Vec::new();
        for ws in (0..book.sheet_count()).filter_map(|i| book.sheet(i).ok()) {
            for name in chart_sheet_names(ws) {
                let idx = book.sheet_collection_no_check().iter().position(|s| s.name() == name);
                match idx {
                    Some(i) if book.sheet(i).is_err() => {
                        wanted.push(i);
                    }
                    Some(_) => {}
                    None => return Err(UNSAVABLE_CHART),
                }
            }
        }
        if wanted.is_empty() {
            return Ok(());
        }
        for idx in wanted {
            let _ws = book.read_sheet(idx);
            if book.sheet(idx).is_err() {
                return Err(UNSAVABLE_CHART); // still raw: another pass would never end
            }
        }
    }
}

/// Sheet names referenced by the charts drawn on `ws`.
///
/// umya exposes series formulas only through a deep, per-chart-type tree, so
/// the names are lifted from the `Address` debug dump (`sheet_name: "..."`).
fn chart_sheet_names(ws: &umya_spreadsheet::Worksheet) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for chart in ws.worksheet_drawing().chart_collection() {
        let dump = format!("{chart:?}");
        for rest in dump.split("sheet_name: \"").skip(1) {
            let name = rest.split('"').next().unwrap_or_default().to_owned();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Run umya's writer, turning a library panic into an error.
pub(super) fn write_guarded(book: &Workbook, path: &std::path::Path) -> Result<(), &'static str> {
    match catch_unwind(AssertUnwindSafe(|| umya_spreadsheet::writer::xlsx::write(book, path))) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_err)) => Err("could not write the workbook"),
        Err(_panic) => Err("the Excel writer crashed on this workbook; the original file is untouched"),
    }
}
