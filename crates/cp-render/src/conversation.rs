//! Conversation and overlay IR types.
//!
//! These types model the conversation region (message history, streaming
//! tool calls, input area) and modal overlays (question forms,
//! autocomplete popups).

use serde::Serialize;

use crate::{Block, Semantic};

// ── Conversation ─────────────────────────────────────────────────────

/// The conversation region — message history + input area.
#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    /// Collapsed history sections (previous conversations).
    pub history_sections: Vec<HistorySection>,
    /// Visible messages.
    pub messages: Vec<Message>,
    /// Currently streaming tool calls.
    pub streaming_tools: Vec<StreamingTool>,
    /// Input area at the bottom.
    pub input: InputArea,
}

/// A collapsed history section header.
#[derive(Debug, Clone, Serialize)]
pub struct HistorySection {
    /// Display label (e.g. "History (23 messages)").
    pub label: String,
    /// Whether this section is expanded.
    pub expanded: bool,
    /// Messages inside this section (only present when expanded).
    pub messages: Vec<Message>,
}

/// A single conversation message.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    /// Role: "user", "assistant", "system".
    pub role: String,
    /// Content blocks (rendered as IR blocks, not raw markdown).
    pub content: Vec<Block>,
    /// Tool use previews attached to this message.
    pub tool_uses: Vec<ToolUsePreview>,
    /// Tool result previews attached to this message.
    pub tool_results: Vec<ToolResultPreview>,
}

/// Preview of a tool use (collapsed in conversation view).
#[derive(Debug, Clone, Serialize)]
pub struct ToolUsePreview {
    /// Tool name (e.g. `Edit`, `console_easy_bash`).
    pub tool_name: String,
    /// Short summary (e.g. "src/main.rs: 3 lines changed").
    pub summary: String,
    /// Semantic colour (success/error/info based on result).
    pub semantic: Semantic,
}

/// Preview of a tool result (collapsed in conversation view).
#[derive(Debug, Clone, Serialize)]
pub struct ToolResultPreview {
    /// Tool name.
    pub tool_name: String,
    /// Short result summary.
    pub summary: String,
    /// Whether the tool call succeeded.
    pub success: bool,
}

/// A tool call currently being streamed.
#[derive(Debug, Clone, Serialize)]
pub struct StreamingTool {
    /// Tool name.
    pub tool_name: String,
    /// Partial input JSON accumulated so far.
    pub partial_input: String,
}

/// The input area at the bottom of the conversation.
#[derive(Debug, Clone, Serialize)]
pub struct InputArea {
    /// Current input text.
    pub text: String,
    /// Cursor position (byte offset).
    pub cursor: usize,
    /// Placeholder text when input is empty.
    pub placeholder: String,
    /// Whether input is currently focused.
    pub focused: bool,
}

// ── Overlays ─────────────────────────────────────────────────────────

/// A modal overlay rendered on top of the main UI.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub enum Overlay {
    /// Multiple-choice question form.
    QuestionForm(QuestionForm),
    /// File path autocomplete popup.
    Autocomplete(Autocomplete),
    /// Performance monitoring overlay (F12).
    Perf(PerfOverlay),
    /// Configuration overlay (Ctrl+H).
    Config(ConfigOverlay),
    /// Command palette overlay (Ctrl+P / Ctrl+K).
    CommandPalette(PaletteOverlay),
    /// Meilisearch indexing status overlay (Ctrl+I).
    SearchIndex(Box<SearchIndexOverlay>),
}

/// A question form overlay (`ask_user_question`).
#[derive(Debug, Clone, Serialize)]
pub struct QuestionForm {
    /// Questions to display.
    pub questions: Vec<Question>,
    /// Index of the currently focused question.
    pub focused_index: usize,
}

