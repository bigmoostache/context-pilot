use crossterm::event::KeyEvent;

use cp_base::config::constants;
use cp_base::modules::{run_with_timeout, truncate_output};
use cp_base::panels::{CacheRequest, CacheUpdate};
use cp_base::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens};
use cp_base::state::runtime::State;

use super::GIT_CMD_TIMEOUT_SECS;
use super::GIT_STATUS_REFRESH_MS;
use crate::types::GitResultRequest;
use cp_base::panels::scroll_key_action;

/// Panel that displays and auto-refreshes the output of a read-only git command.
pub(crate) struct GitResultPanel;

impl Panel for GitResultPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(GIT_STATUS_REFRESH_MS)
    }

    fn build_cache_request(&self, ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        let command = ctx.get_meta_str("result_command")?;
        Some(CacheRequest {
            context_type: Kind::new(Kind::GIT_RESULT),
            data: Box::new(GitResultRequest { context_id: ctx.id.clone(), command: command.to_string() }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, _state: &mut State) -> bool {
        match update {
            CacheUpdate::Content { content, token_count, .. } => {
                ctx.cached_content = Some(content);
                ctx.full_token_count = token_count;
                ctx.total_pages = compute_total_pages(token_count);
                ctx.current_page = 0;
                if ctx.total_pages > 1 {
                    let page_content = paginate_content(
                        ctx.cached_content.as_deref().unwrap_or(""),
                        ctx.current_page,
                        ctx.total_pages,
                    );
                    ctx.token_count = estimate_tokens(&page_content);
                } else {
                    ctx.token_count = token_count;
                }
                ctx.cache_deprecated = false;
                let content_ref = ctx.cached_content.clone().unwrap_or_default();
                let _ = update_if_changed(ctx, &content_ref);
                true
            }
            CacheUpdate::Unchanged { .. } | CacheUpdate::ModuleSpecific { .. } => false,
        }
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let req = request.data.downcast::<GitResultRequest>().ok()?;
        let GitResultRequest { context_id, command } = *req;

        // Parse and execute the command with timeout
        let args = super::classify::validate_git_command(&command).ok()?;

        let mut cmd = std::process::Command::new("git");
        let _ = cmd.args(&args).env("GIT_TERMINAL_PROMPT", "0");
        let output = run_with_timeout(cmd, GIT_CMD_TIMEOUT_SECS);

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let content = if stderr.trim().is_empty() {
                    stdout.to_string()
                } else if stdout.trim().is_empty() {
                    stderr.to_string()
                } else {
                    format!("{stdout}\n{stderr}")
                };
                let content = truncate_output(&content, constants::MAX_RESULT_CONTENT_BYTES);
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::Content { context_id, content, token_count })
            }
            Err(e) => {
                let content = format!("Error executing git: {e}");
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::Content { context_id, content, token_count })
            }
        }
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let ctx = state.context.get(state.selected_context).filter(|c| c.context_type.as_str() == Kind::GIT_RESULT);

        let Some(ctx) = ctx else {
            return vec![Block::styled_text(" No git result panel".into(), Semantic::Muted)];
        };

        let Some(content) = &ctx.cached_content else {
            return vec![Block::Line(vec![S::muted(" Loading...".into()).italic()])];
        };

        content
            .lines()
            .map(|line| {
                let (sem, bold) = if line.starts_with('+') && !line.starts_with("+++") {
                    (Semantic::DiffAdd, false)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    (Semantic::DiffRemove, false)
                } else if line.starts_with("@@") {
                    (Semantic::Accent, false)
                } else if line.starts_with("diff --git") || line.starts_with("+++") || line.starts_with("---") {
                    (Semantic::Code, true)
                } else if line.starts_with("commit ") {
                    (Semantic::Accent, true)
                } else {
                    (Semantic::Default, false)
                };
                let span = S::styled(format!(" {line}"), sem);
                Block::Line(vec![if bold { span.bold() } else { span }])
            })
            .collect()
    }
    fn title(&self, state: &State) -> String {
        if let Some(ctx) = state.context.get(state.selected_context)
            && ctx.context_type.as_str() == Kind::GIT_RESULT
            && let Some(cmd) = ctx.get_meta_str("result_command")
        {
            let short = if cmd.len() > 40 {
                format!("{}...", &cmd.get(..cmd.floor_char_boundary(37)).unwrap_or(""))
            } else {
                cmd.to_string()
            };
            return short;
        }
        "Git Result".to_string()
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type.as_str() != Kind::GIT_RESULT {
                continue;
            }
            let content = ctx.cached_content.as_deref().unwrap_or("[loading...]");
            let header = ctx.get_meta_str("result_command").unwrap_or("Git Result");
            let output = paginate_content(content, ctx.current_page, ctx.total_pages);
            items.push(ContextItem::new(&ctx.id, header, output, ctx.last_refresh_ms));
        }
        items
    }

    fn refresh(&self, _state: &mut State) {}
    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
