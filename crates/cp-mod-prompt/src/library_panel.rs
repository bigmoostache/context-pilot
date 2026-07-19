use crossterm::event::KeyEvent;

use crate::types::{PromptState, PromptType};

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

    fn title(&self, _state: &State) -> String {
        "Library".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let items = self.context(state);
        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == Kind::new(Kind::LIBRARY)) {
            let total: usize = items.iter().map(|i| cp_base::state::context::estimate_tokens(&i.content)).sum();
            ctx.token_count = total;
            let combined: String = items.iter().map(|i| i.content.as_str()).collect::<Vec<_>>().join("\n");
            let _changed = cp_base::panels::update_if_changed(ctx, &combined);
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
        let agents = crate::storage::load_prompts_for(PromptType::Agent);
        let skills = crate::storage::load_prompts_for(PromptType::Skill);
        let commands = crate::storage::load_prompts_for(PromptType::Command);

        let mut content = String::new();

        // Agents table
        content.push_str("Agents (system prompts):\n\n");
        content.push_str("| ID | Name | Active | Description |\n");
        content.push_str("|------|------|--------|-------------|\n");
        for agent in &agents {
            let active = if ps.active_agent_id.as_deref() == Some(&agent.id) { "✓" } else { "" };
            let _wa = writeln!(content, "| {} | {} | {} | {} |", agent.id, agent.name, active, agent.description);
        }

        // Skills table
        if !skills.is_empty() {
            content.push_str("\nSkills (use skill_load to load, Close_panel to unload):\n\n");
            content.push_str("| ID | Name | Loaded | Description |\n");
            content.push_str("|------|------|--------|-------------|\n");
            for skill in &skills {
                let loaded = if ps.loaded_skill_ids.contains(&skill.id) { "✓" } else { "" };
                let _wb = writeln!(content, "| {} | {} | {} | {} |", skill.id, skill.name, loaded, skill.description);
            }
        }

        // Commands table
        if !commands.is_empty() {
            content.push_str("\nCommands:\n\n");
            content.push_str("| Command | Name | Description |\n");
            content.push_str("|---------|------|-------------|\n");
            for cmd in &commands {
                let _wc = writeln!(content, "| /{} | {} | {} |", cmd.id, cmd.name, cmd.description);
            }
        }

        // CRUD cheat sheet for the LLM
        content.push_str("\nHow to manage behaviours:\n");
        content.push_str("- Create: Behaviour_create(name, type, content) — type: 'agent', 'skill', or 'command'\n");
        let _wd = writeln!(
            content,
            "- Edit: use Edit tool on the .md file — agents: {}/  skills: {}/  commands: {}/",
            crate::storage::dir_for(PromptType::Agent).display(),
            crate::storage::dir_for(PromptType::Skill).display(),
            crate::storage::dir_for(PromptType::Command).display()
        );
        content.push_str("- Delete: delete the .md file (the system detects removals automatically)\n");
        content.push_str("- Activate agent: agent_load(id) — pass empty id to revert to default\n");
        content.push_str("- Load skill: skill_load(id) — unload by closing its panel with Close_panel\n");

        vec![ContextItem::new(&ctx.id, "Library", content, ctx.last_refresh_ms)]
    }
}
