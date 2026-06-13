/// User or system action dispatched through the event loop.
/// Each variant maps to a keybinding, mouse event, or internal trigger.
#[derive(Debug, Clone)]
pub enum Action {
    // === Text input ===
    /// Single character typed into the input field.
    InputChar(char),
    /// Multi-character insert (e.g., bracketed paste chunk).
    InsertText(String),
    /// Paste from clipboard (triggers paste-sentinel expansion).
    PasteText(String),
    /// Delete character before cursor.
    InputBackspace,
    /// Delete character after cursor.
    InputDelete,
    /// Submit the current input (Enter key).
    InputSubmit,
    /// Move cursor one word left (Ctrl+Left).
    CursorWordLeft,
    /// Move cursor one word right (Ctrl+Right).
    CursorWordRight,
    /// Delete word before cursor (Ctrl+Backspace).
    DeleteWordLeft,
    /// Remove an empty list continuation marker, keep the newline.
    RemoveListItem,
    /// Move cursor to start of line (Home).
    CursorHome,
    /// Move cursor to end of line (End).
    CursorEnd,
    /// Move cursor one character left (Left arrow).
    CursorLeft,
    /// Move cursor one character right (Right arrow).
    CursorRight,
    /// Select one character left (Shift+Left).
    CursorLeftSelect,
    /// Select one character right (Shift+Right).
    CursorRightSelect,
    /// Extend selection one word left (Shift+Ctrl/Alt+Left).
    CursorWordLeftSelect,
    /// Extend selection one word right (Shift+Ctrl/Alt+Right).
    CursorWordRightSelect,
    /// Extend selection to start of line (Shift+Home).
    CursorHomeSelect,
    /// Extend selection to end of line (Shift+End).
    CursorEndSelect,
    /// Select all text in input (Ctrl+A).
    SelectAll,
    /// Navigate to previous (older) prompt in history (Ctrl+U).
    HistoryPrev,
    /// Navigate to next (newer) prompt in history (Ctrl+D).
    HistoryNext,
    /// Copy current panel content to clipboard (Ctrl+C).
    CopyPanelContent,

    // === Conversation lifecycle ===
    /// Discard all messages and start fresh.
    ClearConversation,
    /// Create a new worker context.
    NewContext,
    /// Switch to the next context panel (Tab).
    SelectNextContext,
    /// Switch to the previous context panel (Shift+Tab).
    SelectPrevContext,

    // === Streaming ===
    /// Append text chunk from LLM stream to the current assistant message.
    AppendChars(String),
    /// Stream finished — carries final token accounting.
    StreamDone {
        /// Input tokens consumed by the prompt.
        input_tokens: usize,
        /// Tokens generated in the response.
        output_tokens: usize,
        /// Input tokens served from provider cache.
        cache_hit_tokens: usize,
        /// Input tokens written to cache on this call.
        cache_miss_tokens: usize,
        /// Provider stop reason (e.g., `"end_turn"`, `"tool_use"`).
        stop_reason: Option<String>,
    },
    /// Unrecoverable stream error.
    StreamError(String),

    // === Scroll ===
    /// Scroll conversation up by `f32` lines.
    ScrollUp(f32),
    /// Scroll conversation down by `f32` lines.
    ScrollDown(f32),

    // === Control ===
    /// Interrupt the active LLM stream (Esc).
    StopStreaming,
    /// Send keystrokes to a tmux pane (legacy, unused).
    TmuxSendKeys {
        /// Target tmux pane identifier.
        pane_id: String,
        /// Key sequence to send.
        keys: String,
    },
    /// Toggle the F12 performance overlay.
    TogglePerfMonitor,
    /// Toggle the config/settings overlay (F1).
    ToggleConfigView,
    /// Toggle the Meilisearch indexing status overlay (Ctrl+I).
    ToggleIndexOverlay,
    /// Copy the index overlay content to the system clipboard (Ctrl+C while overlay is open).
    CopyIndexOverlay,

