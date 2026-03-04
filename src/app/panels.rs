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
use cp_base::cast::SafeCast;

// Re-export the Panel trait, ContextItem, and utility functions from cp-base
#[cfg(test)]
pub(crate) use cp_base::panels::mark_panels_dirty;
pub(crate) use cp_base::panels::{ContextItem, Panel, now_ms, paginate_content, update_if_changed};

/// Render a panel with the binary's full chrome (borders, theme, scroll, profiling).
/// This is NOT part of the Panel trait — it uses binary-specific deps (theme, profile!, UI helpers).
pub(crate) fn render_panel_default(panel: &dyn Panel, frame: &mut Frame<'_>, state: &mut State, area: Rect) {
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
    let viewport_width = content_area.width.to_usize();
    let viewport_height = content_area.height.to_usize();
    let content_height: usize = {
        let _guard = crate::profile!("panel::scroll_calc");
        text.iter().map(|line| count_wrapped_lines(line, viewport_width)).sum()
    };
    let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = {
        let _guard = crate::profile!("panel::paragraph_new");
        Paragraph::new(text)
            .style(base_style)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll_offset.round().to_u16(), 0))
    };

    {
        let _guard = crate::profile!("panel::frame_render");
        frame.render_widget(paragraph, content_area);
    }
}

/// Get the appropriate panel for a context type (delegates to module system).
/// Returns a no-op fallback for orphaned context types (e.g., removed modules).
pub(crate) fn get_panel(context_type: &ContextType) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type).unwrap_or_else(|| Box::new(FallbackPanel))
}

/// Minimal panel for context types whose module has been removed.
struct FallbackPanel;

impl Panel for FallbackPanel {
    fn title(&self, _state: &State) -> String {
        "(removed)".to_string()
    }
    fn content(&self, _state: &State, _base_style: Style) -> Vec<Line<'static>> {
        vec![Line::from("Panel module no longer available")]
    }
}

/// Refresh all panels (update token counts, etc.)
pub(crate) fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<ContextType> = state.context.iter().map(|c| c.context_type.clone()).collect();

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
    use crate::state::ContextElement;
    use crate::state::cache::CacheUpdate;

    /// Helper: create a minimal `ContextElement` for testing
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
        let _ = update_if_changed(&mut ctx, "hello");
        let ts = ctx.last_refresh_ms;
        assert!(!update_if_changed(&mut ctx, "hello"));
        assert_eq!(ctx.last_refresh_ms, ts); // Timestamp unchanged
    }

    #[test]
    fn update_if_changed_different_content_returns_true() {
        let mut ctx = test_ctx("P0", ContextType::new(ContextType::FILE));
        let _ = update_if_changed(&mut ctx, "hello");
        assert!(update_if_changed(&mut ctx, "world"));
    }

    // ── mark_panels_dirty ──────────────────────────────────────────

    #[test]
    fn mark_panels_dirty_targets_correct_type() {
        let mut state = State { dirty: false, ..State::default() };
        // Clear all dirty flags first
        for ctx in &mut state.context {
            ctx.cache_deprecated = false;
        }

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
        let mut state = State { dirty: false, ..State::default() };
        mark_panels_dirty(&mut state, ContextType::new(ContextType::FILE));
        assert!(state.dirty);
    }

    // ── apply_cache_update per panel type ──────────────────────────

    #[test]
    fn file_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::FILE));
        ctx.set_meta("file_path", &"/tmp/test.txt".to_string());
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
    fn git_result_panel_apply_cache_update() {
        let mut state = State::default();
        let mut ctx = test_ctx("P99", ContextType::new(ContextType::GIT_RESULT));
        ctx.set_meta("result_command", &"git log --oneline -5".to_string());
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
        ctx.set_meta("result_command", &"gh pr list".to_string());
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
        ctx.set_meta("file_path", &"/tmp/test.txt".to_string());

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
    fn git_result_has_timer_interval() {
        let panel = get_panel(&ContextType::new(ContextType::GIT_RESULT));
        assert!(panel.cache_refresh_interval_ms().is_some(), "Git result should have timer interval");
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

    // ── needs_cache tests ──────────────────────────────────────────

    #[test]
    fn cache_types_are_correct() {
        crate::modules::init_registry();
        // Panels that need background caching
        assert!(ContextType::new(ContextType::FILE).needs_cache());
        assert!(ContextType::new(ContextType::TREE).needs_cache());
        // glob/grep modules removed — no longer in registry
        // tmux module removed — no longer in registry
        // git fixed panel removed — only git_result remains
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
