use crossterm::event::KeyEvent;

use crate::types::PromptState;
use cp_base::config::INJECTIONS;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind};
use cp_base::state::runtime::State;
use std::fmt::Write as _;

/// Panel displaying the full prompt library (agents, skills, commands).
pub(crate) struct LibraryPanel;

impl Panel for LibraryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        crate::library_blocks::library_blocks(state)
    }
    fn title(&self, state: &State) -> String {
        PromptState::get(state)
            .open_prompt_id
            .as_ref()
            .map_or_else(|| "Library".to_string(), |id| format!("Library: editing {id}"))
    }

    fn refresh(&self, state: &mut State) {
        // Compute token count from context content and track content changes
        let items = self.context(state);
        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == Kind::new(Kind::LIBRARY)) {
            let total: usize = items.iter().map(|i| cp_base::state::context::estimate_tokens(&i.content)).sum();
            ctx.token_count = total;
            // Build combined content for hash tracking
            let combined: String = items.iter().map(|i| i.content.as_str()).collect::<Vec<_>>().join("\n");
            let _ = cp_base::panels::update_if_changed(ctx, &combined);
        }
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let Some(ctx) = state.context.iter().find(|c| c.context_type == Kind::new(Kind::LIBRARY)) else {
            return Vec::new();
        };

        let ps = PromptState::get(state);
        let mut content = String::new();

        // If prompt editor is open, show warning + content for editing
        if let Some(id) = &ps.open_prompt_id {
            let item = ps
                .agents
                .iter()
                .find(|a| &a.id == id)
                .or_else(|| ps.skills.iter().find(|s| &s.id == id))
                .or_else(|| ps.commands.iter().find(|c| &c.id == id));

            if let Some(item) = item {
                let type_str = if ps.agents.iter().any(|a| &a.id == id) {
                    "agent"
                } else if ps.skills.iter().any(|s| &s.id == id) {
                    "skill"
                } else {
                    "command"
                };

                content.push_str(&INJECTIONS.editor_warnings.prompt.banner);
                content.push('\n');
                content.push_str(&INJECTIONS.editor_warnings.prompt.no_follow);
                content.push('\n');
                content.push_str(&INJECTIONS.editor_warnings.prompt.load_hint);
                content.push('\n');
                content.push_str(&INJECTIONS.editor_warnings.prompt.close_hint);
                content.push_str("\n\n");
                let _r = write!(content, "Editing {} '{}' ({}):\n\n", type_str, item.id, item.name);
                content.push_str(&item.content);
                content.push('\n');

                return vec![ContextItem::new(&ctx.id, "Library", content, ctx.last_refresh_ms)];
            }
        }

        // Normal mode: show tables
        content.push_str("Agents (system prompts):\n\n");
        content.push_str("| ID | Name | Active | Description |\n");
        content.push_str("|------|------|--------|-------------|\n");
        for agent in &ps.agents {
            let active = if ps.active_agent_id.as_deref() == Some(&agent.id) { "✓" } else { "" };
            let _r = writeln!(content, "| {} | {} | {} | {} |", agent.id, agent.name, active, agent.description);
        }

        // Skills table
        if !ps.skills.is_empty() {
            content.push_str("\nSkills (use skill_load/skill_unload):\n\n");
            content.push_str("| ID | Name | Loaded | Description |\n");
            content.push_str("|------|------|--------|-------------|\n");
            for skill in &ps.skills {
                let loaded = if ps.loaded_skill_ids.contains(&skill.id) { "✓" } else { "" };
                let _r = writeln!(content, "| {} | {} | {} | {} |", skill.id, skill.name, loaded, skill.description);
            }
        }

        // Commands table
        if !ps.commands.is_empty() {
            content.push_str("\nCommands:\n\n");
            content.push_str("| Command | Name | Description |\n");
            content.push_str("|---------|------|-------------|\n");
            for cmd in &ps.commands {
                let _r = writeln!(content, "| /{} | {} | {} |", cmd.id, cmd.name, cmd.description);
            }
        }

        vec![ContextItem::new(&ctx.id, "Library", content, ctx.last_refresh_ms)]
    }
}
