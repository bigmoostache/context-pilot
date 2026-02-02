use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{ContextType, State};
use crate::ui::{theme, chars};

pub struct GlobPanel;

impl Panel for GlobPanel {
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
        if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content to count files
            let count = ctx.cached_content.as_ref()
                .map(|c| c.lines().count())
                .unwrap_or(0);
            format!("{} ({} files)", ctx.name, count)
        } else {
            "Glob".to_string()
        }
    }

    fn refresh(&self, _state: &mut State) {
        // Glob refresh is handled by background cache system
        // No blocking operations here
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state.context.iter()
            .filter(|c| c.context_type == ContextType::Glob)
            .filter_map(|c| {
                let pattern = c.glob_pattern.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref().cloned()?;
                Some(ContextItem::new(format!("Glob: {}", pattern), content))
            })
            .collect()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
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
            .map(|line| Line::from(vec![
                Span::styled(format!("  {} ", chars::DOT), Style::default().fg(theme::ACCENT_DIM)),
                Span::styled(line.to_string(), Style::default().fg(theme::TEXT)),
            ]))
            .collect()
    }
}
