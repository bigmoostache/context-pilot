//! State types split into domain-focused modules.
//!
//! - `context` — ContextType, ContextElement, estimate_tokens, compute_total_pages
//! - `message` — MessageType, MessageStatus, Message, ToolUseRecord, ToolResultRecord
//! - `config` — SharedConfig, WorkerState, PanelData, ImportantPanelUids
//! - `render_cache` — MessageRenderCache, InputRenderCache, FullContentCache, hash_values
//! - `runtime` — State struct (the main runtime state)

pub mod config;
pub mod context;
pub mod message;
pub mod render_cache;
pub mod runtime;

// === Re-exports for backwards compatibility ===
// All existing `use crate::state::X` imports continue to work.

pub use config::{PanelData, SharedConfig, WorkerState};
pub use context::{ContextElement, ContextType, compute_total_pages, estimate_tokens};
pub use message::{Message, MessageStatus, MessageType, ToolResultRecord, ToolUseRecord, format_messages_to_chunk};
pub use render_cache::{FullContentCache, InputRenderCache, MessageRenderCache, hash_values};
pub use runtime::State;

// Re-export module-owned types (originally re-exported from old state.rs)
pub use crate::modules::git::types::{GitChangeType, GitFileChange};
pub use crate::modules::memory::types::{MemoryImportance, MemoryItem};
pub use crate::modules::prompt::types::PromptItem;
pub use crate::modules::scratchpad::types::ScratchpadCell;
pub use crate::modules::todo::types::{TodoItem, TodoStatus};
pub use crate::modules::tree::types::TreeFileDescription;
