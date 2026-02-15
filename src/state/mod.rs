//! State types — re-exported from cp-base shared library.
//!
//! All types live in `cp_base::state`. This module re-exports them so that
//! existing `crate::state::X` imports throughout the binary keep working.

// ── Wildcard re-export: State, ContextElement, ContextType, Message, etc. ──
pub use cp_base::state::*;

// ── Submodule re-exports (accessed via path, e.g. crate::state::config::SCHEMA_VERSION) ──
pub use cp_base::state::config;
#[cfg(test)]
pub use cp_base::state::message;

// Module-owned types are already re-exported by cp_base::state::* above
// (GitFileChange, TodoItem, MemoryItem, etc. from cp_base::types::*)
