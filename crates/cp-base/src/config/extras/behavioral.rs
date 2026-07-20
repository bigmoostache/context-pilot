//! Behavioral redirect and history-cleanup trap injection types.
//!
//! Extracted from `config/mod.rs` to keep that file under the 500-line limit.

use serde::Deserialize;

/// Behavioral redirects injected to steer the LLM toward correct tools.
#[derive(Debug, Deserialize)]
#[non_exhaustive]
pub struct RedirectInjections {
    /// Tells the LLM to use `Close_conversation_history` instead of `Close_panel`.
    pub conversation_history_close: String,
}

/// Messages for the history cleanup trap — forces the AI to close old
/// conversation history panels before a queued batch can execute.
#[derive(Debug, Deserialize)]
#[non_exhaustive]
pub struct TrapInjections {
    /// Shown when the trap triggers (≥4 history panels at queue flush time).
    pub history_cleanup_triggered: String,
    /// Shown when the AI tries any non-allowed tool while the trap is active.
    pub history_cleanup_blocked: String,
}
