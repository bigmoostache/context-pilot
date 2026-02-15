//! Panel trait and implementations for different context types.
//!
//! The `Panel` trait and core types live in `cp_base::panels`.
//! This module re-exports them and adds binary-specific functionality
//! (rendering with theme/profiling, panel registry).

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::{ContextType, State};
use crate::ui::{helpers::count_wrapped_lines, theme};

// Re-export the Panel trait, ContextItem, and utility functions from cp-base
pub use cp_base::panels::{ContextItem, Panel, mark_panels_dirty, now_ms, paginate_content, update_if_changed};

/// Render a panel with the binary's full chrome (borders, theme, scroll, profiling).
/// This is NOT part of the Panel trait — it uses binary-specific deps (theme, profile!, UI helpers).
pub fn render_panel_default(panel: &dyn Panel, frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::bg_surface());
    let title = panel.title(state);

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

    let text = panel.content(state, base_style);

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

/// Get the appropriate panel for a context type (delegates to module system)
pub fn get_panel(context_type: &ContextType) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type)
        .unwrap_or_else(|| panic!("No module provides a panel for {:?}", context_type))
}

/// Refresh all panels (update token counts, etc.)
pub fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<ContextType> = state.context.iter().map(|c| c.context_type.clone()).collect();

    for context_type in &context_types {
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
        state.context.iter().map(|c| c.context_type.clone()).filter(|ct| seen.insert(ct.clone())).collect();

    for context_type in &context_types {
        let panel = get_panel(context_type);
        items.extend(panel.context(state));
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheUpdate;
    use crate::state::ContextElement;

    /// Helper: create a minimal ContextElement for testing
    fn test_ctx(id: &str, ct: ContextType) -> ContextElement {
        crate::modules::make_default_context_element(id, ct, "test", true)
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

    // ── mark_panels_dirty ──────────────────────────────────────────

    #[test]
    fn mark_panels_dirty_targets_correct_type() {
        let mut state = State::default();
        // Clear all dirty flags first
        for ctx in &mut state.context {
            ctx.cache_deprecated = false;
        }
        state.dirty = false;

        mark_panels_dirty(&mut state, ContextType::new(ContextType::GIT));

        // Only Git panels should be dirty
        for ctx in &state.context {
            if ctx.context_type == ContextType::GIT {
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
        mark_panels_dirty(&mut state, ContextType::new(ContextType::FILE));
        assert!(state.dirty);
    }

    // ── apply_cache_update per panel type ──────────────────────────

    #[test]
    fn file_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::FILE));
        ctx.file_path = Some("/tmp/test.txt".to_string());
        ctx.cache_deprecated = true;
        ctx.cache_in_flight = true;

        let panel = get_panel(&ContextType::new(ContextType::FILE));
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
        let mut ctx = test_ctx("P2", ContextType::new(ContextType::TREE));
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::TREE));
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
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::GLOB));
        ctx.glob_pattern = Some("*.rs".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GLOB));
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
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::GREP));
        ctx.grep_pattern = Some("fn main".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GREP));
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
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::TMUX));
        ctx.tmux_pane_id = Some("%0".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::TMUX));
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
        let idx = state.context.iter().position(|c| c.context_type == ContextType::GIT);
        if idx.is_none() {
            return;
        } // Git module not active
        let idx = idx.unwrap();
        state.context[idx].cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GIT));
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
        let idx = state.context.iter().position(|c| c.context_type == ContextType::GIT);
        if idx.is_none() {
            return;
        }
        let idx = idx.unwrap();
        state.context[idx].cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GIT));
        let mut ctx = state.context.remove(idx);
        let changed = panel.apply_cache_update(CacheUpdate::GitStatusUnchanged, &mut ctx, &mut state);
        state.context.insert(idx, ctx);

        assert!(!changed, "Unchanged should return false");
        assert!(!state.context[idx].cache_deprecated, "cache_deprecated should be cleared");
    }

    #[test]
    fn git_result_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::GIT_RESULT));
        ctx.result_command = Some("git log --oneline -5".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GIT_RESULT));
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
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::GITHUB_RESULT));
        ctx.result_command = Some("gh pr list".to_string());
        ctx.cache_deprecated = true;

        let panel = get_panel(&ContextType::new(ContextType::GITHUB_RESULT));
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
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::FILE));
        ctx.file_path = Some("/tmp/test.txt".to_string());

        let panel = get_panel(&ContextType::new(ContextType::FILE));

        // First update
        let update1 =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "same content".to_string(), token_count: 2 };
        let changed1 = panel.apply_cache_update(update1, &mut ctx, &mut state);
        assert!(changed1);
        let ts1 = ctx.last_refresh_ms;

        // Second update with same content — should NOT bump last_refresh_ms
        let update2 =
            CacheUpdate::Content { context_id: "P99".to_string(), content: "same content".to_string(), token_count: 2 };
        let _changed2 = panel.apply_cache_update(update2, &mut ctx, &mut state);
        assert_eq!(ctx.last_refresh_ms, ts1, "Timestamp should not change for same content");
    }

    // ── Timer interval tests ───────────────────────────────────────

    #[test]
    fn tmux_has_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::TMUX));
        assert!(panel.cache_refresh_interval_ms().is_some(), "Tmux should have timer interval");
    }

    #[test]
    fn git_has_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::GIT));
        assert!(panel.cache_refresh_interval_ms().is_some(), "Git should have timer interval");
    }

    #[test]
    fn file_has_no_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::FILE));
        assert!(panel.cache_refresh_interval_ms().is_none(), "File should use watcher, not timer");
    }

    #[test]
    fn tree_has_no_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::TREE));
        assert!(panel.cache_refresh_interval_ms().is_none(), "Tree should use watcher, not timer");
    }

    #[test]
    fn glob_has_no_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::GLOB));
        assert!(panel.cache_refresh_interval_ms().is_none(), "Glob should use watcher, not timer");
    }

    #[test]
    fn grep_has_no_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::GREP));
        assert!(panel.cache_refresh_interval_ms().is_none(), "Grep should use watcher, not timer");
    }

    // ── needs_cache tests ──────────────────────────────────────────

    #[test]
    fn cache_types_are_correct() {
        // Panels that need background caching
        assert!(ContextType::new(ContextType::FILE).needs_cache());
        assert!(ContextType::new(ContextType::TREE).needs_cache());
        assert!(ContextType::new(ContextType::GLOB).needs_cache());
        assert!(ContextType::new(ContextType::GREP).needs_cache());
        assert!(ContextType::new(ContextType::TMUX).needs_cache());
        assert!(ContextType::new(ContextType::GIT).needs_cache());
        assert!(ContextType::new(ContextType::GIT_RESULT).needs_cache());
        assert!(ContextType::new(ContextType::GITHUB_RESULT).needs_cache());

        // Panels that derive content from state (no background caching)
        assert!(!ContextType::new(ContextType::SYSTEM).needs_cache());
        assert!(!ContextType::new(ContextType::CONVERSATION).needs_cache());
        assert!(!ContextType::new(ContextType::TODO).needs_cache());
        assert!(!ContextType::new(ContextType::MEMORY).needs_cache());
        assert!(!ContextType::new(ContextType::SCRATCHPAD).needs_cache());
        assert!(!ContextType::new(ContextType::LIBRARY).needs_cache());
    }
}
