use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate};
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::core::panels::{ContextItem, Panel, paginate_content};
use crate::state::{ContextElement, ContextType, State, compute_total_pages, estimate_tokens};
use crate::ui::{chars, theme};

pub struct GrepPanel;

impl Panel for GrepPanel {
    fn needs_cache(&self) -> bool {
        true
    }

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
            // Use cached content to count matches
            let count = ctx.cached_content.as_ref().map(|c| c.lines().count()).unwrap_or(0);
            format!("{} ({} matches)", ctx.name, count)
        } else {
            "Grep".to_string()
        }
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let pattern = ctx.grep_pattern.as_ref()?;
        Some(CacheRequest::RefreshGrep {
            context_id: ctx.id.clone(),
            pattern: pattern.clone(),
            path: ctx.grep_path.clone(),
            file_pattern: ctx.grep_file_pattern.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
            return false;
        };
        ctx.cache_deprecated = false;
        // Always update cached content (needed after reload when cached_content is None)
        ctx.cached_content = Some(content.clone());
        ctx.token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        // Only report "changed" when content hash differs
        let new_hash = crate::cache::hash_content(&content);
        if ctx.content_hash.as_deref() == Some(&new_hash) {
            return false;
        }
        ctx.content_hash = Some(new_hash);
        true
    }

    fn refresh(&self, _state: &mut State) {
        // Grep refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshGrep { context_id, pattern, path, file_pattern } = request else {
            return None;
        };
        let search_path = path.as_deref().unwrap_or(".");
        let (content, _count) = super::tools::compute_grep_results(&pattern, search_path, file_pattern.as_deref());
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::Content { context_id, content: content.to_string(), token_count })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == ContextType::Grep)
            .filter_map(|c| {
                let pattern = c.grep_pattern.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, format!("Grep: {}", pattern), output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let content = if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content only - no blocking operations
            ctx.cached_content.as_ref().cloned().unwrap_or_else(|| {
                if ctx.cache_deprecated { "Loading...".to_string() } else { "No results".to_string() }
            })
        } else {
            String::new()
        };

        content
            .lines()
            .map(|line| {
                let parts: Vec<&str> = line.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    Line::from(vec![
                        Span::styled("  ".to_string(), base_style),
                        Span::styled(parts[0].to_string(), Style::default().fg(theme::accent_dim())),
                        Span::styled(":".to_string(), Style::default().fg(theme::text_muted())),
                        Span::styled(parts[1].to_string(), Style::default().fg(theme::warning())),
                        Span::styled(":".to_string(), Style::default().fg(theme::text_muted())),
                        Span::styled(parts[2].to_string(), Style::default().fg(theme::text())),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(format!("  {} ", chars::DOT), Style::default().fg(theme::accent_dim())),
                        Span::styled(line.to_string(), Style::default().fg(theme::text())),
                    ])
                }
            })
            .collect()
    }
}