/// A single question in the form.
#[derive(Debug, Clone, Serialize)]
pub struct Question {
    /// Short header label.
    pub header: String,
    /// Full question text.
    pub text: String,
    /// Available options.
    pub options: Vec<QuestionOption>,
    /// Whether multiple selections are allowed.
    pub multi_select: bool,
    /// Index of the cursor (which option the cursor is on).
    pub cursor: usize,
    /// Indices of currently selected options.
    pub selected: Vec<usize>,
    /// Whether the user is typing in the "Other" free-text field.
    pub typing_other: bool,
    /// Free-text "Other" input value.
    pub other_text: String,
}

/// A single option in a question.
#[derive(Debug, Clone, Serialize)]
pub struct QuestionOption {
    /// Display label.
    pub label: String,
    /// Description text.
    pub description: String,
}

/// Performance monitoring overlay (F12).
#[derive(Debug, Clone, Serialize)]
pub struct PerfOverlay {
    /// Frames per second.
    pub fps: f64,
    /// Average frame time in milliseconds.
    pub frame_avg_ms: f64,
    /// Maximum frame time in milliseconds.
    pub frame_max_ms: f64,
    /// Semantic colour for frame time (green/yellow/red).
    pub frame_semantic: Semantic,
    /// CPU usage percentage (0–100).
    pub cpu_usage: f32,
    /// Semantic colour for CPU usage.
    pub cpu_semantic: Semantic,
    /// Memory usage in megabytes.
    pub memory_mb: f64,
    /// Optional Meilisearch process stats.
    pub meili: Option<PerfMeiliStats>,
    /// Budget bars (e.g. 60fps, 30fps).
    pub budget_bars: Vec<PerfBudgetBar>,
    /// Recent frame times for sparkline (milliseconds).
    pub sparkline: Vec<f64>,
    /// Top operations sorted by cumulative time.
    pub operations: Vec<PerfOp>,
}

/// Meilisearch process stats for perf overlay.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PerfMeiliStats {
    /// CPU usage percentage.
    pub cpu_pct: f64,
    /// Semantic colour for CPU usage.
    pub cpu_semantic: Semantic,
    /// Memory usage in megabytes.
    pub memory_mb: f64,
}

/// A budget bar in the perf overlay.
#[derive(Debug, Clone, Serialize)]
pub struct PerfBudgetBar {
    /// Label (e.g. "60fps", "30fps").
    pub label: String,
    /// Current usage as percentage of budget (0–150).
    pub percent: f64,
    /// Semantic colour (green/yellow/red).
    pub semantic: Semantic,
}

/// A single operation row in the perf overlay table.
#[derive(Debug, Clone, Serialize)]
pub struct PerfOp {
    /// Operation name.
    pub name: String,
    /// Mean execution time in milliseconds.
    pub mean_ms: f64,
    /// Semantic colour for mean time.
    pub mean_semantic: Semantic,
    /// Standard deviation in milliseconds.
    pub std_ms: f64,
    /// Semantic colour for std deviation.
    pub std_semantic: Semantic,
    /// Pre-formatted cumulative time (e.g. "1.2s", "450ms").
    pub total_display: String,
    /// Whether this operation is a hotspot (>30% of total).
    pub is_hotspot: bool,
}

/// Configuration overlay (Ctrl+H).
#[derive(Debug, Clone, Serialize)]
pub struct ConfigOverlay {
    /// Whether showing secondary (reverie) model tab.
    pub secondary_mode: bool,
    /// LLM provider entries.
    pub providers: Vec<ConfigProvider>,
    /// Section title for the model list (e.g. "Model" or "Secondary Model (Reverie)").
    pub model_section_title: String,
    /// Model entries for the active provider.
    pub models: Vec<ConfigModel>,
    /// Budget bars.
    pub budget_bars: Vec<ConfigBudgetBar>,
    /// Index of the currently selected budget bar (0-based).
    pub selected_bar: usize,
    /// Toggle switches.
    pub toggles: Vec<ConfigToggle>,
}

