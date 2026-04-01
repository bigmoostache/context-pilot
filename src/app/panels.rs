//! Panel trait and implementations for different context types.
//!
//! The `Panel` trait and core types live in `cp_base::panels`.
//! This module re-exports them and adds binary-specific functionality
//! (rendering with theme/profiling, panel registry).

use crate::state::{Kind, State};
use cp_base::panels::{CacheRequest, CacheUpdate};
use cp_base::state::context::Entry;

// Re-export the Panel trait, ContextItem, and utility functions from cp-base
pub(crate) use cp_base::panels::{ContextItem, Panel, now_ms, paginate_content, update_if_changed};

/// Get the appropriate panel for a context type (delegates to module system).
/// Returns a no-op fallback for orphaned context types (e.g., removed modules).
pub(crate) fn get_panel(context_type: &Kind) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type).unwrap_or_else(|| Box::new(FallbackPanel))
}

/// Minimal panel for context types whose module has been removed.
struct FallbackPanel;

impl Panel for FallbackPanel {
    fn blocks(&self, _state: &State) -> Vec<cp_render::Block> {
        Vec::new()
    }
    fn title(&self, _state: &State) -> String {
        "(removed)".to_string()
    }
    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
    }
    fn needs_cache(&self) -> bool {
        false
    }
    fn refresh(&self, _state: &mut State) {}
    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }
    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }
    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }
    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}

/// Refresh all panels (update token counts, etc.)
pub(crate) fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<Kind> = state.context.iter().map(|c| c.context_type.clone()).collect();

    for context_type in &context_types {
        let panel = get_panel(context_type);
        panel.refresh(state);
    }
}

/// Collect all context items from all panels
pub(crate) fn collect_all_context(state: &State) -> Vec<ContextItem> {
    let mut items = Vec::new();

    // Get UNIQUE context types from state (dedup to avoid multiplying items!)
    let mut seen = std::collections::HashSet::new();
    let context_types: Vec<Kind> =
        state.context.iter().map(|c| c.context_type.clone()).filter(|ct| seen.insert(ct.clone())).collect();

    for context_type in &context_types {
        let panel = get_panel(context_type);
        items.extend(panel.context(state));
    }

    items
}
