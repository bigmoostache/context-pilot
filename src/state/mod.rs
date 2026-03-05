//! State types — re-exported from cp-base shared library.
//!
//! All types live in `cp_base::state`. This module re-exports them so that
//! existing `crate::state::X` imports throughout the binary keep working.

// ── Re-exports from cp_base sub-modules ──
pub(crate) use cp_base::state::{
    ContextElement, ContextType, ContextTypeMeta, FullContentCache, InputRenderCache, Message, MessageRenderCache,
    MessageStatus, MessageType, PanelData, SharedConfig, State, StreamPhase, StreamingTool, ToolResultRecord,
    ToolUseRecord, WorkerState, compute_total_pages, estimate_tokens, fixed_panel_order, format_messages_to_chunk,
    get_context_type_meta, hash_values, init_context_type_registry, make_default_context_element,
};

// ── Submodule re-exports (accessed via path, e.g. crate::state::config::SCHEMA_VERSION) ──
pub(crate) use cp_base::state::data::config;

// ── Local submodules ──
pub(crate) mod cache;
pub(crate) mod persistence;
