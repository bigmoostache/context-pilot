//! Serializable data structures for persistence (`config::Shared`, `WorkerState`, `Message`).

/// Persistence structs: `config::Shared`, `WorkerState`, `PanelData`.
pub mod config;
/// Message struct and conversation formatting.
pub mod message;
/// Model selection, pricing, and cleaning-threshold helpers for [`super::runtime::State`].
pub mod model_helpers;

// ─── Per-tick cache-break telemetry ─────────────────────────────────────────

/// How the prompt's panel section changed between consecutive streams (SA → SB).
///
/// Mutually exclusive, exhaustive partition:
/// - `NoBreak`: panels identical — cache prefix preserved.
/// - `ContentChanged`: an existing panel's content mutated.
/// - `PanelAppeared`: a brand-new panel entered the prompt (no existing panel changed).
/// - `PanelDisappeared`: a panel from SA was removed from SB (no existing panel changed).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CacheBreakKind {
    /// No cache break — all panels unchanged.
    #[default]
    NoBreak,
    /// An existing panel's content changed (culprit = first changed panel).
    ContentChanged,
    /// A new panel appeared and no existing panel changed (culprit = new panel).
    PanelAppeared,
    /// A panel from the previous prompt was removed (culprit = removed panel).
    PanelDisappeared,
}

impl CacheBreakKind {
    /// TSV-friendly label (lowercase, underscore-separated).
    #[must_use]
    pub const fn as_tsv(self) -> &'static str {
        match self {
            Self::NoBreak => "no_break",
            Self::ContentChanged => "content_changed",
            Self::PanelAppeared => "panel_appeared",
            Self::PanelDisappeared => "panel_disappeared",
        }
    }
}

/// Per-tick telemetry captured at stream start, consumed at stream end for TSV logging.
///
/// Populated by `prepare_stream_context()` (beginning of tick: culprit detection,
/// token layout, recent tools). Consumed by cost-tracking append once the stream
/// finalizes and token costs are known.
#[derive(Debug, Default)]
pub struct TickTelemetry {
    /// Epoch milliseconds when the tick started.
    pub tick_start_ms: u64,
    /// Last 3 tool names, comma-separated (most recent first).
    pub three_last_tools: String,
    /// Context type of the cache-break culprit panel, or `"none"`.
    pub culprit_type: String,
    /// Tokens strictly before the culprit (system + tools + preceding panels).
    pub tokens_before_culprit: usize,
    /// Tokens of the culprit panel itself.
    pub tokens_culprit: usize,
    /// Tokens strictly after the culprit (trailing panels, excluding conversation).
    pub tokens_after_culprit: usize,
    /// Whether the queue module was actively intercepting tools this tick.
    pub queue_is_active: bool,
    /// Whether tempo held this tick (no tool broke it last tick → global freeze).
    pub tempo_is_active: bool,
    /// How the panel section changed between consecutive streams.
    pub break_kind: CacheBreakKind,
    /// Configured `max_freezes` for the culprit panel (0 when no culprit).
    pub culprit_max_freezes: u8,
}
