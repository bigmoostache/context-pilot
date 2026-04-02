//! Intermediate rendering representation for Context Pilot.
//!
//! This crate defines the IR types that sit between the data layer and the
//! terminal renderer. Panels emit `Vec<Block>`, and the centralized
//! `build_frame()` assembles a full [`Frame`] snapshot each tick. A
//! platform-specific adapter then converts the `Frame` into drawable
//! primitives (ratatui for TUI, HTML for web, etc.).
//!
//! # Design principles
//!
//! - **Semantic styling** — colours are expressed as [`Semantic`] tokens,
//!   not RGB. The adapter maps tokens to a concrete palette.
//! - **Structured data** — tables, trees, and progress bars are first-class
//!   [`Block`] variants, not pre-formatted text.
//! - **Serializable** — every type derives [`Serialize`](serde::Serialize)
//!   so the frame can be shipped over the wire to a web frontend.

use serde::Serialize;

/// Conversation and overlay IR types.
pub mod conversation;
/// Frame-level IR types: sidebar, status bar, panel content.
pub mod frame;

// ── Shared primitives ────────────────────────────────────────────────

/// Semantic colour / emphasis token.
///
/// The TUI adapter maps each variant to a concrete `ratatui::style::Style`.
/// Web adapters map to CSS classes. This keeps the IR platform-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum Semantic {
    /// Default foreground.
    Default,
    /// Primary accent — headings, highlights, active items.
    Accent,
    /// Dimmed accent — secondary highlights.
    AccentDim,
    /// Muted / low-contrast — timestamps, metadata, help text.
    Muted,
    /// Success indicator — passed tests, healthy status.
    Success,
    /// Warning indicator — degraded state, approaching limits.
    Warning,
    /// Error / danger — failed checks, critical issues.
    Error,
    /// Informational — neutral callouts, tips.
    Info,
    /// Active / selected item background highlight.
    Active,
    /// Key binding / shortcut label.
    KeyHint,
    /// Literal code or monospaced content.
    Code,
    /// Added / inserted content (diffs).
    DiffAdd,
    /// Removed / deleted content (diffs).
    DiffRemove,
    /// Section or group header.
    Header,
    /// Separator / border / decorative line.
    Border,
    /// Bold emphasis (combined with another semantic via [`Span::bold`]).
    Bold,
}

/// A styled text fragment — the atomic rendering unit.
///
/// Every piece of visible text in the IR is a `Span`. The adapter reads
/// `semantic` + modifiers (`bold`, `italic`, `dimmed`) to produce the
/// final visual style. When `color` is `Some`, the adapter uses the raw
/// RGB value instead of mapping `semantic` — this supports syntax
/// highlighting where colours come from a theme, not from semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// The text content.
    pub text: String,
    /// Semantic colour token.
    pub semantic: Semantic,
    /// Render in bold weight.
    pub bold: bool,
    /// Render in italic style.
    pub italic: bool,
    /// Dim the output (reduce intensity).
    pub dimmed: bool,
    /// Optional raw RGB colour override (syntax highlighting).
    /// When set, the adapter uses this instead of mapping `semantic`.
    pub color: Option<(u8, u8, u8)>,
}

impl Span {
    /// Plain span with default styling.
    #[must_use]
    pub const fn new(text: String) -> Self {
        Self { text, semantic: Semantic::Default, bold: false, italic: false, dimmed: false, color: None }
    }

    /// Span with a specific semantic token.
    #[must_use]
    pub const fn styled(text: String, semantic: Semantic) -> Self {
        Self { text, semantic, bold: false, italic: false, dimmed: false, color: None }
    }

    /// Span with a raw RGB colour override (syntax highlighting).
    #[must_use]
    pub const fn rgb(text: String, red: u8, green: u8, blue: u8) -> Self {
        Self {
            text,
            semantic: Semantic::Default,
            bold: false,
            italic: false,
            dimmed: false,
            color: Some((red, green, blue)),
        }
    }

    /// Accent-coloured span.
    #[must_use]
    pub const fn accent(text: String) -> Self {
        Self::styled(text, Semantic::Accent)
    }

    /// Muted / low-contrast span.
    #[must_use]
    pub const fn muted(text: String) -> Self {
        Self::styled(text, Semantic::Muted)
    }

    /// Success-coloured span.
    #[must_use]
    pub const fn success(text: String) -> Self {
        Self::styled(text, Semantic::Success)
    }

    /// Warning-coloured span.
    #[must_use]
    pub const fn warning(text: String) -> Self {
        Self::styled(text, Semantic::Warning)
    }

    /// Error-coloured span.
    #[must_use]
    pub const fn error(text: String) -> Self {
        Self::styled(text, Semantic::Error)
    }

    /// Info-coloured span.
    #[must_use]
    pub const fn info(text: String) -> Self {
        Self::styled(text, Semantic::Info)
    }

    /// Set bold modifier.
    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Set italic modifier.
    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Set dimmed modifier.
    #[must_use]
    pub const fn dim(mut self) -> Self {
        self.dimmed = true;
        self
    }
}

/// Horizontal alignment for table cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Align {
    /// Left-aligned (default).
    Left,
    /// Centred.
    Center,
    /// Right-aligned.
    Right,
}

