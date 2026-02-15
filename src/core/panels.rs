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

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crossterm::event::KeyEvent;

use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate};
use crate::state::{ContextElement, ContextType, State};
use crate::ui::{helpers::count_wrapped_lines, theme};

/// Get current time in milliseconds since UNIX epoch
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Update last_refresh_ms only if content actually changed (hash differs).
/// Returns true if content changed.
pub fn update_if_changed(ctx: &mut crate::state::ContextElement, content: &str) -> bool {
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
pub fn mark_panels_dirty(state: &mut crate::state::State, context_type: crate::state::ContextType) {
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

    /// Render the panel to the frame (default implementation)
    fn render(&self, frame: &mut Frame, state: &mut State, area: Rect) {
        let base_style = Style::default().bg(theme::bg_surface());
        let title = self.title(state);

        let inner_area = Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), area.height);

        // Build bottom title for dynamic panels: "refreshed Xs ago"
        let bottom_title =
            state.context.get(state.selected_context).filter(|ctx| !ctx.context_type.is_fixed()).and_then(|ctx| {
                let ts = ctx.last_refresh_ms;
                if ts < 1577836800000 {
                    return None;
                } // invalid timestamp
                let now = now_ms();
                if now <= ts {
                    return None;
                }
                Some(format!(" {} ", crate::ui::helpers::format_time_ago(now - ts)))
            });

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style)
            .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::accent()).bold()));

        if let Some(ref bottom) = bottom_title {
            block = block.title_bottom(Span::styled(bottom, Style::default().fg(theme::text_muted())));
        }

        let content_area = block.inner(inner_area);
        frame.render_widget(block, inner_area);

        let text = self.content(state, base_style);

        // Calculate and set max scroll (accounting for wrapped lines)
        let viewport_width = content_area.width as usize;
        let viewport_height = content_area.height as usize;
        let content_height: usize = {
            let _guard = crate::profile!("panel::scroll_calc");
            text.iter().map(|line| count_wrapped_lines(line, viewport_width)).sum()
        };
        let max_scroll = content_height.saturating_sub(viewport_height) as f32;
        state.max_scroll = max_scroll;
        state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

        let paragraph = {
            let _guard = crate::profile!("panel::paragraph_new");
            Paragraph::new(text)
                .style(base_style)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll_offset.round() as u16, 0))
        };

        {
            let _guard = crate::profile!("panel::frame_render");
            frame.render_widget(paragraph, content_area);
        }
    }
}

/// Get the appropriate panel for a context type (delegates to module system)
pub fn get_panel(context_type: ContextType) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type)
        .unwrap_or_else(|| panic!("No module provides a panel for {:?}", context_type))
}

/// Refresh all panels (update token counts, etc.)
pub fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<ContextType> = state.context.iter().map(|c| c.context_type).collect();

    for context_type in context_types {
        let panel = get_panel(context_type);
        panel.refresh(state);
    }
}

