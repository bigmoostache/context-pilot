//! Spreadsheet preview: render `csv`/`tsv`/`xlsx`/`xls`/`xlsb`/`ods` files as
//! tabular JSON for the Finder's table preview (T282).
//!
//! Delimited text (`csv`/`tsv`) is parsed with the `csv` reader (proper quote +
//! embedded-newline handling, not a naive split); binary workbooks (`xlsx` and
//! friends) are parsed with `calamine`. Both collapse to one shape — a list of
//! named sheets, each a grid of stringified cells — so the frontend renders
//! every spreadsheet format through a single table component.
//!
//! The payload is bounded on BOTH axes ([`MAX_ROWS`] × [`MAX_COLS`]) so a huge
//! workbook can't balloon the response; `truncated` flags when either cap (or
//! the multi-sheet limit) clipped the data.

use std::sync::Mutex;

use calamine::{Data, Reader as _, open_workbook_auto};

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::support::{agent_folder, confined_path, extract_param};

/// Maximum rows returned per sheet — bounds the response for a tall sheet.
const MAX_ROWS: usize = 1000;

/// Maximum columns returned per row — bounds the response for a wide sheet.
const MAX_COLS: usize = 50;

/// Maximum number of worksheets returned from a multi-sheet workbook.
const MAX_SHEETS: usize = 20;

/// Maximum delimited-text file size read for a `csv`/`tsv` preview (8 MiB).
/// Comfortably covers real tabular exports while bounding memory; a larger file
/// is read up to the cap and flagged `truncated`.
const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;

/// `GET /api/agent/{id}/fs/sheet?path=` — spreadsheet → table JSON.
///
/// Returns `{ sheets: [{ name, rows: [[cell, …]] }], truncated }` where every
/// cell is a string (numbers/dates are stringified for display). Confined to
/// the agent realm (escape → `403`); a non-file → `404`; an unsupported or
/// unparseable file → `415`. `truncated` is `true` when any row/column/sheet
/// cap clipped the data, so the UI can show a "preview clipped" note.
pub fn fs_sheet(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return HttpReply::error(400, "missing path parameter"),
    };
    let Some(target) = confined_path(&folder, &relative) else {
        return HttpReply::error(403, "path outside agent realm");
    };
    if !target.is_file() {
        return HttpReply::error(404, "file not found");
    }

    let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    let parsed = match ext.as_str() {
        "csv" => parse_delimited(&target, b','),
        "tsv" => parse_delimited(&target, b'\t'),
        "xlsx" | "xls" | "xlsb" | "ods" | "xlsm" => parse_workbook(&target),
        _ => return HttpReply::error(415, "not a spreadsheet"),
    };

    match parsed {
        Some(workbook) => HttpReply::ok(&serde_json::json!({
            "sheets": workbook.sheets,
            "truncated": workbook.truncated,
        })),
        None => HttpReply::error(415, "could not parse spreadsheet"),
    }
}

/// A parsed spreadsheet: named sheets of stringified cell grids + a clip flag.
struct Workbook {
    /// One entry per worksheet, each `{ name, rows, formulas? }`.
    sheets: Vec<serde_json::Value>,
    /// `true` when any row/column/sheet cap clipped the data.
    truncated: bool,
}

/// Parse a delimited-text file (`csv`/`tsv`) into a single sheet.
///
/// Uses the `csv` reader with no header inference (the UI treats the first row
/// as the header) and flexible record lengths, so ragged rows don't abort the
/// parse. Returns `None` only on a read fault — a malformed-but-readable file
/// still yields its best-effort rows.
fn parse_delimited(path: &std::path::Path, delimiter: u8) -> Option<Workbook> {
    let bytes = read_capped(path, MAX_CSV_BYTES)?;
    let over_cap = u64::try_from(bytes.len()).unwrap_or(u64::MAX) >= MAX_CSV_BYTES;

    let mut reader =
        csv::ReaderBuilder::new().delimiter(delimiter).has_headers(false).flexible(true).from_reader(bytes.as_slice());

    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut truncated = over_cap;
    for result in reader.records() {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        let Ok(record) = result else { continue };
        let mut cells: Vec<String> = Vec::new();
        for (i, field) in record.iter().enumerate() {
            if i >= MAX_COLS {
                truncated = true;
                break;
            }
            cells.push(field.to_owned());
        }
        rows.push(serde_json::json!(cells));
    }

    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Sheet1").to_owned();
    Some(Workbook { sheets: vec![serde_json::json!({ "name": name, "rows": rows })], truncated })
}

