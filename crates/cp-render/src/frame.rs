//! Frame-level IR types: sidebar, status bar, panel content.
//!
//! These types compose the outer shell of a [`Frame`] snapshot —
//! everything except the conversation region and modal overlays.

use serde::Serialize;

use crate::{Block, ProgressSegment, Semantic};

// ── Frame ────────────────────────────────────────────────────────────

/// Top-level frame — one complete screen snapshot per render tick.
///
/// Built by the centralized frame builder and consumed by the platform
/// adapter.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    /// Left sidebar region.
    pub sidebar: Sidebar,
    /// Active panel content (right pane).
    pub active_panel: PanelContent,
    /// Bottom status bar.
    pub status_bar: StatusBar,
    /// Conversation region (chat messages + input).
    pub conversation: crate::conversation::Conversation,
    /// Modal overlays (question form, autocomplete, etc.).
    pub overlays: Vec<crate::conversation::Overlay>,
}

// ── Sidebar ──────────────────────────────────────────────────────────

/// Sidebar display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SidebarMode {
    /// Full sidebar with entries, token bar, help hints.
    Normal,
    /// Collapsed to a narrow icon strip.
    Collapsed,
    /// Completely hidden.
    Hidden,
}

/// The left sidebar region.
#[derive(Debug, Clone, Serialize)]
pub struct Sidebar {
    /// Display mode.
    pub mode: SidebarMode,
    /// Context element entries.
    pub entries: Vec<SidebarEntry>,
    /// Token usage bar.
    pub token_bar: Option<TokenBar>,
    /// Token statistics breakdown.
    pub token_stats: Option<TokenStats>,
    /// Active PR card.
    pub pr_card: Option<PrCard>,
    /// Keyboard help hints.
    pub help_hints: Vec<HelpHint>,
}

/// A single context element entry in the sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct SidebarEntry {
    /// Panel ID (e.g. "P1", "P7").
    pub id: String,
    /// Icon character.
    pub icon: String,
    /// Short label (e.g. "wip", "tree", "file (main.rs)").
    pub label: String,
    /// Token count for this element.
    pub tokens: u32,
    /// Whether this entry is currently selected / active.
    pub active: bool,
    /// Whether this panel is frozen (cache-preserved).
    pub frozen: bool,
    /// Whether this is a fixed (built-in) panel vs a dynamic one.
    pub fixed: bool,
    /// Badge text (e.g. page "1/3", unread count).
    pub badge: Option<String>,
}

/// Token usage gauge bar.
#[derive(Debug, Clone, Serialize)]
pub struct TokenBar {
    /// Segments of the bar (system, tools, panels, messages, etc.).
    pub segments: Vec<ProgressSegment>,
    /// Total tokens used.
    pub used: u32,
    /// Budget limit.
    pub budget: u32,
    /// Cleaning threshold.
    pub threshold: u32,
}

/// Breakdown of token usage statistics.
#[derive(Debug, Clone, Serialize)]
pub struct TokenStats {
    /// Individual stat rows.
    pub rows: Vec<TokenRow>,
    /// Total cost in USD for this conversation.
    pub total_cost: Option<f64>,
}

/// A single row in the token stats breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct TokenRow {
    /// Label (e.g. "tot", "strm", "tick").
    pub label: String,
    /// Cache-hit tokens.
    pub hit: u32,
    /// Cache-miss tokens.
    pub miss: u32,
    /// Output tokens.
    pub output: u32,
    /// Cost for cache-hit tokens (USD).
    pub hit_cost: Option<f64>,
    /// Cost for cache-miss tokens (USD).
    pub miss_cost: Option<f64>,
    /// Cost for output tokens (USD).
    pub output_cost: Option<f64>,
}

/// Pull request summary card shown in sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct PrCard {
    /// PR number.
    pub number: u32,
    /// PR title.
    pub title: String,
    /// Lines added.
    pub additions: u32,
    /// Lines removed.
    pub deletions: u32,
    /// Review status (e.g. "Approved", "Changes requested").
    pub review_status: Option<String>,
    /// CI check status (e.g. "passing", "failing").
    pub checks_status: Option<String>,
}

