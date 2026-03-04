use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use cp_base::cast::SafeCast;
use cp_base::config::constants::{PANEL_MAX_LOAD_BYTES, SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use cp_base::config::theme;
use cp_base::panels::{CacheRequest, CacheUpdate, hash_content};
use cp_base::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::Action;
use cp_base::state::{ContextElement, ContextType, State, compute_total_pages, estimate_tokens};

pub(crate) struct FileCacheRequest {
    pub context_id: String,
    pub file_path: String,
    pub current_source_hash: Option<String>,
}

pub(crate) struct FilePanel;

impl Panel for FilePanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn suicide(&self, ctx: &ContextElement, _state: &State) -> bool {
        // Only check when still loading — don't kill panels with content
        if ctx.cached_content.is_some() {
            return false;
        }
        // If the file has been deleted from disk, close the panel
        if let Some(path) = ctx.get_meta_str("file_path") {
            return !PathBuf::from(path).exists();
        }
        false
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
        state.context.get(state.selected_context).map(|ctx| ctx.name.clone()).unwrap_or_else(|| "File".to_string())
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let path = ctx.get_meta_str("file_path")?;
        Some(CacheRequest {
            context_type: ContextType::new(ContextType::FILE),
            data: Box::new(FileCacheRequest {
                context_id: ctx.id.clone(),
                file_path: path.to_string(),
                current_source_hash: ctx.source_hash.clone(),
            }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
            return false;
        };
        ctx.source_hash = Some(hash_content(&content));
        ctx.cached_content = Some(content);
        ctx.full_token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        // token_count reflects current page, not full content
        if ctx.total_pages > 1 {
            let page_content =
                paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
            ctx.token_count = estimate_tokens(&page_content);
        } else {
            ctx.token_count = token_count;
        }
        ctx.cache_deprecated = false;
        let content_ref = ctx.cached_content.clone().unwrap_or_default();
        let _ = update_if_changed(ctx, &content_ref);
        true
    }

    fn refresh(&self, _state: &mut State) {
        // File refresh is handled by background cache system via refresh_cache
    }
    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let req = request.data.downcast::<FileCacheRequest>().ok()?;
        let FileCacheRequest { context_id, file_path, current_source_hash } = *req;
        let path = PathBuf::from(&file_path);
        if !path.exists() {
            return None;
        }
        // Hard byte limit: refuse to load oversized files
        if let Ok(meta) = fs::metadata(&path)
            && meta.len().to_usize() > PANEL_MAX_LOAD_BYTES
        {
            let msg = format!(
                "[File too large to load: {} bytes (limit: {} bytes). Close this panel and use grep or other tools to inspect portions of the file.]",
                meta.len(),
                PANEL_MAX_LOAD_BYTES
            );
            let token_count = estimate_tokens(&msg);
            return Some(CacheUpdate::Content { context_id, content: msg, token_count });
        }
        let content = fs::read_to_string(&path).ok()?;
        let new_hash = hash_content(&content);
        if current_source_hash.as_ref() == Some(&new_hash) {
            return Some(CacheUpdate::Unchanged { context_id });
        }
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::Content { context_id, content, token_count })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == ContextType::FILE)
            .filter_map(|c| {
                let path = c.get_meta_str("file_path")?;
                // Use cached content only - no blocking file reads
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, format!("File: {}", path), output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let selected = state.context.get(state.selected_context);

        let (content, file_path) = if let Some(ctx) = selected {
            let path = ctx.get_meta_str("file_path").unwrap_or("");
            // Use cached content only - no blocking file reads
            let content = ctx.cached_content.clone().unwrap_or_else(|| {
                if ctx.cache_deprecated { "Loading...".to_string() } else { "No content".to_string() }
            });
            (content, path.to_string())
        } else {
            (String::new(), String::new())
        };

        // Get syntax highlighting
        let highlighted = if !file_path.is_empty() {
            state.highlight_fn.map(|f| f(&file_path, &content)).unwrap_or_else(|| std::sync::Arc::new(Vec::new()))
        } else {
            std::sync::Arc::new(Vec::new())
        };

        let mut text: Vec<Line<'_>> = Vec::new();

        if highlighted.is_empty() {
            for (i, line) in content.lines().enumerate() {
                let line_num = i + 1;
                text.push(Line::from(vec![
                    Span::styled(
                        format!(" {:4} ", line_num),
                        Style::default().fg(theme::text_muted()).bg(theme::bg_base()),
                    ),
                    Span::styled(" ", base_style),
                    Span::styled(line.to_string(), Style::default().fg(theme::text())),
                ]));
            }
        } else {
            for (i, spans) in highlighted.iter().enumerate() {
                let line_num = i + 1;
                let mut line_spans = vec![
                    Span::styled(
                        format!(" {:4} ", line_num),
                        Style::default().fg(theme::text_muted()).bg(theme::bg_base()),
                    ),
                    Span::styled(" ", base_style),
                ];

                for (color, text) in spans {
                    line_spans.push(Span::styled(text.clone(), Style::default().fg(*color)));
                }

                text.push(Line::from(line_spans));
            }
        }

        text
    }
}
