use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::actions::Action;
use crate::constants::SCROLL_ARROW_AMOUNT;
use crate::core::panels::{ContextItem, Panel};
use crate::state::{ContextType, State, estimate_tokens};
use crate::ui::theme;

pub struct ScratchpadPanel;

impl ScratchpadPanel {
    /// Format scratchpad cells for LLM context
    fn format_cells_for_context(state: &State) -> String {
        if state.scratchpad_cells.is_empty() {
            return "No scratchpad cells".to_string();
        }

        let mut output = String::new();
        for cell in &state.scratchpad_cells {
            output.push_str(&format!("=== [{}] {} ===\n", cell.id, cell.title));
            output.push_str(&cell.content);
            output.push_str("\n\n");
        }

        output.trim_end().to_string()
    }
}

impl Panel for ScratchpadPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, _state: &State) -> String {
        "Scratchpad".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_cells_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type == ContextType::Scratchpad {
                ctx.token_count = token_count;
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_cells_for_context(state);
        // Find the Scratchpad context element to get its ID and timestamp
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::Scratchpad)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P7", 0));
        vec![ContextItem::new(id, "Scratchpad", content, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if state.scratchpad_cells.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("No scratchpad cells".to_string(), Style::default().fg(theme::text_muted()).italic()),
            ]));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(
                    "Use scratchpad_create_cell to add notes".to_string(),
                    Style::default().fg(theme::text_muted()),
                ),
            ]));
        } else {
            for cell in &state.scratchpad_cells {
                // Cell header
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(cell.id.clone(), Style::default().fg(theme::accent()).bold()),
                    Span::styled(" ", base_style),
                    Span::styled(cell.title.clone(), Style::default().fg(theme::text()).bold()),
                ]));

                // Cell content (show first few lines, truncated)
                let lines: Vec<&str> = cell.content.lines().take(5).collect();
                for line in &lines {
                    text.push(Line::from(vec![
                        Span::styled("   ".to_string(), base_style),
                        Span::styled(line.to_string(), Style::default().fg(theme::text_secondary())),
                    ]));
                }

                // Show ellipsis if content is longer
                let total_lines = cell.content.lines().count();
                if total_lines > 5 {
                    text.push(Line::from(vec![
                        Span::styled("   ".to_string(), base_style),
                        Span::styled(
                            format!("... ({} more lines)", total_lines - 5),
                            Style::default().fg(theme::text_muted()).italic(),
                        ),
                    ]));
                }

                // Blank line between cells
                text.push(Line::from(vec![Span::styled("".to_string(), base_style)]));
            }
        }

        text
    }
}
