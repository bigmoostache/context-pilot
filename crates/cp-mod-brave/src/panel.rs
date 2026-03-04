use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use cp_base::config::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use cp_base::config::theme;
use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::Action;
use cp_base::state::{ContextElement, ContextType, State, compute_total_pages, estimate_tokens};

pub(crate) const BRAVE_PANEL_TYPE: &str = "brave_result";

const META_CONTENT: &str = "result_content";

/// Create a dynamic panel with the given title and content.
///
/// Returns the panel ID string (e.g., "P15").
pub fn create_panel(state: &mut State, title: &str, content: &str) -> String {
    let panel_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    let mut elem =
        cp_base::state::make_default_context_element(&panel_id, ContextType::new(BRAVE_PANEL_TYPE), title, false);
    elem.uid = Some(uid);
    elem.cached_content = Some(content.to_string());
    elem.token_count = estimate_tokens(content);
    elem.full_token_count = elem.token_count;
    elem.total_pages = compute_total_pages(elem.token_count);
    // Store content in metadata so it persists across reloads
    drop(elem.metadata.insert(META_CONTENT.to_string(), serde_json::Value::String(content.to_string())));

    state.context.push(elem);
    panel_id
}

/// Panel renderer for Brave search result panels.
#[derive(Debug, Clone, Copy)]
pub struct BraveResultPanel;

/// Cache request for restoring content from metadata after reload
struct BraveRestoreRequest {
    context_id: String,
    content: String,
}

impl Panel for BraveResultPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        // Only need to restore if cached_content is missing (post-reload)
        if ctx.cached_content.is_some() {
            return None;
        }
        let content = ctx.metadata.get(META_CONTENT)?.as_str()?;
        Some(CacheRequest {
            context_type: ContextType::new(BRAVE_PANEL_TYPE),
            data: Box::new(BraveRestoreRequest { context_id: ctx.id.clone(), content: content.to_string() }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        if let CacheUpdate::Content { content, token_count, .. } = update {
            ctx.cached_content = Some(content.clone());
            ctx.full_token_count = token_count;
            ctx.total_pages = compute_total_pages(token_count);
            ctx.current_page = 0;
            if ctx.total_pages > 1 {
                let page_content =
                    paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
                ctx.token_count = estimate_tokens(&page_content);
            } else {
                ctx.token_count = token_count;
            }
            ctx.cache_deprecated = false;
            let _ = update_if_changed(ctx, &content);
            true
        } else {
            false
        }
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let req = request.data.downcast::<BraveRestoreRequest>().ok()?;
        let token_count = estimate_tokens(&req.content);
        Some(CacheUpdate::Content { context_id: req.context_id.clone(), content: req.content.clone(), token_count })
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
        state
            .context
            .get(state.selected_context)
            .map(|ctx| ctx.name.clone())
            .unwrap_or_else(|| "Brave Result".to_string())
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == ContextType::new(BRAVE_PANEL_TYPE))
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let ctx =
            state.context.get(state.selected_context).filter(|c| c.context_type == ContextType::new(BRAVE_PANEL_TYPE));

        let Some(ctx) = ctx else {
            return vec![Line::from(vec![Span::styled(
                " No brave result panel",
                Style::default().fg(theme::text_muted()),
            )])];
        };

        let Some(content) = &ctx.cached_content else {
            return vec![Line::from(vec![Span::styled(
                " Loading...",
                Style::default().fg(theme::text_muted()).italic(),
            )])];
        };

        content
            .lines()
            .map(|line| {
                Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(line.to_string(), Style::default().fg(theme::text())),
                ])
            })
            .collect()
    }
}
