use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::state::{Kind, State};

use cp_base::panels::scroll_key_action;

/// Panel that displays overview statistics, token usage, and context elements.
pub(super) struct OverviewPanel;

impl Panel for OverviewPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let mut blocks = Vec::new();

        blocks.extend(super::blocks::token_usage_blocks(state));
        blocks.push(cp_render::Block::Separator);

        let git_section = super::blocks::git_blocks(state);
        if !git_section.is_empty() {
            blocks.extend(git_section);
            blocks.push(cp_render::Block::Separator);
        }

        blocks.extend(super::blocks::context_elements_blocks(state));
        blocks.push(cp_render::Block::Separator);

        blocks.extend(super::blocks::statistics_blocks(state));

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Statistics".to_string()
    }

    fn max_freezes(&self) -> u8 {
        2
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Use cached content if available (set by refresh)
        if let Some(ctx) = state.context.iter().find(|c| c.context_type.as_str() == Kind::OVERVIEW)
            && let Some(content) = &ctx.cached_content
        {
            return vec![ContextItem::new(&ctx.id, "Statistics", content.clone(), ctx.last_refresh_ms)];
        }

        // Fallback: generate fresh
        let output = Self::generate_context_content(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::OVERVIEW)
            .map_or(("P5", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Statistics", output, last_refresh_ms)]
    }

    fn refresh(&self, state: &mut State) {
        // Refresh git status (branch, file changes) before generating context
        cp_mod_git::refresh_git_status(state);

        let content = Self::generate_context_content(state);
        let token_count = crate::state::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == Kind::OVERVIEW) {
            ctx.token_count = token_count;
            ctx.cached_content = Some(content.clone());
            let _r = crate::app::panels::update_if_changed(ctx, &content);
        }
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &crate::state::Entry, _state: &State) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::Entry, _state: &State) -> bool {
        false
    }
}

impl OverviewPanel {
    /// Generate the plain-text context content for the LLM.
    fn generate_context_content(state: &State) -> String {
        super::context::generate_context_content(state)
    }
}
