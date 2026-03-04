//! Foundation crate for Context Pilot: shared types, traits, config, state, and panel/tool abstractions.
//!
//! All module crates depend on `cp-base` for common infrastructure.

/// Safe numeric casting helpers (saturating `as` replacements).
pub mod cast;
/// YAML config loader: prompts, library, themes, injections, constants.
pub mod config;
/// Re-export from config::llm_types for convenience.
pub mod llm_types {
    //! Re-export from config::llm_types for convenience.
    pub use crate::config::llm_types::*;
}
/// Module trait: tools, panels, lifecycle hooks for pluggable functionality.
pub mod modules;
/// Panel trait and caching infrastructure for context elements.
pub mod panels;
/// State types: runtime State, SharedConfig, WorkerState, Messages, Actions.
pub mod state;
/// Tool definition types and YAML-driven builder.
pub mod tools;
/// Shared UI helpers: table rendering, text cells, question forms.
pub mod ui;
/// Watcher trait and registry for async condition monitoring.
pub mod watchers {
    //! Re-export from state::watchers for convenience.
    pub use crate::state::watchers::*;
}

// Re-export autocomplete from state for convenience
/// File-path autocomplete state for @-triggered popup.
pub mod autocomplete {
    pub use crate::state::autocomplete::*;
}