    // === Config overlay — primary model ===
    /// Select primary LLM provider.
    ConfigSelectProvider(crate::config::llm_types::LlmProvider),
    /// Select primary Anthropic model.
    ConfigSelectAnthropicModel(crate::config::llm_types::AnthropicModel),
    /// Select primary Grok model.
    ConfigSelectGrokModel(crate::config::llm_types::GrokModel),
    /// Select primary Groq model.
    ConfigSelectGroqModel(crate::config::llm_types::GroqModel),
    /// Select primary `DeepSeek` model.
    ConfigSelectDeepSeekModel(crate::config::llm_types::DeepSeekModel),
    /// Select primary `MiniMax` model.
    ConfigSelectMiniMaxModel(crate::config::llm_types::MiniMaxModel),
    /// Select primary Claude Code V2 model.
    ConfigSelectClaudeCodeV2Model(crate::config::llm_types::ClaudeCodeV2Model),
    /// Move config bar selection forward (→).
    ConfigSelectNextBar,
    /// Move config bar selection backward (←).
    ConfigSelectPrevBar,
    /// Increase the selected config bar value (↑).
    ConfigIncreaseSelectedBar,
    /// Decrease the selected config bar value (↓).
    ConfigDecreaseSelectedBar,
    /// Cycle to next theme.
    ConfigNextTheme,
    /// Cycle to previous theme.
    ConfigPrevTheme,
    /// Toggle spine auto-continuation on/off.
    ConfigToggleAutoContinue,
    /// Make think reminder threshold less negative (more frequent reminders).
    ConfigThinkThresholdUp,
    /// Make think reminder threshold more negative (less frequent reminders).
    ConfigThinkThresholdDown,

    // === Config overlay — secondary model ===
    /// Select secondary (reverie) LLM provider.
    ConfigSelectSecondaryProvider(crate::config::llm_types::LlmProvider),
    /// Select secondary Anthropic model.
    ConfigSelectSecondaryAnthropicModel(crate::config::llm_types::AnthropicModel),
    /// Select secondary Grok model.
    ConfigSelectSecondaryGrokModel(crate::config::llm_types::GrokModel),
    /// Select secondary Groq model.
    ConfigSelectSecondaryGroqModel(crate::config::llm_types::GroqModel),
    /// Select secondary `DeepSeek` model.
    ConfigSelectSecondaryDeepSeekModel(crate::config::llm_types::DeepSeekModel),
    /// Select secondary `MiniMax` model.
    ConfigSelectSecondaryMiniMaxModel(crate::config::llm_types::MiniMaxModel),
    /// Select secondary Claude Code V2 model.
    ConfigSelectSecondaryClaudeCodeV2Model(crate::config::llm_types::ClaudeCodeV2Model),
    /// Toggle reverie (background optimizer) on/off.
    ConfigToggleReverie,
    /// Toggle between primary and secondary model tabs.
    ConfigToggleSecondaryMode,

    // === UI ===
    /// Jump to first dynamic panel on the next page (Shift+Right).
    PageDynamicNext,
    /// Jump to first dynamic panel on the previous page (Shift+Left).
    PageDynamicPrev,

    /// Cycle view mode (Normal → Collapsed → Hidden → Threads → Normal).
    CycleViewMode,
    /// Navigate to next thread in Threads view (↓ / Tab).
    ThreadSelectNext,
    /// Navigate to previous thread in Threads view (↑ / Shift+Tab).
    ThreadSelectPrev,
    /// Start creating a new thread — switches input to thread-naming mode.
    ThreadCreateStart,
    /// Cancel thread creation — clears naming mode without creating.
    ThreadCreateCancel,
    /// Start archiving (deleting) the selected thread — shows confirmation.
    ThreadArchiveStart,
    /// Confirm thread archive — removes the selected thread.
    ThreadArchiveConfirm,
    /// Cancel thread archive — dismisses the confirmation.
    ThreadArchiveCancel,
    /// Open the Ctrl+P command palette.
    OpenCommandPalette,
    /// Reset the session cost counters to zero.
    ResetSessionCosts,
    /// Jump to a specific context panel by ID string (e.g., `"P3"`).
    SelectContextById(String),
    /// No-op — used as a default / placeholder.
    None,
}

/// Outcome of processing an [`Action`] — tells the event loop what to do next.
#[derive(Debug)]
pub enum ActionResult {
    /// No further work needed.
    Nothing,
    /// Interrupt the active LLM stream.
    StopStream,
    /// Trigger an API connectivity check for the current provider.
    StartApiCheck,
    /// Persist state to disk.
    Save,
    /// Persist state and show a status-bar message.
    SaveMessage(String),
}
