use std::fs;
use std::path::PathBuf;

use crossterm::event::KeyEvent;

use cp_base::cast::Safe as _;

use cp_base::config::constants;
use cp_base::panels::scroll_key_action;
use cp_base::panels::{CacheRequest, CacheUpdate, hash_content};
use cp_base::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens};
use cp_base::state::runtime::State;

/// Data sent to the background cache thread for a file panel refresh.
pub(crate) struct FileCacheRequest {
    /// Identifier of the context element to update.
    pub context_id: String,
    /// Absolute path to the file on disk.
    pub file_path: String,
    /// Hash of the currently cached source (used to skip unchanged files).
    pub current_source_hash: Option<String>,
}

/// Panel implementation for displaying file contents with syntax highlighting.
pub(crate) struct FilePanel;

impl Panel for FilePanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn suicide(&self, ctx: &Entry, _state: &State) -> bool {
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
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let selected = state.context.get(state.selected_context);

        let (content, file_path) = selected.map_or_else(
            || (String::new(), String::new()),
            |ctx| {
                let path = ctx.get_meta_str("file_path").unwrap_or("");
                let content = ctx.cached_content.clone().unwrap_or_else(|| {
                    if ctx.cache_deprecated { "Loading...".to_string() } else { "No content".to_string() }
                });
                (content, path.to_string())
            },
        );

        // Get IR syntax highlighting (RGB spans)
        let highlighted = if file_path.is_empty() {
            std::sync::Arc::new(Vec::new())
        } else {
            state.highlight_ir_fn.map_or_else(|| std::sync::Arc::new(Vec::new()), |f| f(&file_path, &content))
        };

        let mut blocks = Vec::new();

        if highlighted.is_empty() {
            // Plain text fallback — no syntax highlighting available
            for (i, line) in content.lines().enumerate() {
                let line_num = i.saturating_add(1);
                blocks.push(cp_render::Block::Line(vec![
                    cp_render::Span::muted(format!(" {line_num:4} ")),
                    cp_render::Span::new(" ".to_string()),
                    cp_render::Span::new(line.to_string()),
                ]));
            }
        } else {
            for (i, spans) in highlighted.iter().enumerate() {
                let line_num = i.saturating_add(1);
                let mut line_spans =
                    vec![cp_render::Span::muted(format!(" {line_num:4} ")), cp_render::Span::new(" ".to_string())];
                line_spans.extend(spans.iter().cloned());
                blocks.push(cp_render::Block::Line(line_spans));
            }
        }

        blocks
    }
    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context).map_or_else(|| "File".to_string(), |ctx| ctx.name.clone())
    }

    fn build_cache_request(&self, ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        let path = ctx.get_meta_str("file_path")?;
        Some(CacheRequest {
            context_type: Kind::new(Kind::FILE),
            data: Box::new(FileCacheRequest {
                context_id: ctx.id.clone(),
                file_path: path.to_string(),
                current_source_hash: ctx.source_hash.clone(),
            }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, _state: &mut State) -> bool {
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
            && meta.len().to_usize() > constants::PANEL_MAX_LOAD_BYTES
        {
            let msg = format!(
                "[File too large to load: {} bytes (limit: {} bytes). Close this panel and use grep or other tools to inspect portions of the file.]",
                meta.len(),
                constants::PANEL_MAX_LOAD_BYTES
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

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == Kind::FILE)
            .filter_map(|c| {
                let path = c.get_meta_str("file_path")?;
                // Use cached content only - no blocking file reads
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, format!("File: {path}"), output, c.last_refresh_ms))
            })
            .collect()
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
}