/// Collect all context items from all panels
pub fn collect_all_context(state: &State) -> Vec<ContextItem> {
    let mut items = Vec::new();

    // Get UNIQUE context types from state (dedup to avoid multiplying items!)
    let mut seen = std::collections::HashSet::new();
    let context_types: Vec<ContextType> =
        state.context.iter().map(|c| c.context_type).filter(|ct| seen.insert(*ct)).collect();

    for context_type in context_types {
        let panel = get_panel(context_type);
        items.extend(panel.context(state));
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheUpdate;
    use crate::state::ContextType;

    /// Helper: create a minimal ContextElement for testing
    fn test_ctx(id: &str, ct: ContextType) -> ContextElement {
        crate::modules::make_default_context_element(id, ct, "test", true)
    }

    // ── update_if_changed ──────────────────────────────────────────

    #[test]
    fn update_if_changed_first_call_returns_true() {
        let mut ctx = test_ctx("P0", ContextType::File);
        ctx.content_hash = None;
        assert!(update_if_changed(&mut ctx, "hello"));
        assert!(ctx.content_hash.is_some());
        assert!(ctx.last_refresh_ms > 0);
    }

    #[test]
    fn update_if_changed_same_content_returns_false() {
        let mut ctx = test_ctx("P0", ContextType::File);
        update_if_changed(&mut ctx, "hello");
        let ts = ctx.last_refresh_ms;
        assert!(!update_if_changed(&mut ctx, "hello"));
        assert_eq!(ctx.last_refresh_ms, ts); // Timestamp unchanged
    }

    #[test]
    fn update_if_changed_different_content_returns_true() {
        let mut ctx = test_ctx("P0", ContextType::File);
        update_if_changed(&mut ctx, "hello");
        assert!(update_if_changed(&mut ctx, "world"));
    }

    // ── mark_panels_dirty ──────────────────────────────────────────

    #[test]
    fn mark_panels_dirty_targets_correct_type() {
        let mut state = State::default();
        // Clear all dirty flags first
        for ctx in &mut state.context {
            ctx.cache_deprecated = false;
        }
        state.dirty = false;

        mark_panels_dirty(&mut state, ContextType::Git);

        // Only Git panels should be dirty
        for ctx in &state.context {
            if ctx.context_type == ContextType::Git {
                assert!(ctx.cache_deprecated, "Git panel should be dirty");
            } else {
                assert!(!ctx.cache_deprecated, "{:?} should not be dirty", ctx.context_type);
            }
        }
        assert!(state.dirty);
    }

    #[test]
    fn mark_panels_dirty_sets_state_dirty() {
        let mut state = State::default();
        state.dirty = false;
        mark_panels_dirty(&mut state, ContextType::File);
        assert!(state.dirty);
    }

    // ── apply_cache_update per panel type ──────────────────────────

    #[test]
    fn file_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::File);
        ctx.file_path = Some("/tmp/test.txt".to_string());
        ctx.cache_deprecated = true;
        ctx.cache_in_flight = true;

        let panel = get_panel(ContextType::File);
        let update = CacheUpdate::Content {
            context_id: "P99".to_string(),
            content: "file content here".to_string(),
            token_count: 5,
        };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated, "cache_deprecated should be cleared");
        assert!(ctx.content_hash.is_some(), "content_hash should be set");
        assert!(ctx.source_hash.is_some(), "source_hash should be set");
        assert!(ctx.last_refresh_ms > 0, "last_refresh_ms should be set");
        assert_eq!(ctx.cached_content.as_deref(), Some("file content here"));
    }

    #[test]
    fn tree_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P2", ContextType::Tree);
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::Tree);
        let update =
            CacheUpdate::Content { context_id: "P2".to_string(), content: "tree output".to_string(), token_count: 3 };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.content_hash.is_some());
    }

    #[test]
    fn glob_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::Glob);
        ctx.glob_pattern = Some("*.rs".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::Glob);
        let update =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "glob results".to_string(), token_count: 2 };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.content_hash.is_some());
    }

    #[test]
    fn grep_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::Grep);
        ctx.grep_pattern = Some("fn main".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::Grep);
        let update =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "grep results".to_string(), token_count: 2 };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.content_hash.is_some());
    }

    #[test]
    fn tmux_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::Tmux);
        ctx.tmux_pane_id = Some("%0".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::Tmux);
        let update = CacheUpdate::Content {
            context_id: "P99".to_string(),
            content: "$ ls\nfile1.txt\nfile2.txt".to_string(),
            token_count: 5,
        };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.source_hash.is_some());
        assert!(ctx.content_hash.is_some());
    }

    #[test]
    fn git_status_apply_cache_update() {
        let mut state = State::default();
        let idx = state.context.iter().position(|c| c.context_type == ContextType::Git);
        if idx.is_none() {
            return;
        } // Git module not active
        let idx = idx.unwrap();
        state.context[idx].cache_deprecated = true;

        let panel = get_panel(ContextType::Git);
        let update = CacheUpdate::GitStatus {
            branch: Some("main".to_string()),
            is_repo: true,
            file_changes: vec![],
            branches: vec![("main".to_string(), true)],
            formatted_content: "## Git Status\nOn branch main".to_string(),
            token_count: 10,
            source_hash: "abc123".to_string(),
        };

        let mut ctx = state.context.remove(idx);
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);
        state.context.insert(idx, ctx);

        assert!(changed);
        assert!(!state.context[idx].cache_deprecated);
        assert_eq!(state.git_branch.as_deref(), Some("main"));
        assert!(state.git_is_repo);
        assert!(state.context[idx].source_hash.is_some());
        assert!(state.context[idx].content_hash.is_some());
    }

    #[test]
    fn git_status_unchanged_clears_deprecated() {
        let mut state = State::default();
        let idx = state.context.iter().position(|c| c.context_type == ContextType::Git);
        if idx.is_none() {
            return;
        }
        let idx = idx.unwrap();
        state.context[idx].cache_deprecated = true;

        let panel = get_panel(ContextType::Git);
        let mut ctx = state.context.remove(idx);
        let changed = panel.apply_cache_update(CacheUpdate::GitStatusUnchanged, &mut ctx, &mut state);
        state.context.insert(idx, ctx);

        assert!(!changed, "Unchanged should return false");
        assert!(!state.context[idx].cache_deprecated, "cache_deprecated should be cleared");
    }

    #[test]
    fn git_result_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::GitResult);
        ctx.result_command = Some("git log --oneline -5".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::GitResult);
        let update = CacheUpdate::Content {
            context_id: "P99".to_string(),
            content: "abc1234 Initial commit".to_string(),
            token_count: 3,
        };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.content_hash.is_some());
    }

    #[test]
    fn github_result_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::GithubResult);
        ctx.result_command = Some("gh pr list".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(ContextType::GithubResult);
        let update =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "#1 Fix bug".to_string(), token_count: 2 };
        let changed = panel.apply_cache_update(update, &mut ctx, &mut state);

        assert!(changed);
        assert!(!ctx.cache_deprecated);
        assert!(ctx.content_hash.is_some());
    }

    // ── Duplicate content detection ────────────────────────────────

    #[test]
    fn apply_cache_update_same_content_twice() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::File);
        ctx.file_path = Some("/tmp/test.txt".to_string());

        let panel = get_panel(ContextType::File);

        // First update
        let update1 =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "same content".to_string(), token_count: 2 };
        let changed1 = panel.apply_cache_update(update1, &mut ctx, &mut state);
        assert!(changed1);
        let ts1 = ctx.last_refresh_ms;

        // Second update with same content — should NOT bump last_refresh_ms
        let update2 =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "same content".to_string(), token_count: 2 };
        // apply_cache_update returns true (it replaced content) but update_if_changed
        // doesn't bump timestamp since hash matches
        let _changed2 = panel.apply_cache_update(update2, &mut ctx, &mut state);
        assert_eq!(ctx.last_refresh_ms, ts1, "Timestamp should not change for same content");
    }

    // ── Timer interval tests ───────────────────────────────────────

    #[test]
    fn tmux_has_timer_interval() {
        let panel = get_panel(ContextType::Tmux);
        assert!(panel.cache_refresh_interval_ms().is_some(), "Tmux should have timer interval");
    }

    #[test]
    fn git_has_timer_interval() {
        let panel = get_panel(ContextType::Git);
        assert!(panel.cache_refresh_interval_ms().is_some(), "Git should have timer interval");
    }

    #[test]
    fn file_has_no_timer_interval() {
        let panel = get_panel(ContextType::File);
        assert!(panel.cache_refresh_interval_ms().is_none(), "File should use watcher, not timer");
    }

    #[test]
    fn tree_has_no_timer_interval() {
        let panel = get_panel(ContextType::Tree);
        assert!(panel.cache_refresh_interval_ms().is_none(), "Tree should use watcher, not timer");
    }

    #[test]
    fn glob_has_no_timer_interval() {
        let panel = get_panel(ContextType::Glob);
        assert!(panel.cache_refresh_interval_ms().is_none(), "Glob should use watcher, not timer");
    }

    #[test]
    fn grep_has_no_timer_interval() {
        let panel = get_panel(ContextType::Grep);
        assert!(panel.cache_refresh_interval_ms().is_none(), "Grep should use watcher, not timer");
    }

    // ── needs_cache tests ──────────────────────────────────────────

    #[test]
    fn cache_types_are_correct() {
        // Panels that need background caching
        assert!(ContextType::File.needs_cache());
        assert!(ContextType::Tree.needs_cache());
        assert!(ContextType::Glob.needs_cache());
        assert!(ContextType::Grep.needs_cache());
        assert!(ContextType::Tmux.needs_cache());
        assert!(ContextType::Git.needs_cache());
        assert!(ContextType::GitResult.needs_cache());
        assert!(ContextType::GithubResult.needs_cache());

        // Panels that derive content from state (no background caching)
        assert!(!ContextType::System.needs_cache());
        assert!(!ContextType::Conversation.needs_cache());
        assert!(!ContextType::Todo.needs_cache());
        assert!(!ContextType::Memory.needs_cache());
        assert!(!ContextType::Scratchpad.needs_cache());
        assert!(!ContextType::Library.needs_cache());
    }
}
