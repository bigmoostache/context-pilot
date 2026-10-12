//! Excel rich-text cell → Univer `IDocumentData` (the cell `p` field, T851).
//!
//! Univer renders per-run formatting only from a document body: one
//! `dataStream` (text + the mandatory `\r\n` paragraph terminator), `textRuns`
//! giving `[st, ed)` character offsets with a style, and one paragraph entry.
//! Hyperlinks live in the same document, as `customRanges`.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use umya_spreadsheet::{Hyperlink, RichText};
use umya_spreadsheet::drawing::Theme;

use super::style::font_part;

/// Attach a hyperlink to a converted text cell as a Univer `customRanges`
/// entry (`rangeType` 0 = HYPERLINK) spanning the whole text.
///
/// External links keep their URL. Links inside the workbook (`Sheet!A1`) become
/// `#gid=<sheet id>&range=A1`, which Univer follows by jumping to the cell.
/// Non-text cells are left alone: a document would mask their number format.
pub(super) fn add_link(entry: &mut Map<String, Value>, link: &Hyperlink, ctx: &LinkCtx<'_>) {
    let Some(url) = link_url(link, ctx) else {
        return;
    };
    let Some(text) = entry
        .get("v")
        .and_then(Value::as_str)
        .filter(|_| entry.get("t") == Some(&json!(1u8)))
        .map(str::to_owned)
    else {
        return;
    };
    let len = text.encode_utf16().count();
    if len == 0 {
        return;
    }
    let range = json!({
        "startIndex": 0u32,
        "endIndex": len.saturating_sub(1),
        "rangeId": ctx.range_id,
        "rangeType": 0u8,
        "properties": { "url": url },
    });
    if let Some(body) = entry.get_mut("p").and_then(|p| p.get_mut("body")).and_then(Value::as_object_mut) {
        let _prev = body.insert("customRanges".to_owned(), json!([range]));
        return;
    }
    let doc = json!({
        "id": "d",
        "documentStyle": {},
        "body": {
            "dataStream": format!("{text}\r\n"),
            "textRuns": [],
            "paragraphs": [{ "startIndex": len }],
            "customRanges": [range],
        },
    });
    let _p = entry.insert("p".to_owned(), doc);
}

/// What a link needs beyond the cell: sheet-name lookup and a unique range id.
pub(super) struct LinkCtx<'ctx> {
    /// Sheet name → Univer sheet id, for links inside the workbook.
    pub sheet_ids: &'ctx HashMap<String, String>,
    /// Unique id for the custom range (one link per cell).
    pub range_id: String,
}

/// Resolved link target; `None` for an internal link to an unknown sheet.
fn link_url(link: &Hyperlink, ctx: &LinkCtx<'_>) -> Option<String> {
    let raw = link.url();
    if !link.location() {
        return (!raw.is_empty()).then(|| raw.to_owned());
    }
    let (sheet, cell) = raw.trim_start_matches('#').rsplit_once('!')?;
    let name = sheet.trim_matches('\'').replace("''", "'");
    let id = ctx.sheet_ids.get(&name)?;
    Some(format!("#gid={id}&range={}", cell.replace('$', "")))
}

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