/// Parse a binary workbook (`xlsx`/`xls`/`xlsb`/`ods`) into one sheet per tab.
///
/// `calamine` auto-detects the format from the extension. Each worksheet's used
/// range is walked row-major; every cell is stringified via [`cell_to_string`].
/// Returns `None` if the workbook can't be opened at all.
fn parse_workbook(path: &std::path::Path) -> Option<Workbook> {
    let mut workbook = open_workbook_auto(path).ok()?;
    let names = workbook.sheet_names();

    let mut sheets: Vec<serde_json::Value> = Vec::new();
    let mut truncated = names.len() > MAX_SHEETS;

    for name in names.into_iter().take(MAX_SHEETS) {
        let Ok(range) = workbook.worksheet_range(&name) else {
            continue;
        };
        // Formula range — not all formats support it; `.ok()` gracefully
        // degrades to no-formula mode for ODS / older XLS.
        let formula_range = workbook.worksheet_formula(&name).ok();

        let (sheet, clipped) = build_sheet(&name, &range, formula_range.as_ref());
        truncated |= clipped;
        sheets.push(sheet);
    }

    if sheets.is_empty() {
        return None;
    }
    Some(Workbook { sheets, truncated })
}

/// Build one sheet's JSON (`{ name, rows, formulas? }`) from a calamine range,
/// bounded by [`MAX_ROWS`] × [`MAX_COLS`]. The returned bool is `true` when the
/// row or column cap clipped the sheet. Extracted from [`parse_workbook`] so the
/// outer per-workbook loop stays under the cognitive-complexity cap.
fn build_sheet(
    name: &str,
    range: &calamine::Range<Data>,
    formula_range: Option<&calamine::Range<String>>,
) -> (serde_json::Value, bool) {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut formulas: Vec<serde_json::Value> = Vec::new();
    let mut has_any_formula = false;
    let mut clipped = false;

    for (ri, row) in range.rows().enumerate() {
        if rows.len() >= MAX_ROWS {
            clipped = true;
            break;
        }
        let (cells, formula_row, row_had_formula, row_clipped) = build_row(ri, row, formula_range);
        has_any_formula |= row_had_formula;
        clipped |= row_clipped;
        rows.push(serde_json::json!(cells));
        formulas.push(serde_json::json!(formula_row));
    }

    let mut sheet = serde_json::json!({ "name": name, "rows": rows });
    // Only include the formulas array when the sheet actually has formulas,
    // keeping the response lean for plain data sheets. Insert via the object
    // map (not `sheet["formulas"] = …`, which indexes and can panic).
    if has_any_formula && let Some(obj) = sheet.as_object_mut() {
        let _prev = obj.insert("formulas".to_owned(), serde_json::json!(formulas));
    }
    (sheet, clipped)
}

/// Stringify one row's cells (bounded by [`MAX_COLS`]) and gather any formula
/// strings at each column. Returns `(cells, formula_row, row_had_formula,
/// clipped)` — `clipped` is `true` when the column cap truncated the row.
fn build_row(
    ri: usize,
    row: &[Data],
    formula_range: Option<&calamine::Range<String>>,
) -> (Vec<String>, Vec<serde_json::Value>, bool, bool) {
    let mut cells: Vec<String> = Vec::new();
    let mut formula_row: Vec<serde_json::Value> = Vec::new();
    let mut has_any_formula = false;
    let mut clipped = false;

    for (ci, cell) in row.iter().enumerate() {
        if ci >= MAX_COLS {
            clipped = true;
            break;
        }
        cells.push(cell_to_string(cell));

        // Extract the formula string for this cell position, if any.
        let formula = formula_range.and_then(|fr| fr.get((ri, ci))).filter(|f| !f.is_empty());
        if let Some(f) = formula {
            formula_row.push(serde_json::json!(f));
            has_any_formula = true;
        } else {
            formula_row.push(serde_json::Value::Null);
        }
    }
    (cells, formula_row, has_any_formula, clipped)
}

/// Stringify one workbook cell for display. Empty/error cells render as an
/// empty string; everything else uses its natural textual form (numbers without
/// a trailing `.0` where integral, via `Data`'s own `Display`).
fn cell_to_string(cell: &Data) -> String {
    cp_base::deref_match!(cell, {
        Data::Empty => String::new(),
        Data::String(ref s) | Data::DateTimeIso(ref s) | Data::DurationIso(ref s) => s.clone(),
        // `f64`'s `Display` already omits a trailing `.0` for integral values
        // (e.g. `42.0` renders as `42`), matching how a spreadsheet shows a
        // whole number — so no explicit integral special-case (or `as i64`
        // truncating cast) is needed.
        Data::Float(f) => format!("{f}"),
        Data::Int(i) => format!("{i}"),
        Data::Bool(b) => format!("{b}"),
        Data::DateTime(d) => format!("{d}"),
        Data::Error(ref e) => format!("#{e:?}"),
    })
}

/// Read a file into memory, capped at `max` bytes (the read simply stops at the
/// cap — the caller flags truncation). Returns `None` on an I/O fault.
fn read_capped(path: &std::path::Path, max: u64) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    let _read = file.take(max).read_to_end(&mut buf).ok()?;
    Some(buf)
}
