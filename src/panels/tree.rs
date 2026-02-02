use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{ContextType, State};
use crate::ui::{theme, helpers::*};

pub struct TreePanel;

impl Panel for TreePanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, _state: &State) -> String {
        "Directory Tree".to_string()
    }

    fn refresh(&self, _state: &mut State) {
        // Tree refresh is handled by background cache system
        // No blocking operations here
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Find tree context and use cached content
        for ctx in &state.context {
            if ctx.context_type == ContextType::Tree {
                if let Some(content) = &ctx.cached_content {
                    if !content.is_empty() {
                        return vec![ContextItem::new("Directory Tree", content.clone())];
                    }
                }
                break;
            }
        }
        Vec::new()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        // Find tree context and use cached content
        let tree_content = state.context.iter()
            .find(|c| c.context_type == ContextType::Tree)
            .and_then(|ctx| ctx.cached_content.as_ref())
            .cloned()
            .unwrap_or_else(|| "Loading...".to_string());

        let mut text: Vec<Line> = Vec::new();
        for line in tree_content.lines() {
            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::styled(" ".to_string(), Style::default().fg(theme::TEXT)));

            // Check for description (after " - ")
            let (main_line, description) = if let Some(desc_idx) = line.find(" - ") {
                (&line[..desc_idx], Some(&line[desc_idx..]))
            } else {
                (line, None)
            };

            // Parse the main part
            if let Some(size_start) = find_size_pattern(main_line) {
                let (before_size, size_part) = main_line.split_at(size_start);
                spans.push(Span::styled(before_size.to_string(), Style::default().fg(theme::TEXT)));
                spans.push(Span::styled(size_part.to_string(), Style::default().fg(theme::ACCENT_DIM)));
            } else if let Some((start, end)) = find_children_pattern(main_line) {
                let before = &main_line[..start];
                let children_part = &main_line[start..end];
                let after = &main_line[end..];
                spans.push(Span::styled(before.to_string(), Style::default().fg(theme::TEXT)));
                spans.push(Span::styled(children_part.to_string(), Style::default().fg(theme::ACCENT)));
                if !after.is_empty() {
                    spans.push(Span::styled(after.to_string(), Style::default().fg(theme::TEXT)));
                }
            } else {
                spans.push(Span::styled(main_line.to_string(), Style::default().fg(theme::TEXT)));
            }

            if let Some(desc) = description {
                spans.push(Span::styled(desc.to_string(), Style::default().fg(theme::TEXT_MUTED)));
            }

            text.push(Line::from(spans));
        }

        text
    }
}
