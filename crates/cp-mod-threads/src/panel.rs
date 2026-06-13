use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{ThreadStatus, ThreadsState};
use std::fmt::Write as _;

/// Panel that renders the thread list and provides thread context to the LLM.
pub(crate) struct ThreadsPanel;

impl ThreadsPanel {
    /// Format thread data for LLM context.
    ///
    /// `MY_TURN` threads get full message history (last 5).
    /// `THEIR_TURN` threads get a summary line only.
    fn format_threads_for_context(state: &State) -> String {
        let ts = ThreadsState::get(state);
        if ts.threads.is_empty() {
            return "No threads".to_string();
        }

        let mut output = String::new();
        for thread in &ts.threads {
            let _r = writeln!(output, "{} [{}]  \"{}\"  ({} messages)", thread.id, thread.status, thread.name, thread.messages.len());

            match thread.status {
                ThreadStatus::MyTurn => {
                    // Show last 5 messages for MY_TURN threads
                    let start = thread.messages.len().saturating_sub(5);
                    for msg in thread.messages.get(start..).unwrap_or_default() {
                        let content_preview = msg
                            .content
                            .as_deref()
                            .unwrap_or("[no text]");
                        let truncated = if content_preview.len() > 120 {
                            format!(
                                "{}...",
                                content_preview
                                    .get(..content_preview.floor_char_boundary(117))
                                    .unwrap_or("")
                            )
                        } else {
                            content_preview.to_string()
                        };
                        let _w = writeln!(output, "  └─ [{}] {}", msg.author, truncated);
                    }
                }
                ThreadStatus::TheirTurn => {
                    // Summary only for THEIR_TURN
                    let _w = writeln!(output, "  └─ (waiting for user)");
                }
            }
        }

        output.trim_end().to_string()
    }
}

impl Panel for ThreadsPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let ts = ThreadsState::get(state);

        if ts.threads.is_empty() {
            return vec![
                Block::Line(vec![S::muted("  No threads".into()).italic()]),
                Block::Line(vec![S::muted("  Threads are created from the Threads view (Ctrl+V)".into())]),
            ];
        }

        let mut blocks = Vec::new();
        for thread in &ts.threads {
            // Thread header: T1 [MY_TURN]  "lint audit"  (3 messages)
            let status_style = match thread.status {
                ThreadStatus::MyTurn => Semantic::Success,
                ThreadStatus::TheirTurn => Semantic::Muted,
            };
            blocks.push(Block::Line(vec![
                S::new("  ".into()),
                S::accent(thread.id.clone()).bold(),
                S::new(" ".into()),
                S::styled(format!("[{}]", thread.status), status_style),
                S::new("  ".into()),
                S::new(format!("\"{}\"", thread.name)).bold(),
                S::muted(format!("  ({} msgs)", thread.messages.len())),
            ]));

            // Last 5 messages
            let start = thread.messages.len().saturating_sub(5);
            for msg in thread.messages.get(start..).unwrap_or_default() {
                let content_preview = msg
                    .content
                    .as_deref()
                    .unwrap_or("[no text]");
                let truncated = if content_preview.len() > 80 {
                    format!(
                        "{}...",
                        content_preview
                            .get(..content_preview.floor_char_boundary(77))
                            .unwrap_or("")
                    )
                } else {
                    content_preview.to_string()
                };
                blocks.push(Block::Line(vec![
                    S::new("    └─ ".into()),
                    S::accent(format!("[{}]", msg.author)),
                    S::new(" ".into()),
                    S::muted(truncated),
                ]));
            }

            blocks.push(Block::Empty);
        }

        blocks
    }

    fn title(&self, _state: &State) -> String {
        "Threads".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_threads_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::THREADS {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_threads_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::THREADS)
            .map_or(("P?", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Threads", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }
    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }
    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }
    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}
