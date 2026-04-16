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
    /// Toggle reverie (background optimizer) on/off.
    ConfigToggleReverie,
    /// Toggle between primary and secondary model tabs.
    ConfigToggleSecondaryMode,

    // === UI ===
    /// Cycle sidebar display mode (Full → Collapsed → Hidden).
    CycleSidebarMode,
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
