//! Status panel for connected MCP servers.
//!
//! A fixed sidebar panel listing each configured server, its connection status,
//! and the tools it advertises. Read-only — no keys beyond scrolling, no cache.

use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::estimate_tokens;
use cp_base::state::runtime::State;

use super::servers::McpState;

/// Context type / fixed-panel key for the MCP status panel.
pub const MCP_KIND: &str = "mcp";

/// Renders the MCP server registry in the sidebar.
pub(crate) struct McpPanel;

impl McpPanel {
    /// Plain-text rendering shared by the LLM context and token estimation.
    fn format_for_context(state: &State) -> String {
        use std::fmt::Write as _;

        let mcp = McpState::get(state);
        if mcp.servers.is_empty() {
            return "No MCP servers configured (.context-pilot/shared/mcp.json).".to_string();
        }

        let mut out = String::from(
            "MCP tools are disabled by default. Use tool_manage to enable the ones you need.\n\n",
        );
        for name in mcp.sorted_names() {
            let Some(entry) = mcp.servers.get(&name) else { continue };
            let _r = writeln!(out, "{name}: {}", entry.status.label());
            for tool in &entry.tools {
                let tool_id = super::tools::namespaced_id(&name, &tool.name);
                let desc = tool.description.as_deref().unwrap_or("(no description)");
                let short = first_sentence(desc, 200);
                let _t = writeln!(out, "  - {tool_id}: {short}");
            }
        }
        out.trim_end().to_string()
    }
}

impl Panel for McpPanel {
    fn title(&self, _state: &State) -> String {
        "MCP".to_string()
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let mcp = McpState::get(state);
        if mcp.servers.is_empty() {
            return vec![Block::Line(vec![S::muted("  No MCP servers configured".into()).italic()])];
        }

        let mut blocks = Vec::new();
        for name in mcp.sorted_names() {
            let Some(entry) = mcp.servers.get(&name) else { continue };
            let status_sem = if entry.status.is_connected() { Semantic::Success } else { Semantic::Error };
            blocks.push(Block::Line(vec![
                S::new(" ".into()),
                S::styled(name.clone(), Semantic::AccentDim),
                S::muted("  ".into()),
                S::styled(entry.status.label(), status_sem),
            ]));
            for tool in &entry.tools {
                blocks.push(Block::Line(vec![
                    S::new("   ".into()),
                    S::muted("• ".into()),
                    S::styled(tool.name.clone(), Semantic::Default),
                ]));
            }
        }
        blocks
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_for_context(state);
        let token_count = estimate_tokens(&content);
        for ctx in &mut state.context {
            if ctx.context_type.as_str() == MCP_KIND {
                ctx.token_count = token_count;
                let _r = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == MCP_KIND)
            .map_or(("mcp", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "MCP Servers", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}

/// Extract the first sentence from a tool description, capped at `max` chars.
/// Looks for the first newline or `. ` boundary. Appends `…` when truncated.
/// Full descriptions are available in `tool-definitions` when a tool is enabled.
fn first_sentence(desc: &str, max: usize) -> String {
    let mut out = String::new();
    let mut prev_was_period = false;
    let mut count: usize = 0;

    for ch in desc.chars() {
        if count >= max {
            out.push('…');
            return out;
        }
        if ch == '\n' {
            if !out.is_empty() && out.len() < desc.len() {
                out.push('…');
            }
            return out;
        }
        // `. ` boundary — the period is already in `out`.
        if prev_was_period && ch == ' ' {
            return out;
        }
        out.push(ch);
        prev_was_period = ch == '.';
        count = count.saturating_add(1);
    }
    out
}
