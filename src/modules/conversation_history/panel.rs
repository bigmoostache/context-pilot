use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel, paginate_content};
use crate::state::{Kind, State};
use cp_base::panels::scroll_key_action;
use cp_base::state::data::message::MsgKind;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via `detach_conversation_chunks`) and never refreshed.
pub(super) struct ConversationHistoryPanel;

/// Render a single message into IR blocks (simplified history view).
fn render_message_blocks(msg: &cp_base::state::data::message::Message) -> Vec<cp_render::Block> {
    let mut blocks = Vec::new();

    match msg.msg_type {
        MsgKind::TextMessage => {
            // Role header line
            let (icon, semantic) = if msg.role == "user" {
                ("👤", cp_render::Semantic::Accent)
            } else {
                ("🤖", cp_render::Semantic::AccentDim)
            };
            blocks.push(cp_render::Block::Line(vec![
                cp_render::Span::styled(format!("{icon} "), semantic),
                cp_render::Span::styled(format!("[{}]", msg.id), semantic).bold(),
            ]));

            // Content lines
            if !msg.content.is_empty() {
                for line in msg.content.lines() {
                    blocks.push(cp_render::Block::Line(vec![cp_render::Span::new(line.to_string())]));
                }
            }
            blocks.push(cp_render::Block::Empty);
        }
        MsgKind::ToolCall => {
            blocks.push(cp_render::Block::Line(vec![
                cp_render::Span::styled("🔧 ".to_string(), cp_render::Semantic::Info),
                cp_render::Span::styled(format!("[{}] tool_call", msg.id), cp_render::Semantic::Info).bold(),
            ]));
            for tu in &msg.tool_uses {
                blocks.push(cp_render::Block::Line(vec![
                    cp_render::Span::muted("  → ".to_string()),
                    cp_render::Span::styled(tu.name.clone(), cp_render::Semantic::Accent),
                ]));
            }
            blocks.push(cp_render::Block::Empty);
        }
        MsgKind::ToolResult => {
            blocks.push(cp_render::Block::Line(vec![
                cp_render::Span::styled("📋 ".to_string(), cp_render::Semantic::Muted),
                cp_render::Span::styled(format!("[{}] tool_result", msg.id), cp_render::Semantic::Muted),
            ]));
            for tr in &msg.tool_results {
                // Show first line of result content
                let preview = tr.content.lines().next().unwrap_or("");
                let truncated = if preview.len() > 80 {
                    format!("{}…", preview.get(..80).unwrap_or(preview))
                } else {
                    preview.to_string()
                };
                blocks.push(cp_render::Block::Line(vec![
                    cp_render::Span::muted("  ".to_string()),
                    cp_render::Span::muted(truncated),
                ]));
            }
            blocks.push(cp_render::Block::Empty);
        }
    }

    blocks
}

impl Panel for ConversationHistoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let ctx = match state.context.get(state.selected_context) {
            Some(c) if c.context_type.as_str() == Kind::CONVERSATION_HISTORY => c,
            _ => {
                return vec![cp_render::Block::Line(vec![
                    cp_render::Span::muted("No conversation history.".to_string()).italic(),
                ])];
            }
        };

        // Prefer rendering from history_messages (structured message data)
        if let Some(ref msgs) = ctx.history_messages {
            let mut blocks = Vec::new();
            for msg in msgs {
                blocks.extend(render_message_blocks(msg));
            }
            if blocks.is_empty() {
                blocks.push(cp_render::Block::Line(vec![
                    cp_render::Span::muted("No messages in this history block.".to_string()).italic(),
                ]));
            }
            return blocks;
        }

        // Fallback: plain-text rendering from cached_content
        if let Some(content) = &ctx.cached_content {
            return content
                .lines()
                .map(|line| cp_render::Block::Line(vec![cp_render::Span::muted(line.to_string())]))
                .collect();
        }

        vec![cp_render::Block::Line(vec![
            cp_render::Span::muted("No messages in this history block.".to_string()).italic(),
        ])]
    }
    fn title(&self, state: &State) -> String {
        state
            .context
            .get(state.selected_context)
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .map_or_else(|| "Chat History".to_string(), |c| c.name.clone())
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn refresh(&self, _state: &mut State) {}

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &crate::state::Entry, _state: &State) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::Entry, _state: &State) -> bool {
        false
    }
}
