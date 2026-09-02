//! IR block generation for the Library panel.
//!
//! Dynamically loads prompts from disk on every render call and renders them
//! **YAML-style** (one sequence per behaviour kind), mirroring the LLM-facing
//! `context()` text. No editor mode — editing is done via the Edit tool on
//! `.md` files directly.

use cp_render::{Block, Semantic, Span as S};

use crate::types::{PromptState, PromptType};
use cp_base::state::runtime::State;

/// Build IR blocks for the library panel's TUI display.
pub(crate) fn library_blocks(state: &State) -> Vec<Block> {
    let ps = PromptState::get(state);
    let agents = crate::storage::load_prompts_for(PromptType::Agent);
    let skills = crate::storage::load_prompts_for(PromptType::Skill);
    let commands = crate::storage::load_prompts_for(PromptType::Command);

    let mut blocks = Vec::new();

    // Agents / skills / commands sequences, YAML-style (mirrors context()).
    agents_yaml(&agents, ps, &mut blocks);
    skills_yaml(&skills, ps, &mut blocks);
    commands_yaml(&commands, &mut blocks);

    blocks
}

// ── YAML helpers ─────────────────────────────────────────────────────

/// A `key: value` line indented by `indent` spaces, key muted and value styled.
fn kv_line(indent: usize, key: &str, value: String, value_sem: Semantic) -> Block {
    Block::Line(vec![S::muted(format!("{:indent$}{key}: ", "", indent = indent)), S::styled(value, value_sem)])
}

/// A YAML boolean value styled green when `true`, muted when `false`.
fn bool_line(key: &str, value: bool) -> Block {
    let sem = if value { Semantic::Success } else { Semantic::Muted };
    kv_line(4, key, value.to_string(), sem)
}

/// The `  - id: {id}` sequence-entry opener (dash + id).
fn entry_line(id: &str) -> Block {
    Block::Line(vec![S::muted("  - id: ".into()), S::styled(id.into(), Semantic::AccentDim)])
}

// ── Sequence builders ────────────────────────────────────────────────

/// Build the agents YAML sequence.
fn agents_yaml(agents: &[crate::types::PromptItem], ps: &PromptState, blocks: &mut Vec<Block>) {
    blocks.push(Block::Line(vec![S::styled("agents:".into(), Semantic::Header).bold()]));
    for agent in agents {
        let is_active = ps.active_agent_id.as_deref() == Some(&agent.id);
        blocks.push(entry_line(&agent.id));
        blocks.push(kv_line(4, "name", agent.name.clone(), Semantic::Default));
        blocks.push(bool_line("active", is_active));
        blocks.push(bool_line("builtin", agent.is_builtin));
        blocks.push(kv_line(4, "description", agent.description.clone(), Semantic::Muted));
    }
}

/// Build the skills YAML sequence.
fn skills_yaml(skills: &[crate::types::PromptItem], ps: &PromptState, blocks: &mut Vec<Block>) {
    if skills.is_empty() {
        return;
    }
    blocks.push(Block::Empty);
    blocks.push(Block::Line(vec![S::styled("skills:".into(), Semantic::Header).bold()]));
    for skill in skills {
        let is_loaded = ps.loaded_skill_ids.contains(&skill.id);
        blocks.push(entry_line(&skill.id));
        blocks.push(kv_line(4, "name", skill.name.clone(), Semantic::Default));
        blocks.push(bool_line("loaded", is_loaded));
        blocks.push(bool_line("builtin", skill.is_builtin));
        blocks.push(kv_line(4, "description", skill.description.clone(), Semantic::Muted));
    }
}

/// Build the commands YAML sequence.
fn commands_yaml(commands: &[crate::types::PromptItem], blocks: &mut Vec<Block>) {
    if commands.is_empty() {
        return;
    }
    blocks.push(Block::Empty);
    blocks.push(Block::Line(vec![S::styled("commands:".into(), Semantic::Header).bold()]));
    for cmd in commands {
        blocks.push(entry_line(&cmd.id));
        blocks.push(kv_line(4, "invoke", format!("/{}", cmd.id), Semantic::Accent));
        blocks.push(kv_line(4, "name", cmd.name.clone(), Semantic::Default));
        blocks.push(bool_line("builtin", cmd.is_builtin));
        blocks.push(kv_line(4, "description", cmd.description.clone(), Semantic::Muted));
    }
}
