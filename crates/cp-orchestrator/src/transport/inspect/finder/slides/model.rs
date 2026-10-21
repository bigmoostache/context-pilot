//! The render model sent to the webapp: a deck already resolved through the
//! master → layout → slide cascade, so the frontend only positions boxes.
//!
//! Geometry is in EMU (914 400 per inch, 12 700 per point); the frontend
//! scales the slide box to its viewport. Font sizes are hundredths of a point
//! (OOXML's own unit). Colours are CSS strings. Optional fields are omitted
//! from the JSON when absent, keeping text-heavy decks small.

use serde::Serialize;

/// A whole presentation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Deck {
    /// Slide width (EMU).
    pub width: i64,
    /// Slide height (EMU).
    pub height: i64,
    /// Slides in presentation order.
    pub slides: Vec<Slide>,
    /// `true` when the slide cap clipped the deck.
    pub truncated: bool,
}

/// One slide, master and layout decorations included.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Slide {
    /// 1-based position.
    pub index: usize,
    /// Hidden in slide show.
    pub hidden: bool,
    /// Background paint (CSS colour, gradient or `url(data:…)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Shapes back to front.
    pub shapes: Vec<Shape>,
    /// Speaker notes as plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// One positioned element (shape, picture, connector, table).
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Shape {
    /// Name from `p:cNvPr` (tooltips, debugging).
    pub name: String,
    /// Left (EMU).
    pub x: i64,
    /// Top (EMU).
    pub y: i64,
    /// Width (EMU).
    pub w: i64,
    /// Height (EMU).
    pub h: i64,
    /// Clockwise rotation in 60000ths of a degree.
    pub rot: i64,
    /// Mirrored horizontally.
    pub flip_h: bool,
    /// Mirrored vertically.
    pub flip_v: bool,
    /// Outline as SVG path data in the shape's `0..w × 0..h` box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Interior paint (CSS colour or gradient).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Outline stroke.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    /// Picture as a `data:` URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Text frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextBody>,
    /// Table grid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<Table>,
}

/// A line style.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Stroke {
    /// CSS colour.
    pub color: String,
    /// Width (EMU).
    pub width: i64,
    /// Preset dash name (`dash`, `sysDot`, …); omitted when solid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<String>,
    /// Arrow head at the end (`triangle`, `arrow`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Arrow head at the start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<String>,
}

/// A text frame: body properties plus paragraphs.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TextBody {
    /// Vertical anchor: `t`, `ctr`, `b`.
    pub anchor: String,
    /// Insets left, top, right, bottom (EMU).
    pub insets: [i64; 4],
    /// Wrap text inside the box (`false` = single line overflow).
    pub wrap: bool,
    /// `normAutofit` shrink factor in 1/1000 percent (100000 = none).
    pub font_scale: i64,
    /// `vert` text direction (`horz`, `vert`, `vert270`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vert: Option<String>,
    /// Paragraphs.
    pub paragraphs: Vec<Paragraph>,
}

/// One paragraph with its resolved properties.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Paragraph {
    /// `l`, `ctr`, `r`, `just`, `dist`.
    pub align: String,
    /// Left margin (EMU).
    pub margin_left: i64,
    /// First-line indent (EMU, negative = hanging).
    pub indent: i64,
    /// Line spacing: positive = 1/1000 percent, negative = −(1/100 pt).
    pub line_spacing: i64,
    /// Space before (1/100 pt).
    pub space_before: i64,
    /// Space after (1/100 pt).
    pub space_after: i64,
    /// Bullet glyph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bullet: Option<Bullet>,
    /// Size used for an empty paragraph's line box (1/100 pt).
    pub end_size: i64,
    /// Runs and line breaks.
    pub runs: Vec<Run>,
}

/// A bullet or auto-number, already rendered to its glyph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Bullet {
    /// Glyph text (`•`, `3.`, `c)`).
    pub text: String,
    /// CSS colour; omitted = follow the first run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Font face; omitted = follow the first run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Size in 1/1000 percent of the run size.
    pub scale: i64,
}

/// A run of uniformly formatted text (`text == "\n"` is a line break).
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Run {
    /// Characters.
    pub text: String,
    /// Size (1/100 pt).
    pub size: i64,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strike: bool,
    /// CSS colour.
    pub color: String,
    /// Font face.
    pub font: String,
    /// Highlight colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    /// Superscript (> 0) / subscript (< 0) offset in 1/1000 percent.
    pub baseline: i64,
    /// `all` / `small` capitals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caps: Option<String>,
    /// Hyperlink target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// A table: column widths plus rows of cells.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Table {
    /// Column widths (EMU).
    pub cols: Vec<i64>,
    /// Rows top to bottom.
    pub rows: Vec<TableRow>,
}

/// One table row.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TableRow {
    /// Minimum height (EMU).
    pub height: i64,
    /// Cells left to right (covered cells included, flagged `merged`).
    pub cells: Vec<TableCell>,
}

/// One table cell.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TableCell {
    /// Columns spanned.
    pub col_span: i64,
    /// Rows spanned.
    pub row_span: i64,
    /// Covered by a neighbour's span: not drawn.
    pub merged: bool,
    /// Background paint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Borders left, top, right, bottom.
    pub borders: [Option<Stroke>; 4],
    /// Content.
    pub text: TextBody,
}
