//! Excel rich-text cell → Univer `IDocumentData` (the cell `p` field, T851).
//!
//! Univer renders per-run formatting only from a document body: one
//! `dataStream` (text + the mandatory `\r\n` paragraph terminator), `textRuns`
//! giving `[st, ed)` character offsets with a style, and one paragraph entry.

use serde_json::{Map, Value, json};
use umya_spreadsheet::RichText;
use umya_spreadsheet::drawing::Theme;

use super::style::font_part;

/// Build the `p` document for a rich-text cell.
pub(super) fn rich_doc(rt: &RichText, theme: &Theme) -> Value {
    let mut stream = String::new();
    let mut runs: Vec<Value> = Vec::new();
    let mut offset = 0usize;
    for element in rt.rich_text_elements() {
        let text = element.text();
        // Univer offsets count UTF-16 code units (JS string indices).
        let len = text.encode_utf16().count();
        stream.push_str(text);
        if let Some(font) = element.run_properties() {
            let mut ts = Map::new();
            font_part(&mut ts, font, theme);
            if !ts.is_empty() {
                runs.push(json!({ "st": offset, "ed": offset.saturating_add(len), "ts": ts }));
            }
        }
        offset = offset.saturating_add(len);
    }
    stream.push_str("\r\n");
    json!({
        "id": "d",
        "documentStyle": {},
        "body": {
            "dataStream": stream,
            "textRuns": runs,
            "paragraphs": [{ "startIndex": offset }],
        },
    })
}
