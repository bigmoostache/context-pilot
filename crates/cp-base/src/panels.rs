//! Panel trait and implementations for different context types.
//!
//! Each panel type implements the Panel trait, providing a consistent
//! interface for rendering AND context generation for the LLM.
//!
//! ## Caching Architecture
//!
//! Panels use a two-level caching system:
//! - `cache_deprecated`: Source data changed, cache needs regeneration
//! - `cached_content`: The actual cached content string
//!
//! When `refresh()` is called:
//! 1. Check if cache is deprecated (or missing)
//! 2. If so, regenerate cache from source data
//! 3. Update token count from cached content
//!
//! `context()` returns the cached content without regenerating.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::prelude::*;

use crossterm::event::KeyEvent;

use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate};
use crate::state::{ContextElement, ContextType, State};

/// Get current time in milliseconds since UNIX epoch
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Update last_refresh_ms only if content actually changed (hash differs).
/// Returns true if content changed.
pub fn update_if_changed(ctx: &mut ContextElement, content: &str) -> bool {
    let new_hash = crate::cache::hash_content(content);
    if ctx.content_hash.as_deref() == Some(&new_hash) {
        return false;
    }
    ctx.content_hash = Some(new_hash);
    ctx.last_refresh_ms = now_ms();
    true
}

/// Mark all panels of a given context type as cache-deprecated (dirty).
/// Also sets `state.dirty = true` so the UI re-renders.
pub fn mark_panels_dirty(state: &mut State, context_type: ContextType) {
    for ctx in &mut state.context {
        if ctx.context_type == context_type {
            ctx.cache_deprecated = true;
        }
    }
    state.dirty = true;
}

/// Paginate content for LLM context output.
/// Returns the original content unchanged when total_pages <= 1.
/// Otherwise slices by approximate token offset, snaps to line boundaries,
/// and prepends a page header.
pub fn paginate_content(full_content: &str, current_page: usize, total_pages: usize) -> String {
    use crate::constants::{CHARS_PER_TOKEN, PANEL_PAGE_TOKENS};

    if total_pages <= 1 {
        return full_content.to_string();
    }

    let chars_per_page = PANEL_PAGE_TOKENS as f32 * CHARS_PER_TOKEN;
    let start_char = (current_page as f32 * chars_per_page) as usize;

    // Snap start to next line boundary
    let start = if start_char == 0 {
        0
    } else if start_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after start_char
        full_content[start_char..].find('\n').map(|pos| start_char + pos + 1).unwrap_or(full_content.len())
    };

    let end_char = start + chars_per_page as usize;
    let end = if end_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after end_char to snap to line boundary
        full_content[end_char..].find('\n').map(|pos| end_char + pos + 1).unwrap_or(full_content.len())
    };

    let page_content = &full_content[start..end];
    format!("[Page {}/{} — use panel_goto_page to navigate]\n{}", current_page + 1, total_pages, page_content)
}

/// A single context item to be sent to the LLM
#[derive(Debug, Clone)]
pub struct ContextItem {
    /// Context element ID (e.g., "P7", "P8") for LLM reference
    pub id: String,
    /// Header/title for this context (e.g., "File: src/main.rs" or "Todo List")
    pub header: String,
    /// The actual content
    pub content: String,
    /// Last refresh timestamp in milliseconds since UNIX epoch (for sorting panels)
    pub last_refresh_ms: u64,
}

impl ContextItem {
    pub fn new(
        id: impl Into<String>,
        header: impl Into<String>,
        content: impl Into<String>,
        last_refresh_ms: u64,
    ) -> Self {
        Self { id: id.into(), header: header.into(), content: content.into(), last_refresh_ms }
    }
}

/// Trait for all panel types
pub trait Panel {
    /// Generate the panel's title for display
    fn title(&self, state: &State) -> String;

    /// Generate the panel's content lines for rendering (uses 'static since we create owned data)
    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>>;

    /// Handle keyboard input for this panel
    /// Returns None to use default handling, Some(action) to override
    fn handle_key(&self, _key: &KeyEvent, _state: &State) -> Option<Action> {
        None // Default: use global key handling
    }

    /// Whether this panel uses background caching (cached_content from background loading)
    fn needs_cache(&self) -> bool {
        false
    }

    /// Refresh token counts and any cached data (called before generating context)
    fn refresh(&self, _state: &mut State) {
        // Default: no refresh needed
    }

    /// Compute a cache update for this panel in the background.
    /// Called from a background thread — implementations should do blocking I/O here.
    /// Returns None if no update is needed (e.g., content unchanged).
    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    /// Build a cache request for the given context element.
    /// Returns None for panels without background caching.
    fn build_cache_request(&self, _ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        None
    }

    /// Apply a cache update to the context element and state.
    /// Returns true if content changed (caller sets state.dirty).
    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut ContextElement, _state: &mut State) -> bool {
        false
    }

    /// Timer interval in ms for auto-refresh. None = no timer (uses watchers or no refresh).
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    /// Generate context items to send to the LLM
    /// Returns empty vec if this panel doesn't contribute to LLM context
    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    /// Render the panel to the frame (default: no-op, override in binary)
    fn render(&self, _frame: &mut Frame, _state: &mut State, _area: Rect) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ContextType;

    /// Helper: create a minimal ContextElement for testing
    fn test_ctx(id: &str, ct: ContextType) -> ContextElement {
        ContextElement {
            id: id.to_string(),
            uid: None,
            context_type: ct,
            name: "test".to_string(),
            token_count: 0,
            file_path: None,
            glob_pattern: None,
            glob_path: None,
            grep_pattern: None,
            grep_path: None,
            grep_file_pattern: None,
            tmux_pane_id: None,
            tmux_lines: None,
            tmux_last_keys: None,
            tmux_description: None,
            result_command: None,
            skill_prompt_id: None,
            cached_content: None,
            history_messages: None,
            cache_deprecated: true,
            cache_in_flight: false,
            last_refresh_ms: now_ms(),
            content_hash: None,
            source_hash: None,
            current_page: 0,
            total_pages: 1,
            full_token_count: 0,
            panel_cache_hit: false,
            panel_total_cost: 0.0,
        }
    }

    // ── update_if_changed ──────────────────────────────────────────

    #[test]
    fn update_if_changed_first_call_returns_true() {
        let mut ctx = test_ctx("P0", ContextType::new(ContextType::FILE));
        ctx.content_hash = None;
        assert!(update_if_changed(&mut ctx, "hello"));
        assert!(ctx.content_hash.is_some());
        assert!(ctx.last_refresh_ms > 0);
    }

    #[test]
    fn update_if_changed_same_content_returns_false() {
        let mut ctx = test_ctx("P0", ContextType::new(ContextType::FILE));
        update_if_changed(&mut ctx, "hello");
        let ts = ctx.last_refresh_ms;
        assert!(!update_if_changed(&mut ctx, "hello"));
        assert_eq!(ctx.last_refresh_ms, ts); // Timestamp unchanged
    }

    #[test]
    fn update_if_changed_different_content_returns_true() {
        let mut ctx = test_ctx("P0", ContextType::new(ContextType::FILE));
        update_if_changed(&mut ctx, "hello");
        assert!(update_if_changed(&mut ctx, "world"));
    }
}