/// A provider entry in the config overlay.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigProvider {
    /// Key hint to press (e.g. "1", "2").
    pub key: String,
    /// Display name (e.g. "Anthropic Claude").
    pub name: String,
    /// Whether this provider is currently selected.
    pub selected: bool,
}

/// A model entry in the config overlay.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigModel {
    /// Key hint to press (e.g. "a", "b").
    pub key: String,
    /// Display name (e.g. "Opus 4.5").
    pub name: String,
    /// Context window size (e.g. "200K").
    pub context_window: String,
    /// Pricing string (e.g. "$3/$15").
    pub pricing: String,
    /// Whether this model is currently selected.
    pub selected: bool,
}

/// A budget bar in the config overlay.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigBudgetBar {
    /// Display label (e.g. "Context Budget", "Clean Trigger").
    pub label: String,
    /// Percentage value (0–100) shown beside the bar.
    pub percent: usize,
    /// Fill ratio (0.0–1.0) for the bar.
    pub fill_ratio: f64,
    /// Value display string (e.g. "128K tok", "$5.00").
    pub value_display: String,
    /// Optional extra text (e.g. "(85%)").
    pub extra: Option<String>,
    /// Semantic colour for the filled portion.
    pub semantic: Semantic,
    /// Whether this bar is currently selected for adjustment.
    pub selected: bool,
}

/// A toggle switch in the config overlay.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigToggle {
    /// Label (e.g. "Auto-continue").
    pub label: String,
    /// Whether the toggle is on.
    pub enabled: bool,
    /// Status display text (e.g. "ON", "OFF", "-5").
    pub value_display: String,
    /// Key hint to toggle (e.g. "s", "r").
    pub key_hint: String,
    /// Optional second key for adjustment (e.g. `[` and `]`).
    pub adjust_keys: Option<(String, String)>,
}

/// Command palette overlay (Ctrl+P / Ctrl+K).
#[derive(Debug, Clone, Serialize)]
pub struct PaletteOverlay {
    /// Current search query.
    pub query: String,
    /// Cursor byte position in query.
    pub cursor: usize,
    /// Filtered command entries.
    pub entries: Vec<PaletteEntry>,
    /// Index of the selected entry.
    pub selected_index: usize,
}

/// A single command palette entry.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteEntry {
    /// Command label (e.g. "Close panel").
    pub label: String,
    /// Short description.
    pub description: String,
}

/// File path autocomplete popup.
#[derive(Debug, Clone, Serialize)]
pub struct Autocomplete {
    /// Current query / prefix.
    pub query: String,
    /// Matching entries (visible window after scrolling).
    pub entries: Vec<AutocompleteEntry>,
    /// Index of the highlighted entry (relative to `entries`).
    pub selected_index: usize,
    /// Directory prefix for the title (e.g. "src/ui").
    pub dir_prefix: String,
    /// Total number of matches (before scroll windowing).
    pub total_matches: usize,
    /// Number of visual lines the input occupies (for popup positioning).
    pub input_visual_lines: u16,
}

/// A single autocomplete suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct AutocompleteEntry {
    /// Display text (file name or path).
    pub label: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Icon character.
    pub icon: String,
}

// ── Search index overlay ─────────────────────────────────────────────

/// Meilisearch indexing status overlay (Ctrl+I).
#[derive(Debug, Clone, Serialize)]
pub struct SearchIndexOverlay {
    /// Title text (may include "✓ Copied!" flash).
    pub title: String,
    /// Footer hint text.
    pub footer: String,
    /// Server status section.
    pub server: SearchServer,
    /// Core index statistics.
    pub index: SearchIndex,
    /// Extension breakdown (bar chart).
    pub extensions: Vec<SearchExtension>,
    /// Splitter statistics (tree-sitter vs fallback).
    pub splitter: Option<SearchSplitter>,
    /// Embedding statistics.
    pub embeddings: Option<SearchEmbeddings>,
    /// Recent Meilisearch tasks.
    pub recent_tasks: Vec<SearchTask>,
    /// Top recomputed files.
    pub top_recomputed: Vec<SearchRecomputed>,
    /// Recently sent files.
    pub recently_sent: Vec<SearchRecentFile>,
}

