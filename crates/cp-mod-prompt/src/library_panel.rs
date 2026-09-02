use crossterm::event::KeyEvent;

use crate::types::{PromptState, PromptType};

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind};
use cp_base::state::runtime::State;
use std::fmt::Write as _;

/// Render `s` as a single-line, double-quoted YAML scalar: backslashes and
/// quotes are escaped and newlines flattened to spaces, so a description
/// containing `:`, `|`, `"`, or line breaks stays valid, one-line YAML.
fn yaml_scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ");
    format!("\"{escaped}\"")
}

/// Append the agents YAML sequence (each entry carries an `active` bool).
fn push_agents_yaml(content: &mut String, agents: &[crate::types::PromptItem], active_id: Option<&str>) {
    content.push_str("agents:\n");
    for agent in agents {
        let active = active_id == Some(&agent.id);
        let _wa = writeln!(content, "  - id: {}", agent.id);
        let _wn = writeln!(content, "    name: {}", yaml_scalar(&agent.name));
        let _wc = writeln!(content, "    active: {active}");
        let _wd = writeln!(content, "    description: {}", yaml_scalar(&agent.description));
    }
}

/// Append the skills YAML sequence (each entry carries a `loaded` bool), if any exist.
fn push_skills_yaml(content: &mut String, skills: &[crate::types::PromptItem], loaded: &[String]) {
    if skills.is_empty() {
        return;
    }
    content.push_str("\nskills:\n");
    for skill in skills {
        let is_loaded = loaded.contains(&skill.id);
        let _wi = writeln!(content, "  - id: {}", skill.id);
        let _wn = writeln!(content, "    name: {}", yaml_scalar(&skill.name));
        let _wl = writeln!(content, "    loaded: {is_loaded}");
        let _wd = writeln!(content, "    description: {}", yaml_scalar(&skill.description));
    }
}

/// Append the commands YAML sequence (each entry's slash invocation is `/{id}`), if any exist.
fn push_commands_yaml(content: &mut String, commands: &[crate::types::PromptItem]) {
    if commands.is_empty() {
        return;
    }
    content.push_str("\ncommands:\n");
    for cmd in commands {
        let _wi = writeln!(content, "  - id: {}", cmd.id);
        let _wc = writeln!(content, "    invoke: /{}", cmd.id);
        let _wn = writeln!(content, "    name: {}", yaml_scalar(&cmd.name));
        let _wd = writeln!(content, "    description: {}", yaml_scalar(&cmd.description));
    }
}

/// Append the "how to manage behaviours" cheat sheet as YAML comment lines.
fn push_crud_cheatsheet(content: &mut String) {
    content.push_str("\n# Managing behaviours (fleet-shared, under ~/.context-pilot/behaviours/):\n");
    content.push_str("#   create:        Behaviour_create(name, type, content) - type: agent | skill | command\n");
    let _wd = writeln!(
        content,
        "#   edit:          Edit the .md file - agents: {}/  skills: {}/  commands: {}/",
        crate::storage::dir_for(PromptType::Agent).display(),
        crate::storage::dir_for(PromptType::Skill).display(),
        crate::storage::dir_for(PromptType::Command).display()
    );
    content.push_str("#   delete:        remove the .md file (removals are detected automatically)\n");
    content.push_str("#   activate agent: agent_load(id) - pass an empty id to revert to default\n");
    content.push_str("#   load skill:    skill_load(id) - unload by closing its panel with Close_panel\n");
}

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
        push_agents_yaml(&mut content, &agents, ps.active_agent_id.as_deref());
        push_skills_yaml(&mut content, &skills, &ps.loaded_skill_ids);
        push_commands_yaml(&mut content, &commands);
        push_crud_cheatsheet(&mut content);

        vec![ContextItem::new(&ctx.id, "Library", content, ctx.last_refresh_ms)]
    }
}
