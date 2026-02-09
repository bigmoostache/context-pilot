use ratatui::prelude::*;

use crate::core::panels::{paginate_content, ContextItem, Panel};
use crate::modules::core::conversation_render;
use crate::state::{ContextType, State};
use crate::ui::theme;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via detach_conversation_chunks) and never refreshed.
pub struct ConversationHistoryPanel;

impl Panel for ConversationHistoryPanel {
    fn title(&self, _state: &State) -> String {
        "Chat History".to_string()
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

        for ctx in &state.context {
            if ctx.context_type != ContextType::ConversationHistory {
                continue;
            }

            // Separator header
            lines.push(Line::from(vec![
                Span::styled(
                    format!("── {} ──", ctx.name),
                    Style::default().fg(theme::text_muted()).bold(),
                ),
            ]));

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

            lines.push(Line::from(""));
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "No detached conversation chunks yet.".to_string(),
                    Style::default().fg(theme::text_muted()).italic(),
                ),
            ]));
        }
        lines
    }
}
