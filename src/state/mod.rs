//! State types — re-exported from cp-base shared library.
//!
//! All types live in `cp_base::state`. This module re-exports them so that
//! existing `crate::state::X` imports throughout the binary keep working.

// ── Re-exports from cp_base sub-modules ──
pub(crate) use cp_base::state::context::{
    Entry, Kind, TypeMeta, compute_total_pages, estimate_tokens, fixed_panel_order, get_context_type_meta,
    init_context_type_registry, make_default_entry,
};
pub(crate) use cp_base::state::data::config::{PanelData, Shared as SharedConfig, WorkerState};
pub(crate) use cp_base::state::data::message::{Message, MsgKind, MsgStatus, format_messages_to_chunk};
pub(crate) use cp_base::state::flags::{StreamPhase, StreamingTool};
pub(crate) use cp_base::state::runtime::State;
pub(crate) use cp_base::ui::render_cache::{FullCache, InputCache, MessageCache, hash_values};

// ── Submodule re-exports (accessed via path, e.g. crate::state::config::SCHEMA_VERSION) ──
pub(crate) use cp_base::state::data::config;

// Records re-export (used by conversation panel and persistence)
pub(crate) use cp_base::state::data::message::{ToolResultRecord, ToolUseRecord};

// ── Local submodules ──
pub(crate) mod cache;
pub(crate) mod persistence;
