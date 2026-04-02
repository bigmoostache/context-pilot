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
    /// Indices of currently selected options.
    pub selected: Vec<usize>,
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

/// File path autocomplete popup.
#[derive(Debug, Clone, Serialize)]
pub struct Autocomplete {
    /// Current query / prefix.
    pub query: String,
    /// Matching entries.
    pub entries: Vec<AutocompleteEntry>,
    /// Index of the highlighted entry.
    pub selected_index: usize,
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
