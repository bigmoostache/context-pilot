use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::state::State;
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// Panel that displays tool configuration, agents, skills, and presets.
pub(super) struct ToolsPanel;

impl Panel for ToolsPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let mut blocks = Vec::new();

        blocks.extend(super::tools_blocks::tools_blocks(state));
        blocks.push(cp_render::Block::Separator);
        blocks.extend(super::tools_blocks::seeds_blocks(state));

        let presets_section = super::tools_blocks::presets_blocks();
        if !presets_section.is_empty() {
            blocks.push(cp_render::Block::Separator);
            blocks.extend(presets_section);
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Configuration".to_string()
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = generate_tools_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == "tools")
            .map_or(("P?", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Tools", content, last_refresh_ms)]
    }

    fn refresh(&self, state: &mut State) {
        let content = generate_tools_context(state);
        let token_count = crate::state::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == "tools") {
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

/// Generate the plain-text/markdown tools context sent to the LLM.
fn generate_tools_context(state: &State) -> String {
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

    let mut output = format!("Tools ({enabled_count} enabled, {disabled_count} disabled):\n\n");
    output.push_str("| Category | Tool | Status | Description |\n");
    output.push_str("|----------|------|--------|-------------|\n");
    for tool in &state.tools {
        let status = if tool.enabled { "\u{2713}" } else { "\u{2717}" };
        let _r = writeln!(output, "| {} | {} | {} | {} |", tool.category, tool.id, status, tool.short_desc);
    }

    output
}
