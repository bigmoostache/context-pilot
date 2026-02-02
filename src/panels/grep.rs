use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::state::{ContextType, State};
use crate::ui::{theme, chars};

pub struct GrepPanel;

impl Panel for GrepPanel {
    fn title(&self, state: &State) -> String {
        if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content to count matches
            let count = ctx.cached_content.as_ref()
                .map(|c| c.lines().count())
                .unwrap_or(0);
            format!("{} ({} matches)", ctx.name, count)
        } else {
            "Grep".to_string()
        }
    }

    fn refresh(&self, _state: &mut State) {
        // Grep refresh is handled by background cache system
        // No blocking operations here
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::Grep)
            .filter_map(|c| {
                let pattern = c.grep_pattern.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref().cloned()?;
                Some(ContextItem::new(format!("Grep: {}", pattern), content))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let content = if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content only - no blocking operations
            ctx.cached_content.as_ref()
                .cloned()
                .unwrap_or_else(|| {
                    if ctx.cache_deprecated {
                        "Loading...".to_string()
                    } else {
                        "No results".to_string()
                    }
                })
        } else {
            String::new()
        };

        content.lines()
            .map(|line| {
                let parts: Vec<&str> = line.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    Line::from(vec![
                        Span::styled("  ".to_string(), base_style),
                        Span::styled(parts[0].to_string(), Style::default().fg(theme::ACCENT_DIM)),
                        Span::styled(":".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(parts[1].to_string(), Style::default().fg(theme::WARNING)),
                        Span::styled(":".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(parts[2].to_string(), Style::default().fg(theme::TEXT)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(format!("  {} ", chars::DOT), Style::default().fg(theme::ACCENT_DIM)),
                        Span::styled(line.to_string(), Style::default().fg(theme::TEXT)),
                    ])
                }
            })
            .collect()
    }
}
