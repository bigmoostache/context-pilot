//! State types split into domain-focused modules.
//!
//! - `context` — ContextType, ContextElement, estimate_tokens, compute_total_pages
//! - `message` — MessageType, MessageStatus, Message, ToolUseRecord, ToolResultRecord
//! - `config` — SharedConfig, WorkerState, PanelData, ImportantPanelUids
//! - `render_cache` — MessageRenderCache, InputRenderCache, FullContentCache, hash_values
//! - `runtime` — State struct (the main runtime state)

pub mod context;
pub mod message;
pub mod config;
pub mod render_cache;
pub mod runtime;

// === Re-exports for backwards compatibility ===
// All existing `use crate::state::X` imports continue to work.

pub use context::{ContextType, ContextElement, estimate_tokens, compute_total_pages};
pub use message::{MessageType, MessageStatus, Message, ToolUseRecord, ToolResultRecord};
pub use config::{SharedConfig, WorkerState, PanelData};
pub use render_cache::{MessageRenderCache, InputRenderCache, FullContentCache, hash_values};
pub use runtime::State;

// Re-export module-owned types (originally re-exported from old state.rs)
pub use crate::modules::todo::types::{TodoStatus, TodoItem};
pub use crate::modules::memory::types::{MemoryImportance, MemoryItem};
pub use crate::modules::prompt::types::SystemItem;
pub use crate::modules::scratchpad::types::ScratchpadCell;
pub use crate::modules::tree::types::TreeFileDescription;
pub use crate::modules::git::types::{GitFileChange, GitChangeType};
