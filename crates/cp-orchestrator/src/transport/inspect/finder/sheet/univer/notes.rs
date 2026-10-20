//! Excel cell notes (legacy comments) → Univer `SHEET_NOTE_PLUGIN` data
//! (T851 phase 3): `{ row: { col: ISheetNote } }` for one sheet, 0-based.

use serde_json::{Map, Value, json};
use umya_spreadsheet::{Comment, Worksheet};

use super::grid::{MAX_COLS, MAX_ROWS};

/// Default note box size in pixels (Excel's default comment shape).
const NOTE_W: u32 = 160;
/// Default note box height in pixels.
const NOTE_H: u32 = 72;

/// All notes of one sheet; empty map when it has none.
pub(super) fn sheet_notes(ws: &Worksheet) -> Map<String, Value> {
    let mut rows: Map<String, Value> = Map::new();
    for comment in ws.comments() {
        let (r, c) = (comment.coordinate().row_num(), comment.coordinate().col_num());
        if r == 0 || c == 0 || r > MAX_ROWS || c > MAX_COLS {
            continue;
        }
        let (row, col) = (r.saturating_sub(1), c.saturating_sub(1));
        let note = json!({
            "id": format!("note-{row}-{col}"),
            "row": row,
            "col": col,
            "width": NOTE_W,
            "height": NOTE_H,
            "note": note_text(comment),
        });
        let entry = rows.entry(row.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            let _prev = obj.insert(col.to_string(), note);
        }
    }
    rows
}

/// Plain text of a note, whether stored as plain or rich text.
fn note_text(comment: &Comment) -> String {
    let body = comment.text();
    body.text()
        .map_or_else(|| body.rich_text().map(|rt| rt.text().into_owned()).unwrap_or_default(), |t| t.value().to_owned())
}
