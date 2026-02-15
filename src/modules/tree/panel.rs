use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate};
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::core::panels::{ContextItem, Panel, paginate_content};
use crate::state::{ContextElement, ContextType, State, compute_total_pages, estimate_tokens};
use crate::ui::{helpers::*, theme};

pub struct TreePanel;

impl Panel for TreePanel {
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

    fn title(&self, _state: &State) -> String {
        "Directory Tree".to_string()
    }

    fn build_cache_request(&self, ctx: &ContextElement, state: &State) -> Option<CacheRequest> {
        Some(CacheRequest::RefreshTree {
            context_id: ctx.id.clone(),
            tree_filter: state.tree_filter.clone(),
            tree_open_folders: state.tree_open_folders.clone(),
            tree_descriptions: state.tree_descriptions.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
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

    fn refresh(&self, _state: &mut State) {
        // Tree refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshTree { context_id, tree_filter, tree_open_folders, tree_descriptions } = request
        else {
            return None;
        };
        let content = super::tools::generate_tree_string(&tree_filter, &tree_open_folders, &tree_descriptions);
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::Content { context_id, content, token_count })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Find tree context and use cached content
        for ctx in &state.context {
            if ctx.context_type == ContextType::Tree {
                if let Some(content) = &ctx.cached_content
                    && !content.is_empty()
                {
                    let output = paginate_content(content, ctx.current_page, ctx.total_pages);
                    return vec![ContextItem::new(&ctx.id, "Directory Tree", output, ctx.last_refresh_ms)];
                }
                break;
            }
        }
        Vec::new()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let _guard = crate::profile!("panel::tree::content");
        // Find tree context and use cached content
        let tree_content = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::Tree)
            .and_then(|ctx| ctx.cached_content.as_ref())
            .cloned()
            .unwrap_or_else(|| "Loading...".to_string());

        let mut text: Vec<Line> = Vec::new();
        for line in tree_content.lines() {
            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::styled(" ".to_string(), Style::default().fg(theme::text())));

            // Check for description (after " - ")
            let (main_line, description) = if let Some(desc_idx) = line.find(" - ") {
                (&line[..desc_idx], Some(&line[desc_idx..]))
            } else {
                (line, None)
            };

            // Parse the main part
            if let Some(size_start) = find_size_pattern(main_line) {
                let (before_size, size_part) = main_line.split_at(size_start);
                spans.push(Span::styled(before_size.to_string(), Style::default().fg(theme::text())));
                spans.push(Span::styled(size_part.to_string(), Style::default().fg(theme::accent_dim())));
            } else if let Some((start, end)) = find_children_pattern(main_line) {
                let before = &main_line[..start];
                let children_part = &main_line[start..end];
                let after = &main_line[end..];
                spans.push(Span::styled(before.to_string(), Style::default().fg(theme::text())));
                spans.push(Span::styled(children_part.to_string(), Style::default().fg(theme::accent())));
                if !after.is_empty() {
                    spans.push(Span::styled(after.to_string(), Style::default().fg(theme::text())));
                }
            } else {
                spans.push(Span::styled(main_line.to_string(), Style::default().fg(theme::text())));
            }

            if let Some(desc) = description {
                spans.push(Span::styled(desc.to_string(), Style::default().fg(theme::text_muted())));
            }

            text.push(Line::from(spans));
        }

        text
    }
}