/// A table cell — one or more styled spans with alignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cell {
    /// Styled fragments inside this cell.
    pub spans: Vec<Span>,
    /// Horizontal alignment.
    pub align: Align,
}

impl Cell {
    /// Left-aligned cell from a single span.
    #[must_use]
    pub fn left(span: Span) -> Self {
        Self { spans: vec![span], align: Align::Left }
    }

    /// Right-aligned cell from a single span.
    #[must_use]
    pub fn right(span: Span) -> Self {
        Self { spans: vec![span], align: Align::Right }
    }

    /// Centred cell from a single span.
    #[must_use]
    pub fn center(span: Span) -> Self {
        Self { spans: vec![span], align: Align::Center }
    }

    /// Cell from multiple spans, left-aligned.
    #[must_use]
    pub const fn multi(spans: Vec<Span>) -> Self {
        Self { spans, align: Align::Left }
    }

    /// Plain text cell, left-aligned.
    #[must_use]
    pub fn text(text: String) -> Self {
        Self { spans: vec![Span::new(text)], align: Align::Left }
    }

    /// Styled text cell, left-aligned.
    #[must_use]
    pub fn styled(text: String, semantic: Semantic) -> Self {
        Self { spans: vec![Span::styled(text, semantic)], align: Align::Left }
    }

    /// Empty cell (no content, left-aligned).
    #[must_use]
    pub const fn empty() -> Self {
        Self { spans: Vec::new(), align: Align::Left }
    }
}

/// Column definition for [`Block::Table`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Column {
    /// Column header text (empty string = no header).
    pub header: String,
    /// Default alignment for cells in this column.
    pub align: Align,
}

// ── Block: the universal panel content unit ──────────────────────────

/// A segment of a progress / gauge bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgressSegment {
    /// Fraction of the bar this segment occupies (0–100).
    pub percent: u8,
    /// Semantic colour for this segment.
    pub semantic: Semantic,
    /// Optional label rendered inside / beside the segment.
    pub label: Option<String>,
}

/// A node in a [`Block::Tree`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeNode {
    /// Display text (may contain icon prefix).
    pub label: Vec<Span>,
    /// Nested children.
    pub children: Vec<Self>,
    /// Whether this node is expanded (children visible).
    pub expanded: bool,
}

/// A key–value pair: styled key spans + styled value spans.
pub type KeyValuePair = (Vec<Span>, Vec<Span>);

/// The universal content building block.
///
/// Panels return `Vec<Block>` from their `blocks()` method. The adapter
/// converts each variant into platform-specific widgets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Block {
    /// A single line of styled spans.
    Line(Vec<Span>),

    /// An empty line (vertical spacing).
    Empty,

    /// A section header — rendered prominently with optional underline.
    Header(Vec<Span>),

    /// A data table with optional column headers.
    Table {
        /// Column definitions (alignment + optional header text).
        columns: Vec<Column>,
        /// Row data — each row is a `Vec<Cell>` matching column count.
        rows: Vec<Vec<Cell>>,
    },

    /// A horizontal progress / gauge bar.
    ProgressBar {
        /// Segments that compose the bar (must sum to ≤ 100).
        segments: Vec<ProgressSegment>,
        /// Optional text label shown beside the bar.
        label: Option<String>,
    },

    /// A tree / hierarchy.
    Tree(Vec<TreeNode>),

    /// A horizontal separator line.
    Separator,

    /// A list of key–value pairs rendered in two columns.
    KeyValue(Vec<KeyValuePair>),
}

impl Block {
    /// Shorthand: single line from one span.
    #[must_use]
    pub fn text(text: String) -> Self {
        Self::Line(vec![Span::new(text)])
    }

    /// Shorthand: styled single line.
    #[must_use]
    pub fn styled_text(text: String, semantic: Semantic) -> Self {
        Self::Line(vec![Span::styled(text, semantic)])
    }

    /// Shorthand: section header from plain text.
    #[must_use]
    pub fn header(text: String) -> Self {
        Self::Header(vec![Span::styled(text, Semantic::Header)])
    }

    /// Shorthand: line from multiple spans.
    #[must_use]
    pub const fn line(spans: Vec<Span>) -> Self {
        Self::Line(spans)
    }

    /// Shorthand: simple table with string headers.
    #[must_use]
    pub fn table(headers: Vec<(&str, Align)>, rows: Vec<Vec<Cell>>) -> Self {
        let columns = headers.into_iter().map(|(h, a)| Column { header: h.to_owned(), align: a }).collect();
        Self::Table { columns, rows }
    }

    /// Shorthand: empty line (vertical spacing).
    #[must_use]
    pub const fn empty() -> Self {
        Self::Empty
    }

    /// Shorthand: horizontal separator.
    #[must_use]
    pub const fn separator() -> Self {
        Self::Separator
    }

    /// Shorthand: key-value list.
    #[must_use]
    pub const fn kv(pairs: Vec<KeyValuePair>) -> Self {
        Self::KeyValue(pairs)
    }

    /// Shorthand: single key-value pair as a block.
    #[must_use]
    pub fn kv_row(key: Vec<Span>, value: Vec<Span>) -> Self {
        Self::KeyValue(vec![(key, value)])
    }
}
