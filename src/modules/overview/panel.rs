use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::infra::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{ContextType, State};

use super::render;

pub(super) struct OverviewPanel;

impl Panel for OverviewPanel {
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
        "Statistics".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Use cached content if available (set by refresh)
        if let Some(ctx) = state.context.iter().find(|c| c.context_type == ContextType::OVERVIEW)
            && let Some(content) = &ctx.cached_content
        {
            return vec![ContextItem::new(&ctx.id, "Statistics", content.clone(), ctx.last_refresh_ms)];
        }

        // Fallback: generate fresh
        let output = self.generate_context_content(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type == ContextType::OVERVIEW)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P5", 0));
        vec![ContextItem::new(id, "Statistics", output, last_refresh_ms)]
    }

    fn refresh(&self, state: &mut State) {
        // Refresh git status (branch, file changes) before generating context
        cp_mod_git::refresh_git_status(state);

        let content = self.generate_context_content(state);
        let token_count = crate::state::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::OVERVIEW) {
            ctx.token_count = token_count;
            ctx.cached_content = Some(content.clone());
            crate::app::panels::update_if_changed(ctx, &content);
        }
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let _guard = crate::profile!("panel::overview::content");
        let mut text: Vec<Line<'_>> = Vec::new();

        text.extend(render::render_token_usage(state, base_style));
        text.extend(render::separator());

        let git_section = render::render_git_status(state, base_style);
        if !git_section.is_empty() {
            text.extend(git_section);
            text.extend(render::separator());
        }

        text.extend(render::render_context_elements(state, base_style));
        text.extend(render::separator());

        text.extend(render::render_statistics(state, base_style));

        text
    }
}

impl OverviewPanel {
    fn generate_context_content(&self, state: &State) -> String {
        super::context::generate_context_content(state)
    }
}
