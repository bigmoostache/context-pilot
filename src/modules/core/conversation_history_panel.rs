use ratatui::prelude::*;

use crate::core::panels::{paginate_content, ContextItem, Panel};
use crate::modules::core::conversation_render;
use crate::state::{ContextType, State};
use crate::ui::theme;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via detach_conversation_chunks) and never refreshed.
pub struct ConversationHistoryPanel;

impl Panel for ConversationHistoryPanel {
    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context)
            .filter(|c| c.context_type == ContextType::ConversationHistory)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Chat History".to_string())
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::ConversationHistory)
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
            Some(c) if c.context_type == ContextType::ConversationHistory => c,
            _ => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "No conversation history.".to_string(),
                        Style::default().fg(theme::text_muted()).italic(),
                    ),
                ]));
                return lines;
            }
        };

        // Prefer rendering from history_messages (full formatting with icons/markdown)
        if let Some(ref msgs) = ctx.history_messages {
            for msg in msgs {
                let msg_lines = conversation_render::render_message(
                    msg, viewport_width, base_style, false, state.dev_mode,
                );
                lines.extend(msg_lines);
            }
        } else if let Some(content) = &ctx.cached_content {
            // Fallback: plain-text rendering for panels that only have cached_content
            for line in content.lines() {
                lines.push(Line::from(vec![
                    Span::styled(line.to_string(), base_style.fg(theme::text_muted())),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "No messages in this history block.".to_string(),
                    Style::default().fg(theme::text_muted()).italic(),
                ),
            ]));
        }
        lines
    }
}
