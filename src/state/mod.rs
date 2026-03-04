//! State types — re-exported from cp-base shared library.
//!
//! All types live in `cp_base::state`. This module re-exports them so that
//! existing `crate::state::X` imports throughout the binary keep working.

// ── Wildcard re-export: State, ContextElement, ContextType, Message, etc. ──
pub(crate) use cp_base::state::*;

// ── Submodule re-exports (accessed via path, e.g. crate::state::config::SCHEMA_VERSION) ──
pub(crate) use cp_base::state::config;
#[cfg(test)]
pub(crate) use cp_base::state::message;

// ── Local submodules ──
pub(crate) mod cache;
pub(crate) mod persistence;
