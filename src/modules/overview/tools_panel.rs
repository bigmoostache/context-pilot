use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::infra::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::State;

pub(super) struct ToolsPanel;

impl Panel for ToolsPanel {
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
        "Configuration".to_string()
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

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        use super::render::separator;

        let mut text = super::render_details::render_tools(state, base_style);
        text.extend(separator());
        text.extend(super::render_details::render_seeds(state, base_style));

        let presets_section = super::render_details::render_presets(base_style);
        if !presets_section.is_empty() {
            text.extend(separator());
            text.extend(presets_section);
        }

        text
    }
}

/// Generate the plain-text/markdown tools context sent to the LLM.
fn generate_tools_context(state: &State) -> String {
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

    let mut output = format!("Tools ({} enabled, {} disabled):\n\n", enabled_count, disabled_count);
    output.push_str("| Category | Tool | Status | Description |\n");
    output.push_str("|----------|------|--------|-------------|\n");
    for tool in &state.tools {
        let status = if tool.enabled { "\u{2713}" } else { "\u{2717}" };
        output.push_str(&format!("| {} | {} | {} | {} |\n", tool.category, tool.id, status, tool.short_desc));
    }

    output
}