/// Server status for the search index overlay.
#[derive(Debug, Clone, Serialize)]
pub struct SearchServer {
    /// Server URL (e.g. `http://127.0.0.1:49166`).
    pub url: String,
    /// Whether the server is online.
    pub online: bool,
    /// Meilisearch version string.
    pub version: String,
    /// CPU usage percentage (if available).
    pub cpu_pct: Option<f64>,
    /// Semantic colour for CPU usage.
    pub cpu_semantic: Semantic,
    /// Memory display string (e.g. "215 MB"), if available.
    pub memory_display: Option<String>,
}

/// Core index statistics for the search overlay.
#[derive(Debug, Clone, Serialize)]
pub struct SearchIndex {
    /// Number of files indexed.
    pub files_indexed: u64,
    /// Number of chunks indexed.
    pub chunks_indexed: u64,
    /// Pending queue depth.
    pub queue_depth: u64,
    /// Error count.
    pub error_count: u64,
    /// Whether the index is ready (vs scanning).
    pub index_ready: bool,
    /// Last activity as relative string (e.g. "5s ago").
    pub last_activity: String,
    /// Used disk space display (e.g. "12 MB").
    pub disk_used: String,
    /// Total disk space display (e.g. "45 MB").
    pub disk_total: String,
    /// Document store size display (e.g. "8 MB").
    pub docs_display: String,
    /// Average chunk size display, if available.
    pub avg_chunk: Option<String>,
}

/// Extension entry for the bar chart.
#[derive(Debug, Clone, Serialize)]
pub struct SearchExtension {
    /// Extension name (e.g. "rs", "ts").
    pub name: String,
    /// File count.
    pub count: u64,
    /// Pre-computed bar width (character cells).
    pub bar_width: usize,
    /// Percentage of total files.
    pub pct: u64,
}

/// Splitter statistics (tree-sitter vs fixed-size fallback).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SearchSplitter {
    /// Tree-sitter chunk count.
    pub tree_sitter_chunks: u64,
    /// Tree-sitter percentage.
    pub tree_sitter_pct: u64,
    /// Fallback chunk count.
    pub fallback_chunks: u64,
    /// Fallback percentage.
    pub fallback_pct: u64,
}

/// Embedding statistics.
#[derive(Debug, Clone, Serialize)]
pub struct SearchEmbeddings {
    /// Model name (e.g. "voyage-code-3").
    pub model: String,
    /// Number of embedding vectors.
    pub vector_count: u64,
    /// Whether embeddings are currently being generated.
    pub is_indexing: bool,
    /// Number of embedded documents.
    pub embedded_docs: u64,
    /// Total number of documents.
    pub total_docs: u64,
    /// Coverage percentage.
    pub coverage_pct: u64,
    /// Semantic colour for coverage.
    pub coverage_semantic: Semantic,
    /// Number of log documents.
    pub logs_doc_count: u64,
}

/// A recent Meilisearch task entry.
#[derive(Debug, Clone, Serialize)]
pub struct SearchTask {
    /// Task UID.
    pub uid: u64,
    /// Task type (e.g. "indexAddition").
    pub task_type: String,
    /// Status string (e.g. "succeeded", "failed").
    pub status: String,
    /// Semantic colour for the status.
    pub status_semantic: Semantic,
    /// Duration display string.
    pub duration: String,
}

/// A recomputed file entry.
#[derive(Debug, Clone, Serialize)]
pub struct SearchRecomputed {
    /// File path (may be truncated).
    pub path: String,
    /// Recompute count.
    pub count: u64,
}

/// A recently sent file entry.
#[derive(Debug, Clone, Serialize)]
pub struct SearchRecentFile {
    /// File path (may be truncated).
    pub path: String,
    /// Relative time ago string.
    pub ago: String,
}
