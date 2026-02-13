use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::cache::{CacheRequest, CacheUpdate};
use crate::core::panels::{paginate_content, ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use super::GLOB_DEPRECATION_MS;
use crate::state::{compute_total_pages, estimate_tokens, ContextElement, ContextType, State};
use crate::ui::{theme, chars};

pub struct GlobPanel;

impl Panel for GlobPanel {
    fn needs_cache(&self) -> bool { true }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, state: &State) -> String {
        if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content to count files
            let count = ctx.cached_content.as_ref()
                .map(|c| c.lines().count())
                .unwrap_or(0);
            format!("{} ({} files)", ctx.name, count)
        } else {
            "Glob".to_string()
        }
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let pattern = ctx.glob_pattern.as_ref()?;
        Some(CacheRequest::RefreshGlob {
            context_id: ctx.id.clone(),
            pattern: pattern.clone(),
            base_path: ctx.glob_path.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::GlobContent { content, token_count, .. } = update else {
            return false;
        };
        ctx.cache_deprecated = false;
        // Check if content actually changed before updating
        let new_hash = crate::cache::hash_content(&content);
        if ctx.content_hash.as_deref() == Some(&new_hash) {
            return false;
        }
        ctx.cached_content = Some(content);
        ctx.token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        ctx.content_hash = Some(new_hash);
        true
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(GLOB_DEPRECATION_MS)
    }

    fn refresh(&self, _state: &mut State) {
        // Glob refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshGlob { context_id, pattern, base_path } = request else {
            return None;
        };
        let base = base_path.as_deref().unwrap_or(".");
        let (content, _count) = super::tools::compute_glob_results(&pattern, base);
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::GlobContent {
            context_id,
            content: content.to_string(),
            token_count,
        })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::Glob)
            .filter_map(|c| {
                let pattern = c.glob_pattern.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, format!("Glob: {}", pattern), output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let content = if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content only - no blocking operations
            ctx.cached_content.as_ref()
                .cloned()
                .unwrap_or_else(|| {
                    if ctx.cache_deprecated {
                        "Loading...".to_string()
                    } else {
                        "No results".to_string()
                    }
                })
        } else {
            String::new()
        };

        content.lines()
            .map(|line| Line::from(vec![
                Span::styled(format!("  {} ", chars::DOT), Style::default().fg(theme::accent_dim())),
                Span::styled(line.to_string(), Style::default().fg(theme::text())),
            ]))
            .collect()
    }
}
