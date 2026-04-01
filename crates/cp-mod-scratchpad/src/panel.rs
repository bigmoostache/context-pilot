use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Span, Style};

use cp_base::config::accessors::theme;
use cp_base::panels::{ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::ScratchpadState;
use std::fmt::Write as _;

/// Panel that renders scratchpad cells and provides their content as LLM context.
pub(crate) struct ScratchpadPanel;

impl ScratchpadPanel {
    /// Format scratchpad cells for LLM context
    fn format_cells_for_context(state: &State) -> String {
        let ss = ScratchpadState::get(state);
        if ss.scratchpad_cells.is_empty() {
            return "No scratchpad cells".to_string();
        }

        let mut output = String::new();
        for cell in &ss.scratchpad_cells {
            let _r = writeln!(output, "=== [{}] {} ===", cell.id, cell.title);
            output.push_str(&cell.content);
            output.push_str("\n\n");
        }

        output.trim_end().to_string()
    }
}

impl Panel for ScratchpadPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, _state: &State) -> String {
        "Scratchpad".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_cells_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SCRATCHPAD {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_cells_for_context(state);
        // Find the Scratchpad context element to get its ID and timestamp
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::SCRATCHPAD)
            .map_or(("P7", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Scratchpad", content, last_refresh_ms)]
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
    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let ss = ScratchpadState::get(state);
        let mut text: Vec<Line<'_>> = Vec::new();

        if ss.scratchpad_cells.is_empty() {
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
            for cell in &ss.scratchpad_cells {
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
                            format!("... ({} more lines)", total_lines.saturating_sub(5)),
                            Style::default().fg(theme::text_muted()).italic(),
                        ),
                    ]));
                }

                // Blank line between cells
                text.push(Line::from(vec![Span::styled(String::new(), base_style)]));
            }
        }

        text
    }
}
