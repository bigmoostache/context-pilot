use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Span, Style};

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel, paginate_content};
use crate::modules::conversation::render;
use crate::state::{ContextType, State};
use crate::ui::theme;
use cp_base::panels::scroll_key_action;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via `detach_conversation_chunks`) and never refreshed.
pub(super) struct ConversationHistoryPanel;

impl Panel for ConversationHistoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, state: &State) -> String {
        state
            .context
            .get(state.selected_context)
            .filter(|c| c.context_type.as_str() == ContextType::CONVERSATION_HISTORY)
            .map_or_else(|| "Chat History".to_string(), |c| c.name.clone())
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == ContextType::CONVERSATION_HISTORY)
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let viewport_width = state.last_viewport_width;

        // Render only the currently selected context element
        let ctx = match state.context.get(state.selected_context) {
            Some(c) if c.context_type.as_str() == ContextType::CONVERSATION_HISTORY => c,
            _ => {
                lines.push(Line::from(vec![Span::styled(
                    "No conversation history.".to_string(),
                    Style::default().fg(theme::text_muted()).italic(),
                )]));
                return lines;
            }
        };

        // Prefer rendering from history_messages (full formatting with icons/markdown)
        if let Some(ref msgs) = ctx.history_messages {
            for msg in msgs {
                let msg_lines = render::render_message(
                    msg,
                    &render::MessageRenderOpts {
                        viewport_width,
                        base_style,
                        is_streaming: false,
                        dev_mode: state.flags.ui.dev_mode,
                    },
                );
                lines.extend(msg_lines);
            }
        } else if let Some(content) = &ctx.cached_content {
            // Fallback: plain-text rendering for panels that only have cached_content
            for line in content.lines() {
                lines.push(Line::from(vec![Span::styled(line.to_string(), base_style.fg(theme::text_muted()))]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "No messages in this history block.".to_string(),
                Style::default().fg(theme::text_muted()).italic(),
            )]));
        }
        lines
    }

    fn refresh(&self, _state: &mut State) {}

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &crate::state::ContextElement,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::ContextElement,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::ContextElement, _state: &State) -> bool {
        false
    }

    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}
}