/// A keyboard shortcut hint shown at bottom of sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct HelpHint {
    /// Key combination (e.g. "Tab", "Ctrl+P").
    pub key: String,
    /// Description of what the key does.
    pub description: String,
}

// ── Status bar ───────────────────────────────────────────────────────

/// Bottom status bar — single-line information strip.
#[derive(Debug, Clone, Serialize)]
pub struct StatusBar {
    /// Primary status badge (e.g. "Streaming", "Ready").
    pub badge: Badge,
    /// LLM provider name (e.g. "Claude", "OAuth", "Grok").
    pub provider: Option<String>,
    /// Active model name.
    pub model: Option<String>,
    /// Active agent card.
    pub agent: Option<AgentCard>,
    /// Loaded skills.
    pub skills: Vec<SkillCard>,
    /// Git branch + changes summary.
    pub git: Option<GitChanges>,
    /// Auto-continuation indicator.
    pub auto_continue: Option<AutoContinue>,
    /// Active reverie sub-agent cards (multiple concurrent possible).
    pub reveries: Vec<ReverieCard>,
    /// Queue status.
    pub queue: Option<QueueCard>,
    /// Stop reason from last completion.
    pub stop_reason: Option<StopReason>,
    /// API retry count (0 = no retry in progress).
    pub retry_count: u8,
    /// Max retries allowed.
    pub max_retries: u8,
    /// Number of panels currently loading.
    pub loading_count: u16,
    /// Character count of current input text.
    pub input_char_count: u32,
}

/// Primary status badge.
#[derive(Debug, Clone, Serialize)]
pub struct Badge {
    /// Display text.
    pub label: String,
    /// Semantic colour.
    pub semantic: Semantic,
}

/// Stop reason indicator.
#[derive(Debug, Clone, Serialize)]
pub struct StopReason {
    /// Reason text (e.g. "end\_turn", "max\_tokens").
    pub reason: String,
    /// Semantic colour.
    pub semantic: Semantic,
}

/// Active agent indicator.
#[derive(Debug, Clone, Serialize)]
pub struct AgentCard {
    /// Agent display name.
    pub name: String,
}

/// Active skill indicator.
#[derive(Debug, Clone, Serialize)]
pub struct SkillCard {
    /// Skill display name.
    pub name: String,
}

/// Git branch and changes summary.
#[derive(Debug, Clone, Serialize)]
pub struct GitChanges {
    /// Current branch name.
    pub branch: String,
    /// Number of files changed.
    pub files_changed: u32,
    /// Lines added.
    pub additions: u32,
    /// Lines removed.
    pub deletions: u32,
}

/// Auto-continuation status.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AutoContinue {
    /// Current continuation count.
    pub count: u32,
    /// Maximum allowed continuations (if set).
    pub max: Option<u32>,
}

/// Active reverie sub-agent indicator.
#[derive(Debug, Clone, Serialize)]
pub struct ReverieCard {
    /// Agent name driving the reverie.
    pub agent: String,
    /// Tool calls completed so far.
    pub tool_count: u32,
}

/// Queue status indicator.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct QueueCard {
    /// Number of queued actions.
    pub count: u32,
    /// Whether the queue is actively intercepting.
    pub active: bool,
}

// ── Panel content ────────────────────────────────────────────────────

/// Generic panel content — the right-pane region.
///
/// Built from `Panel::blocks()` + metadata. The adapter renders this
/// as a bordered, scrollable area.
#[derive(Debug, Clone, Serialize)]
pub struct PanelContent {
    /// Panel title (shown in border).
    pub title: String,
    /// Content blocks.
    pub blocks: Vec<Block>,
    /// "Refreshed N ago" timestamp text, if applicable.
    pub refreshed_ago: Option<String>,
}
