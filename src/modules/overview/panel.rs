use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Style};

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::state::{ContextType, State};

use super::render;
use cp_base::panels::scroll_key_action;

/// Panel that displays overview statistics, token usage, and context elements.
pub(super) struct OverviewPanel;

impl Panel for OverviewPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn title(&self, _state: &State) -> String {
        "Statistics".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Use cached content if available (set by refresh)
        if let Some(ctx) = state.context.iter().find(|c| c.context_type.as_str() == ContextType::OVERVIEW)
            && let Some(content) = &ctx.cached_content
        {
            return vec![ContextItem::new(&ctx.id, "Statistics", content.clone(), ctx.last_refresh_ms)];
        }

        // Fallback: generate fresh
        let output = Self::generate_context_content(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == ContextType::OVERVIEW)
            .map_or(("P5", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Statistics", output, last_refresh_ms)]
    }

    fn refresh(&self, state: &mut State) {
        // Refresh git status (branch, file changes) before generating context
        cp_mod_git::refresh_git_status(state);

        let content = Self::generate_context_content(state);
        let token_count = crate::state::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == ContextType::OVERVIEW) {
            ctx.token_count = token_count;
            ctx.cached_content = Some(content.clone());
            let _r = crate::app::panels::update_if_changed(ctx, &content);
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

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &crate::state::ContextElement,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::ContextElement,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::ContextElement, _state: &State) -> bool {
        false
    }

    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}
}

impl OverviewPanel {
    /// Generate the plain-text context content for the LLM.
    fn generate_context_content(state: &State) -> String {
        super::context::generate_context_content(state)
    }
}
