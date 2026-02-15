//! Wait logic for ensuring panels are loaded before continuing stream.
//!
//! When the LLM uses tools like file_open, we need to wait for the background
//! cache system to load the file content before continuing the stream.

use std::sync::mpsc::Sender;

use crate::cache::{CacheUpdate, process_cache_request};
use crate::state::{ContextType, State};

/// Check if any File or Tmux context panels have cache_deprecated = true
pub fn has_dirty_panels(state: &State) -> bool {
    state
        .context
        .iter()
        .any(|c| (c.context_type == ContextType::File || c.context_type == ContextType::Tmux) && c.cache_deprecated)
}

/// Check if any File context panels have cache_deprecated = true
pub fn has_dirty_file_panels(state: &State) -> bool {
    state.context.iter().any(|c| c.context_type == ContextType::File && c.cache_deprecated)
}

/// Trigger immediate cache refresh for all dirty file panels.
/// Returns true if any panels needed refresh.
pub fn trigger_dirty_panel_refresh(state: &State, cache_tx: &Sender<CacheUpdate>) -> bool {
    let mut any_triggered = false;
    for ctx in &state.context {
        if ctx.context_type == ContextType::File && ctx.cache_deprecated && !ctx.cache_in_flight {
            let panel = crate::core::panels::get_panel(ctx.context_type);
            if let Some(request) = panel.build_cache_request(ctx, state) {
                process_cache_request(request, cache_tx.clone());
                any_triggered = true;
            }
        }
    }
    any_triggered
}
