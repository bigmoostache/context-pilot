use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::highlight::highlight_file;
use crate::state::{ContextType, State};
use crate::ui::theme;

pub struct FilePanel;

impl Panel for FilePanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context)
            .map(|ctx| ctx.name.clone())
            .unwrap_or_else(|| "File".to_string())
    }

    fn refresh(&self, _state: &mut State) {
        // File refresh is handled by background cache system
        // No blocking operations here
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::File)
            .filter_map(|c| {
                let path = c.file_path.as_ref()?;
                // Use cached content only - no blocking file reads
                let content = c.cached_content.as_ref().cloned()?;
                Some(ContextItem::new(&c.id, format!("File: {}", path), content, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let selected = state.context.get(state.selected_context);

        let (content, file_path) = if let Some(ctx) = selected {
            let path = ctx.file_path.as_deref().unwrap_or("");
            // Use cached content only - no blocking file reads
            let content = ctx.cached_content.clone()
                .unwrap_or_else(|| {
                    if ctx.cache_deprecated {
                        "Loading...".to_string()
                    } else {
                        "No content".to_string()
                    }
                });
            (content, path.to_string())
        } else {
            (String::new(), String::new())
        };

        // Get syntax highlighting
        let highlighted = if !file_path.is_empty() {
            highlight_file(&file_path, &content)
        } else {
            Vec::new()
        };

        let mut text: Vec<Line> = Vec::new();

        if highlighted.is_empty() {
            for (i, line) in content.lines().enumerate() {
                let line_num = i + 1;
                text.push(Line::from(vec![
                    Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::TEXT_MUTED).bg(theme::BG_BASE)),
                    Span::styled(" ", base_style),
                    Span::styled(line.to_string(), Style::default().fg(theme::TEXT)),
                ]));
            }
        } else {
            for (i, spans) in highlighted.iter().enumerate() {
                let line_num = i + 1;
                let mut line_spans = vec![
                    Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::TEXT_MUTED).bg(theme::BG_BASE)),
                    Span::styled(" ", base_style),
                ];

                for (color, text) in spans {
                    line_spans.push(Span::styled(text.clone(), Style::default().fg(*color)));
                }

                text.push(Line::from(line_spans));
            }
        }

        text
    }
}
