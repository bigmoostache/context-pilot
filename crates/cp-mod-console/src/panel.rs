use cp_base::panels::{CacheRequest, CacheUpdate, hash_content};
use cp_base::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::ConsoleState;

/// Maximum characters of console output to include in context sent to the LLM.
/// Keeps only the tail (most recent output). ~2000 tokens at ~4 chars/token.
const MAX_CONTEXT_CHARS: usize = 8_000;

/// Cache request payload: pre-read ring buffer data on the main thread.
struct ConsoleCacheRequest {
    /// Panel context ID this request belongs to.
    context_id: String,
    /// Snapshot of the ring buffer content.
    buffer_content: String,
    /// Monotonic byte count at read time (for change detection).
    total_written: u64,
    /// Previous source hash to skip no-op refreshes.
    current_source_hash: Option<String>,
}

/// Panel implementation for console session output.
pub(crate) struct ConsolePanel;

impl Panel for ConsolePanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(200)
    }

    fn suicide(&self, ctx: &Entry, state: &State) -> bool {
        let Some(session_name) = ctx.get_meta_str("console_name") else { return false };

        // Callback consoles: suicide if a newer console with the same callback_id exists.
        // This auto-closes stale failure panels when the callback re-fires.
        if let Some(cb_id) = ctx.get_meta_str("callback_id") {
            let my_ts = ctx.last_refresh_ms;
            let has_newer = state.context.iter().any(|c| {
                c.id != ctx.id
                    && c.context_type == Kind::new(Kind::CONSOLE)
                    && c.get_meta_str("callback_id") == Some(cb_id)
                    && c.last_refresh_ms > my_ts
            });
            if has_newer {
                return true;
            }
        }

        // Non-callback consoles (or no newer sibling): only check when loading
        if ctx.cached_content.is_some() {
            return false;
        }

        // If the console session no longer exists (e.g. server reloaded), close the panel
        let cs = ConsoleState::get(state);
        !cs.sessions.contains_key(session_name)
    }

    fn build_cache_request(&self, ctx: &Entry, state: &State) -> Option<CacheRequest> {
        let session_name = ctx.get_meta_str("console_name")?;
        let cs = ConsoleState::get(state);
        let handle = cs.sessions.get(session_name)?;
        let (buffer_content, total_written) = handle.buffer.read_all();

        Some(CacheRequest {
            context_type: Kind::new(Kind::CONSOLE),
            data: Box::new(ConsoleCacheRequest {
                context_id: ctx.id.clone(),
                buffer_content,
                total_written,
                current_source_hash: ctx.source_hash.clone(),
            }),
        })
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let req = request.data.downcast::<ConsoleCacheRequest>().ok()?;
        let ConsoleCacheRequest { context_id, buffer_content, total_written, current_source_hash } = *req;

        // Use total_written as a cheap change-detection proxy
        let new_hash = format!("{}_{}", total_written, hash_content(&buffer_content));
        if current_source_hash.as_ref() == Some(&new_hash) {
            return Some(CacheUpdate::Unchanged { context_id });
        }

        // Truncate to tail — keep only the most recent output for context.
        // We must snap `cut` to a valid UTF-8 char boundary before slicing,
        // since console output may contain multi-byte characters (e.g. '²').
        let truncated = if buffer_content.len() > MAX_CONTEXT_CHARS {
            let mut cut = buffer_content.len().saturating_sub(MAX_CONTEXT_CHARS);
            while cut < buffer_content.len() && !buffer_content.is_char_boundary(cut) {
                cut = cut.saturating_add(1);
            }
            let start = buffer_content
                .get(cut..)
                .unwrap_or("")
                .find('\n')
                .map_or(cut, |p| cut.saturating_add(p).saturating_add(1));
            format!(
                "[...truncated, showing last {}B of {}B...]\n{}",
                buffer_content.len().saturating_sub(start),
                buffer_content.len(),
                buffer_content.get(start..).unwrap_or("")
            )
        } else {
            buffer_content
        };

        let token_count = estimate_tokens(&truncated);
        Some(CacheUpdate::Content { context_id, content: truncated, token_count })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
            return false;
        };
        let total_written_hash = format!("{}_{}", content.len(), hash_content(&content));
        ctx.source_hash = Some(total_written_hash);
        ctx.cached_content = Some(content.clone());
        ctx.token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        ctx.cache_deprecated = false;
        let _ = update_if_changed(ctx, &content);

        // Also update status metadata from session handle
        if let Some(session_name) = ctx.get_meta_str("console_name").map(ToString::to_string) {
            let cs = ConsoleState::get(state);
            if let Some(handle) = cs.sessions.get(&session_name) {
                let status_label = handle.get_status().label();
                ctx.set_meta("console_status", &status_label);
            }
        }
        true
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let (content, command, status) = state.context.get(state.selected_context).map_or_else(
            || (String::new(), String::new(), String::new()),
            |ctx| {
                let content = ctx.cached_content.clone().unwrap_or_else(|| {
                    if ctx.cache_deprecated { "Loading...".to_string() } else { "No output".to_string() }
                });
                let cmd = ctx.get_meta_str("console_command").unwrap_or("").to_string();
                let st = ctx.get_meta_str("console_status").unwrap_or("?").to_string();
                (content, cmd, st)
            },
        );

        let status_sem = if status.starts_with("running") {
            Semantic::Accent
        } else if status.starts_with("exited(0)") {
            Semantic::Success
        } else {
            Semantic::Error
        };

        let mut blocks = vec![
            Block::Line(vec![
                S::styled(" $ ".into(), Semantic::AccentDim),
                S::new(command),
                S::styled(format!("  [{status}]"), status_sem),
            ]),
            Block::Separator,
        ];

        for line in content.lines() {
            blocks.push(Block::Line(vec![S::new(format!(" {line}"))]));
        }
        blocks
    }
    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context).map_or_else(
            || "Console".to_string(),
            |ctx| {
                let desc = ctx
                    .get_meta_str("console_description")
                    .or_else(|| ctx.get_meta_str("console_command"))
                    .unwrap_or("?");
                let status = ctx.get_meta_str("console_status").unwrap_or("?");
                format!("console: {desc} ({status})")
            },
        )
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
    }

    fn refresh(&self, _state: &mut State) {}

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == Kind::CONSOLE)
            .filter_map(|c| {
                let desc =
                    c.get_meta_str("console_description").or_else(|| c.get_meta_str("console_command")).unwrap_or("?");
                let content = c.cached_content.as_ref()?;
                let status = c.get_meta_str("console_status").unwrap_or("?");
                let header = format!("Console: {desc} ({status})");

                // Content is already truncated to MAX_CONTEXT_CHARS in refresh_cache
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, header, output, c.last_refresh_ms))
            })
            .collect()
    }
}
