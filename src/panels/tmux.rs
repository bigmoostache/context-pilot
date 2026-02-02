use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::state::{ContextType, State};
use crate::ui::{theme, chars};

pub struct TmuxPanel;

impl Panel for TmuxPanel {
    fn title(&self, state: &State) -> String {
        if let Some(ctx) = state.context.get(state.selected_context) {
            let pane_id = ctx.tmux_pane_id.as_deref().unwrap_or("?");
            format!("tmux {}", pane_id)
        } else {
            "Tmux".to_string()
        }
    }

    fn refresh(&self, _state: &mut State) {
        // Tmux refresh is handled by background cache system
        // No blocking operations here
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::Tmux)
            .filter_map(|c| {
                let pane_id = c.tmux_pane_id.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref().cloned()?;
                let desc = c.tmux_description.as_deref().unwrap_or("");
                let header = if desc.is_empty() {
                    format!("Tmux Pane {}", pane_id)
                } else {
                    format!("Tmux Pane {} ({})", pane_id, desc)
                };
                Some(ContextItem::new(header, content))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let (content, description, last_keys) = if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content only - no blocking operations
            let content = ctx.cached_content.as_ref()
                .cloned()
                .unwrap_or_else(|| {
                    if ctx.cache_deprecated {
                        "Loading...".to_string()
                    } else {
                        "No content".to_string()
                    }
                });
            let desc = ctx.tmux_description.clone().unwrap_or_default();
            let last = ctx.tmux_last_keys.clone();
            (content, desc, last)
        } else {
            (String::new(), String::new(), None)
        };

        let mut text: Vec<Line> = Vec::new();

        if !description.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(description, Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
        }
        if let Some(ref keys) = last_keys {
            text.push(Line::from(vec![
                Span::styled(" last: ".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(keys.clone(), Style::default().fg(theme::ACCENT_DIM)),
            ]));
        }
        if !text.is_empty() {
            text.push(Line::from(vec![
                Span::styled(format!(" {}", chars::HORIZONTAL.repeat(40)), Style::default().fg(theme::BORDER)),
            ]));
        }

        for line in content.lines() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(line.to_string(), Style::default().fg(theme::TEXT)),
            ]));
        }

        text
    }
}
