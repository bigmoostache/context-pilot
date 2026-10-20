//! `a:tbl` graphic frames: grid, spans, cell paint, borders and text, with the
//! deck's table style (`ppt/tableStyles.xml`) applied underneath the cells'
//! own properties for the whole table, header row, banded rows and first
//! column.

use super::super::color::Rgba;
use super::super::model::{Stroke, Table, TableCell, TableRow, TextBody};
use super::super::text::{self, TextCtx};
use super::super::xml::Node;
use super::fill::Paint;

/// Default cell insets (EMU): left, top, right, bottom.
const CELL_INSETS: [(&str, i64); 4] = [("marL", 91_440), ("marT", 45_720), ("marR", 91_440), ("marB", 45_720)];

/// Border element names, in model order (left, top, right, bottom).
const BORDERS: [&str; 4] = ["a:lnL", "a:lnT", "a:lnR", "a:lnB"];

/// Table-style border names matching [`BORDERS`].
const STYLE_BORDERS: [&str; 4] = ["a:left", "a:top", "a:right", "a:bottom"];

/// What a table needs besides its own XML.
pub(super) struct TableCtx<'doc> {
    /// Paint lookups of the slide.
    pub paint: &'doc Paint<'doc>,
    /// The matching `a:tblStyle`, if the deck defines it.
    pub style: Option<&'doc Node>,
    /// Inherited list styles for cell text (presentation defaults, master).
    pub lists: &'doc [&'doc Node],
}

/// Convert an `a:tbl`.
pub(super) fn table(tbl: &Node, ctx: &TableCtx<'_>) -> Table {
    let cols = tbl
        .path("a:tblGrid")
        .map(|grid| grid.all("a:gridCol").map(|col| col.int("w").unwrap_or(0)).collect())
        .unwrap_or_default();
    let props = tbl.child("a:tblPr");
    let on = |key: &str| props.and_then(|node| node.flag(key)).unwrap_or(false);
    let flags = Flags {
        first_row: on("firstRow"),
        last_row: on("lastRow"),
        band_row: on("bandRow"),
        first_col: on("firstCol"),
    };
    let trs: Vec<&Node> = tbl.all("a:tr").collect();
    let last = trs.len().saturating_sub(1);
    let rows = trs
        .iter()
        .enumerate()
        .map(|(ri, tr)| {
            let cells =
                tr.all("a:tc").enumerate().map(|(ci, tc)| cell(tc, &parts(&flags, ri, ci, last), ctx)).collect();
            TableRow { height: tr.int("h").unwrap_or(0), cells }
        })
        .collect();
    Table { cols, rows }
}

/// The `a:tblPr` switches that select table-style parts.
struct Flags {
    /// Header row styled.
    first_row: bool,
    /// Total row styled.
    last_row: bool,
    /// Banded rows.
    band_row: bool,
    /// First column styled.
    first_col: bool,
}

/// Table-style part names for a cell, most specific first.
fn parts(flags: &Flags, ri: usize, ci: usize, last: usize) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags.first_row && ri == 0 {
        out.push("a:firstRow");
    }
    if flags.last_row && ri == last && ri > 0 {
        out.push("a:lastRow");
    }
    if flags.first_col && ci == 0 {
        out.push("a:firstCol");
    }
    if flags.band_row && !(flags.first_row && ri == 0) {
        let data_row = if flags.first_row { ri.saturating_sub(1) } else { ri };
        out.push(if data_row.is_multiple_of(2) { "a:band1H" } else { "a:band2H" });
    }
    out.push("a:wholeTbl");
    out
}

/// Convert one `a:tc`.
fn cell(tc: &Node, parts: &[&str], ctx: &TableCtx<'_>) -> TableCell {
    let props = tc.child("a:tcPr");
    let style_parts: Vec<&Node> = parts.iter().filter_map(|name| ctx.style?.child(name)).collect();
    let theme = ctx.paint.theme;
    let fill = props
        .and_then(|node| ctx.paint.fill(node, None))
        .or_else(|| style_parts.iter().find_map(|part| ctx.paint.fill(part.path("a:tcStyle/a:fill")?, None)));
    let text_style = style_parts.iter().find_map(|part| part.child("a:tcTxStyle"));
    let font_color = text_style.and_then(|style| theme.color(style, None));
    let rels = ctx.paint.rels;
    let tctx = TextCtx { theme, rels, font_color, font_face: None };
    let mut text = tc.child("a:txBody").map_or_else(TextBody::default, |tx| text::body(tx, ctx.lists, &[], &tctx));
    if text_style.and_then(|style| style.attr("b")) == Some("on") {
        text.paragraphs.iter_mut().flat_map(|para| para.runs.iter_mut()).for_each(|run| run.bold = true);
    }
    text.insets = CELL_INSETS.map(|(key, dflt)| props.and_then(|node| node.int(key)).unwrap_or(dflt));
    props.and_then(|node| node.attr("anchor")).unwrap_or("t").clone_into(&mut text.anchor);
    TableCell {
        col_span: tc.int("gridSpan").unwrap_or(1).max(1),
        row_span: tc.int("rowSpan").unwrap_or(1).max(1),
        merged: tc.flag("hMerge").unwrap_or(false) || tc.flag("vMerge").unwrap_or(false),
        fill: fill.filter(|paint| !paint.none).and_then(|paint| paint.css),
        borders: borders(props, &style_parts, ctx.paint),
        text,
    }
}

/// Cell borders: `a:lnX` on the cell, else the table style's `a:tcBdr`.
fn borders(props: Option<&Node>, style_parts: &[&Node], paint: &Paint<'_>) -> [Option<Stroke>; 4] {
    let mut out: [Option<Stroke>; 4] = Default::default();
    for (slot, (own, styled)) in out.iter_mut().zip(BORDERS.iter().zip(STYLE_BORDERS)) {
        *slot = props.and_then(|node| node.child(own)).map_or_else(
            || style_parts.iter().find_map(|part| style_line(part.path("a:tcStyle/a:tcBdr")?.child(styled)?, paint)),
            |line| paint.line(&[line], None),
        );
    }
    out
}

/// A table-style border (`a:ln` inside, or a `lnRef` to the theme).
fn style_line(edge: &Node, paint: &Paint<'_>) -> Option<Stroke> {
    if let Some(line) = edge.child("a:ln") {
        return paint.line(&[line], None);
    }
    let line_ref = edge.child("a:lnRef")?;
    let ph: Option<Rgba> = paint.theme.color(line_ref, None);
    let theme_line = paint.theme.lines.get(usize::try_from(line_ref.int("idx")?.saturating_sub(1)).ok()?)?;
    paint.line(&[theme_line], ph)
}

#[cfg(test)]
mod tests {
    use super::{Flags, parts};

    #[test]
    fn header_and_bands() {
        let flags = Flags { first_row: true, last_row: false, band_row: true, first_col: false };
        assert_eq!(parts(&flags, 0, 0, 3), vec!["a:firstRow", "a:wholeTbl"]);
        assert_eq!(parts(&flags, 1, 2, 3), vec!["a:band1H", "a:wholeTbl"]);
    }
}
