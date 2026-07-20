//! IR block generation for the Library panel.
//!
//! Dynamically loads prompts from disk on every render call.
//! No editor mode — editing is done via the Edit tool on `.md` files directly.

use cp_render::{Align, Block, Cell as IrCell, Semantic, Span as S};

use crate::types::{PromptState, PromptType};
use cp_base::state::runtime::State;

/// Build IR blocks for the library panel's TUI display.
pub(crate) fn library_blocks(state: &State) -> Vec<Block> {
    let ps = PromptState::get(state);
    let agents = crate::storage::load_prompts_for(PromptType::Agent);
    let skills = crate::storage::load_prompts_for(PromptType::Skill);
    let commands = crate::storage::load_prompts_for(PromptType::Command);

    let mut blocks = Vec::new();

    // Active agent + loaded skills summary
    let active_name = ps
        .active_agent_id
        .as_ref()
        .and_then(|id| agents.iter().find(|a| &a.id == id))
        .map_or("(none)", |a| a.name.as_str());

    blocks.push(Block::KeyValue(vec![(
        vec![S::muted(" Active Agent: ".into())],
        vec![S::accent(active_name.into()).bold()],
    )]));

    if !ps.loaded_skill_ids.is_empty() {
        let skill_names: Vec<String> = ps
            .loaded_skill_ids
            .iter()
            .filter_map(|id| skills.iter().find(|s| &s.id == id).map(|s| s.name.clone()))
            .collect();
        blocks.push(Block::KeyValue(vec![(
            vec![S::muted(" Loaded Skills: ".into())],
            vec![S::success(skill_names.join(", "))],
        )]));
    }
    blocks.push(Block::Empty);

    // Agents table
    agents_table(&agents, ps, &mut blocks);
    skills_table(&skills, ps, &mut blocks);
    commands_table(&commands, &mut blocks);

    blocks
}

// ── Table builders ───────────────────────────────────────────────────

/// Build the agents table section.
fn agents_table(agents: &[crate::types::PromptItem], ps: &PromptState, blocks: &mut Vec<Block>) {
    blocks.push(Block::Line(vec![
        S::muted(" AGENTS".into()).bold(),
        S::muted(format!("  ({} available)", agents.len())),
    ]));
    blocks.push(Block::Empty);

    let rows: Vec<Vec<IrCell>> = agents
        .iter()
        .map(|agent| {
            let is_active = ps.active_agent_id.as_deref() == Some(&agent.id);
            let (active_str, active_sem) =
                if is_active { ("\u{2713}", Semantic::Success) } else { ("", Semantic::Muted) };
            let (type_str, type_sem) =
                if agent.is_builtin { ("built-in", Semantic::AccentDim) } else { ("custom", Semantic::Success) };
            vec![
                IrCell::styled(agent.id.clone(), Semantic::AccentDim),
                IrCell::text(agent.name.clone()),
                IrCell::styled(active_str.into(), active_sem),
                IrCell::styled(type_str.into(), type_sem),
                IrCell::styled(agent.description.clone(), Semantic::Muted),
            ]
        })
        .collect();
    blocks.push(Block::table(
        vec![
            ("ID", Align::Left),
            ("Name", Align::Left),
            ("Active", Align::Left),
            ("Type", Align::Left),
            ("Description", Align::Left),
        ],
        rows,
    ));
}

/// Build the skills table section.
fn skills_table(skills: &[crate::types::PromptItem], ps: &PromptState, blocks: &mut Vec<Block>) {
    if skills.is_empty() {
        return;
    }
    blocks.push(Block::Empty);
    blocks.push(Block::Line(vec![
        S::muted(" SKILLS".into()).bold(),
        S::muted(format!("  ({} available, {} loaded)", skills.len(), ps.loaded_skill_ids.len())),
    ]));
    blocks.push(Block::Empty);

    let rows: Vec<Vec<IrCell>> = skills
        .iter()
        .map(|skill| {
            let is_loaded = ps.loaded_skill_ids.contains(&skill.id);
            let (loaded_str, loaded_sem) =
                if is_loaded { ("\u{2713}", Semantic::Success) } else { ("", Semantic::Muted) };
            let (type_str, type_sem) =
                if skill.is_builtin { ("built-in", Semantic::AccentDim) } else { ("custom", Semantic::Success) };
            vec![
                IrCell::styled(skill.id.clone(), Semantic::AccentDim),
                IrCell::text(skill.name.clone()),
                IrCell::styled(loaded_str.into(), loaded_sem),
                IrCell::styled(type_str.into(), type_sem),
                IrCell::styled(skill.description.clone(), Semantic::Muted),
            ]
        })
        .collect();
    blocks.push(Block::table(
        vec![
            ("ID", Align::Left),
            ("Name", Align::Left),
            ("Loaded", Align::Left),
            ("Type", Align::Left),
            ("Description", Align::Left),
        ],
        rows,
    ));
}

/// Build the commands table section.
fn commands_table(commands: &[crate::types::PromptItem], blocks: &mut Vec<Block>) {
    if commands.is_empty() {
        return;
    }
    blocks.push(Block::Empty);
    blocks.push(Block::Line(vec![
        S::muted(" COMMANDS".into()).bold(),
        S::muted(format!("  ({} available)", commands.len())),
    ]));
    blocks.push(Block::Empty);

    let rows: Vec<Vec<IrCell>> = commands
        .iter()
        .map(|cmd| {
            let (type_str, type_sem) =
                if cmd.is_builtin { ("built-in", Semantic::AccentDim) } else { ("custom", Semantic::Success) };
            vec![
                IrCell::styled(format!("/{}", cmd.id), Semantic::Accent),
                IrCell::text(cmd.name.clone()),
                IrCell::styled(type_str.into(), type_sem),
                IrCell::styled(cmd.description.clone(), Semantic::Muted),
            ]
        })
        .collect();
    blocks.push(Block::table(
        vec![("Command", Align::Left), ("Name", Align::Left), ("Type", Align::Left), ("Description", Align::Left)],
        rows,
    ));
}
