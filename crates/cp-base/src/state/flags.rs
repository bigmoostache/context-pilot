//! Stream-phase state machine, UI/config/lifecycle boolean flags, and streaming-tool advisory state.
//!
//! Extracted from `runtime.rs` to keep the main `State` file under the line-length limit.
//! All types here are re-used by `State` (composed, not inherited).

/// Type alias for the syntax highlighting callback function.
/// Takes (`file_path`, content) and returns highlighted spans per line: Vec<Vec<(Color, String)>>
pub type HighlightFn = fn(&str, &str) -> std::sync::Arc<Vec<Vec<(ratatui::style::Color, String)>>>;

/// IR-aware syntax highlighting callback.
/// Takes (`file_path`, content) and returns IR spans per line (RGB colour override).
pub type HighlightIrFn = fn(&str, &str) -> std::sync::Arc<Vec<Vec<cp_render::Span>>>;

/// The phase of the LLM stream lifecycle.
///
/// Encodes the only three legal combinations of the old `is_streaming` / `is_tooling`
/// booleans. The fourth combination (`tooling=true, streaming=false`) was always
/// illegal — this enum makes it unrepresentable.
///
/// Transitions are tracked via [`StreamPhase::transition`] using `#[track_caller]`
/// so every state change logs its source location automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamPhase {
    /// Not streaming — between conversation turns.
    #[default]
    Idle,
    /// Actively receiving tokens from the LLM.
    Receiving,
    /// Stream is active but currently executing tool calls.
    ExecutingTools,
}

impl StreamPhase {
    /// Transition to a new phase, recording the caller's source location.
    ///
    /// This is the **only** way to change the stream phase. Every callsite is
    /// automatically captured via `#[track_caller]` — no manual strings needed.
    /// Enable the `RUST_LOG=trace` env var (once a logger is wired) to see transitions.
    #[track_caller]
    pub fn transition(&mut self, to: Self) {
        let from = *self;
        if from != to {
            let loc = std::panic::Location::caller();
            // No-op until a log backend (env_logger, tracing, etc.) is registered in the binary.
            log::trace!("[StreamPhase] {from:?} → {to:?} ({}:{})", loc.file(), loc.line(),);
        }
        *self = to;
    }

    /// Whether we're in any streaming state (receiving tokens or executing tools).
    #[must_use]
    pub const fn is_streaming(self) -> bool {
        matches!(self, Self::Receiving | Self::ExecutingTools)
    }

    /// Whether we're currently executing tool calls (subset of streaming).
    #[must_use]
    pub const fn is_tooling(self) -> bool {
        matches!(self, Self::ExecutingTools)
    }
}

/// Stream-related state: the current [`StreamPhase`] plus independent scroll tracking.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamState {
    /// Current phase of the LLM stream lifecycle.
    pub phase: StreamPhase,
    /// Whether the user has manually scrolled (disables auto-scroll to bottom).
    pub user_scrolled: bool,
}

/// UI and lifecycle status flags — separated from [`StreamState`] to stay under
/// clippy's 3-bool threshold per struct.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiState {
    /// Whether the UI needs to be redrawn.
    pub dirty: bool,
    /// Dev mode — shows additional debug info like token counts.
    pub dev_mode: bool,
    /// Performance monitoring overlay enabled (F12 to toggle).
    pub perf_enabled: bool,
}

/// Configuration overlay flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigOverlay {
    /// Configuration view is open (Ctrl+H to toggle).
    pub config_view: bool,
    /// Whether config overlay is showing secondary model selection (Tab toggles).
    pub config_secondary_mode: bool,
    /// Whether the reverie system is enabled (auto-trigger on threshold breach).
    pub reverie_enabled: bool,
}

/// Lifecycle flags for async operations and reload state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Lifecycle {
    /// Whether an API check is in progress.
    pub api_check_in_progress: bool,
    /// Reload pending (set by `system_reload`, triggers reload after tool result saved).
    pub reload_pending: bool,
    /// Waiting for file panels to load before continuing stream.
    pub waiting_for_panels: bool,
}

/// Composite of all boolean status flags, organized by domain.
///
/// Access individual flags via domain sub-structs: `flags.stream.is_streaming`,
/// `flags.ui.dirty`, `flags.config.reverie_enabled`, `flags.lifecycle.reload_pending`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusBools {
    /// Streaming and scrolling state.
    pub stream: StreamState,
    /// UI rendering and debug toggles.
    pub ui: UiState,
    /// Configuration overlay state.
    pub config: ConfigOverlay,
    /// Async operation and reload lifecycle.
    pub lifecycle: Lifecycle,
}

/// Advisory state for a tool call currently being streamed by the LLM.
///
/// Populated from [`StreamEvent::ToolProgress`] events, cleared on
/// [`StreamEvent::ToolUse`] or [`StreamEvent::Done`]. Pure UI — has
/// no effect on tool execution.
#[derive(Debug, Clone, Default)]
pub struct StreamingTool {
    /// Tool name (e.g., `"Edit"`, `"Open"`). Known from `content_block_start`.
    pub name: String,
    /// Accumulated partial JSON input (grows with each `input_json_delta`).
    pub input_so_far: String,
}
