use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::cache::{hash_content, CacheRequest, CacheUpdate};
use crate::core::panels::{now_ms, paginate_content, ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{PANEL_MAX_LOAD_BYTES, SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::highlight::highlight_file;
use crate::state::{compute_total_pages, estimate_tokens, ContextElement, ContextType, State};
use crate::ui::theme;

pub struct FilePanel;

impl Panel for FilePanel {
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
        state.context.get(state.selected_context)
            .map(|ctx| ctx.name.clone())
            .unwrap_or_else(|| "File".to_string())
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let path = ctx.file_path.as_ref()?;
        Some(CacheRequest::RefreshFile {
            context_id: ctx.id.clone(),
            file_path: path.clone(),
            current_hash: ctx.file_hash.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::FileContent { content, hash, token_count, .. } = update else {
            return false;
        };
        ctx.cached_content = Some(content);
        ctx.file_hash = Some(hash);
        ctx.full_token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        // token_count reflects current page, not full content
        if ctx.total_pages > 1 {
            let page_content = paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
            ctx.token_count = estimate_tokens(&page_content);
        } else {
            ctx.token_count = token_count;
        }
        ctx.cache_deprecated = false;
        ctx.last_refresh_ms = now_ms();
        true
    }

    fn refresh(&self, _state: &mut State) {
        // File refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshFile { context_id, file_path, current_hash } = request else {
            return None;
        };
        let path = PathBuf::from(&file_path);
        if !path.exists() {
            return None;
        }
        // Hard byte limit: refuse to load oversized files
        if let Ok(meta) = fs::metadata(&path) {
            if meta.len() as usize > PANEL_MAX_LOAD_BYTES {
                let msg = format!(
                    "[File too large to load: {} bytes (limit: {} bytes). Close this panel and use grep or other tools to inspect portions of the file.]",
                    meta.len(), PANEL_MAX_LOAD_BYTES
                );
                let token_count = estimate_tokens(&msg);
                return Some(CacheUpdate::FileContent {
                    context_id,
                    content: msg,
                    hash: format!("oversized_{}", meta.len()),
                    token_count,
                });
            }
        }
        let content = fs::read_to_string(&path).ok()?;
        let new_hash = hash_content(&content);
        if current_hash.as_ref() == Some(&new_hash) {
            return None;
        }
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::FileContent {
            context_id,
            content,
            hash: new_hash,
            token_count,
        })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::File)
            .filter_map(|c| {
                let path = c.file_path.as_ref()?;
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
            let path = ctx.file_path.as_deref().unwrap_or("");
            // Use cached content only - no blocking file reads
            let content = ctx.cached_content.clone()
                .unwrap_or_else(|| {
                    if ctx.cache_deprecated {
                        "Loading...".to_string()
                    } else {
                        "No content".to_string()
                    }
                });
            (content, path.to_string())
        } else {
            (String::new(), String::new())
        };

        // Get syntax highlighting
        let highlighted = if !file_path.is_empty() {
            highlight_file(&file_path, &content)
        } else {
            Vec::new()
        };

        let mut text: Vec<Line> = Vec::new();

        if highlighted.is_empty() {
            for (i, line) in content.lines().enumerate() {
                let line_num = i + 1;
                text.push(Line::from(vec![
                    Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::text_muted()).bg(theme::bg_base())),
                    Span::styled(" ", base_style),
                    Span::styled(line.to_string(), Style::default().fg(theme::text())),
                ]));
            }
        } else {
            for (i, spans) in highlighted.iter().enumerate() {
                let line_num = i + 1;
                let mut line_spans = vec![
                    Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::text_muted()).bg(theme::bg_base())),
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
