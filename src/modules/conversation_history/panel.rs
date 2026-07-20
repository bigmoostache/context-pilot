use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel, paginate_content};
use crate::state::{Kind, State};
use cp_base::panels::scroll_key_action;
use cp_base::state::data::message::MsgKind;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via `detach_conversation_chunks`) and never refreshed.
pub(super) struct ConversationHistoryPanel;

/// Render a `TextMessage` history entry: role header + content lines.
fn render_text_hist(msg: &cp_base::state::data::message::Message) -> Vec<cp_render::Block> {
    let mut blocks = Vec::new();
    let (icon, semantic) = if msg.role == "user" {
        ("\u{1f464}", cp_render::Semantic::Accent)
    } else {
        ("\u{1f916}", cp_render::Semantic::AccentDim)
    };
    blocks.push(cp_render::Block::Line(vec![
        cp_render::Span::styled(format!("{icon} "), semantic),
        cp_render::Span::styled(format!("[{}]", msg.id), semantic).bold(),
    ]));
    if !msg.content.is_empty() {
        for line in msg.content.lines() {
            blocks.push(cp_render::Block::Line(vec![cp_render::Span::new(line.to_owned())]));
        }
    }
    blocks.push(cp_render::Block::Empty);
    blocks
}

/// Render a `ToolCall` history entry: header + one line per tool name.
fn render_toolcall_hist(msg: &cp_base::state::data::message::Message) -> Vec<cp_render::Block> {
    let mut blocks = Vec::new();
    blocks.push(cp_render::Block::Line(vec![
        cp_render::Span::styled("\u{1f527} ".to_owned(), cp_render::Semantic::Info),
        cp_render::Span::styled(format!("[{}] tool_call", msg.id), cp_render::Semantic::Info).bold(),
    ]));
    for tu in &msg.tool_uses {
        blocks.push(cp_render::Block::Line(vec![
            cp_render::Span::muted("  \u{2192} ".to_owned()),
            cp_render::Span::styled(tu.name.clone(), cp_render::Semantic::Accent),
        ]));
    }
    blocks.push(cp_render::Block::Empty);
    blocks
}

/// Render a `ToolResult` history entry: header + first-line preview per result.
fn render_toolresult_hist(msg: &cp_base::state::data::message::Message) -> Vec<cp_render::Block> {
    let mut blocks = Vec::new();
    blocks.push(cp_render::Block::Line(vec![
        cp_render::Span::styled("\u{1f4cb} ".to_owned(), cp_render::Semantic::Muted),
        cp_render::Span::styled(format!("[{}] tool_result", msg.id), cp_render::Semantic::Muted),
    ]));
    for tr in &msg.tool_results {
        let preview = tr.content.lines().next().unwrap_or("");
        let truncated = if preview.len() > 80 {
            format!("{}…", preview.get(..80).unwrap_or(preview))
        } else {
            preview.to_owned()
        };
        blocks.push(cp_render::Block::Line(vec![
            cp_render::Span::muted("  ".to_owned()),
            cp_render::Span::muted(truncated),
        ]));
    }
    blocks.push(cp_render::Block::Empty);
    blocks
}

/// Render a single message into IR blocks (simplified history view).
fn render_message_blocks(msg: &cp_base::state::data::message::Message) -> Vec<cp_render::Block> {
    if msg.msg_type == MsgKind::ToolCall {
        render_toolcall_hist(msg)
    } else if msg.msg_type == MsgKind::ToolResult {
        render_toolresult_hist(msg)
    } else {
        render_text_hist(msg)
    }
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
                    cp_render::Span::muted("No conversation history.".to_owned()).italic(),
                ])];
            }
        };

        // Prefer rendering from history_messages (structured message data)
        if let Some(msgs) = ctx.history_messages.as_ref() {
            let mut blocks = Vec::new();
            for msg in msgs {
                blocks.extend(render_message_blocks(msg));
            }
            if blocks.is_empty() {
                blocks.push(cp_render::Block::Line(vec![
                    cp_render::Span::muted("No messages in this history block.".to_owned()).italic(),
                ]));
            }
            return blocks;
        }

        // Fallback: plain-text rendering from cached_content
        if let Some(content) = ctx.cached_content.as_ref() {
            return content
                .lines()
                .map(|line| cp_render::Block::Line(vec![cp_render::Span::muted(line.to_owned())]))
                .collect();
        }

        vec![cp_render::Block::Line(vec![
            cp_render::Span::muted("No messages in this history block.".to_owned()).italic(),
        ])]
    }
    fn title(&self, state: &State) -> String {
        state
            .context
            .get(state.selected_context)
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .map_or_else(|| "Chat History".to_owned(), |c| c.name.clone())
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
                let output = paginate_content(content, c.current_page, c.total_pages, &c.page_descriptions);
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
