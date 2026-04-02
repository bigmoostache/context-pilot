//! IR block builders for the Tools / Configuration panel.
//!
//! Produces `Vec<Block>` equivalents of `render_details.rs` functions
//! (`render_tools`, `render_seeds`, `render_presets`), using `cp_render`
//! types instead of ratatui. Called from `ToolsPanel::blocks()`.

use std::collections::HashSet;

use cp_render::{Align, Block, Cell, Column, Semantic, Span};

use crate::modules::all_modules;
use crate::state::State;

use cp_mod_preset::tools::list_presets_with_info;
use cp_mod_prompt::types::PromptState;

/// TOOLS section: tools grouped by category, with enable/disable status.
pub(super) fn tools_blocks(state: &State) -> Vec<Block> {
    let mut out = Vec::new();

    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

    out.push(Block::Header(vec![
        Span::styled("TOOLS".to_owned(), Semantic::Muted),
        Span::muted(format!("  ({enabled_count} enabled, {disabled_count} disabled)")),
    ]));
    out.push(Block::Empty);

    // Build category descriptions from modules
    let cat_descs: std::collections::HashMap<&str, &str> =
        all_modules().iter().flat_map(|m| m.tool_category_descriptions()).collect();

    // Collect unique categories in order of first appearance
    let mut seen_cats = HashSet::new();
    let categories: Vec<String> =
        state.tools.iter().filter(|t| seen_cats.insert(t.category.clone())).map(|t| t.category.clone()).collect();

    for category in &categories {
        let category_tools: Vec<_> = state.tools.iter().filter(|t| t.category == *category).collect();
        if category_tools.is_empty() {
            continue;
        }

        let cat_name = category.to_uppercase();
        let cat_desc = cat_descs.get(category.as_str()).copied().unwrap_or("");

        out.push(Block::line(vec![Span::accent(format!(" {cat_name}")).bold(), Span::muted(format!("  {cat_desc}"))]));

        let columns = vec![
            Column { header: "Tool".to_owned(), align: Align::Left },
            Column { header: "On".to_owned(), align: Align::Left },
            Column { header: "Description".to_owned(), align: Align::Left },
        ];

        let rows: Vec<Vec<Cell>> = category_tools
            .iter()
            .map(|tool| {
                let (status_icon, status_semantic) =
                    if tool.enabled { ("\u{2713}", Semantic::Success) } else { ("\u{2717}", Semantic::Error) };
                vec![
                    Cell::styled(tool.id.clone(), Semantic::Default),
                    Cell::styled(status_icon.to_owned(), status_semantic),
                    Cell::styled(tool.short_desc.clone(), Semantic::Muted),
                ]
            })
            .collect();

        out.push(Block::Table { columns, rows });
        out.push(Block::Empty);
    }

    out
}

/// AGENTS section: system prompts table + skills + commands.
pub(super) fn seeds_blocks(state: &State) -> Vec<Block> {
    let mut out = Vec::new();
    let ps = PromptState::get(state);

    out.push(Block::Header(vec![
        Span::styled("AGENTS".to_owned(), Semantic::Muted),
        Span::muted(format!("  ({} available)", ps.agents.len())),
    ]));
    out.push(Block::Empty);

    // Agents table
    let columns = vec![
        Column { header: "ID".to_owned(), align: Align::Left },
        Column { header: "Name".to_owned(), align: Align::Left },
        Column { header: "Active".to_owned(), align: Align::Left },
        Column { header: "Description".to_owned(), align: Align::Left },
    ];

    let rows: Vec<Vec<Cell>> = ps
        .agents
        .iter()
        .map(|agent| {
            let is_active = ps.active_agent_id.as_deref() == Some(&agent.id);
            let (active_str, active_semantic) =
                if is_active { ("\u{2713}", Semantic::Success) } else { ("", Semantic::Muted) };

            let display_name = truncate_str(&agent.name, 20);
            let display_desc = truncate_str(&agent.description, 35);

            vec![
                Cell::styled(agent.id.clone(), Semantic::AccentDim),
                Cell::styled(display_name, Semantic::Default),
                Cell::styled(active_str.to_owned(), active_semantic),
                Cell::styled(display_desc, Semantic::Muted),
            ]
        })
        .collect();

    out.push(Block::Table { columns, rows });

    // Skills section
    if !ps.skills.is_empty() {
        out.push(Block::Separator);
        out.push(Block::Header(vec![
            Span::styled("SKILLS".to_owned(), Semantic::Muted),
            Span::muted(format!("  ({} available, {} loaded)", ps.skills.len(), ps.loaded_skill_ids.len())),
        ]));
        out.push(Block::Empty);

        let skill_columns = vec![
            Column { header: "ID".to_owned(), align: Align::Left },
            Column { header: "Name".to_owned(), align: Align::Left },
            Column { header: "Loaded".to_owned(), align: Align::Left },
            Column { header: "Description".to_owned(), align: Align::Left },
        ];

        let skill_rows: Vec<Vec<Cell>> = ps
            .skills
            .iter()
            .map(|skill| {
                let is_loaded = ps.loaded_skill_ids.contains(&skill.id);
                let (loaded_str, loaded_semantic) =
                    if is_loaded { ("\u{2713}", Semantic::Success) } else { ("", Semantic::Muted) };
                vec![
                    Cell::styled(skill.id.clone(), Semantic::AccentDim),
                    Cell::styled(skill.name.clone(), Semantic::Default),
                    Cell::styled(loaded_str.to_owned(), loaded_semantic),
                    Cell::styled(skill.description.clone(), Semantic::Muted),
                ]
            })
            .collect();

        out.push(Block::Table { columns: skill_columns, rows: skill_rows });
    }

    // Commands section
    if !ps.commands.is_empty() {
        out.push(Block::Separator);
        out.push(Block::Header(vec![
            Span::styled("COMMANDS".to_owned(), Semantic::Muted),
            Span::muted(format!("  ({} available)", ps.commands.len())),
        ]));
        out.push(Block::Empty);

        let cmd_columns = vec![
            Column { header: "ID".to_owned(), align: Align::Left },
            Column { header: "Name".to_owned(), align: Align::Left },
            Column { header: "Description".to_owned(), align: Align::Left },
        ];

        let cmd_rows: Vec<Vec<Cell>> = ps
            .commands
            .iter()
            .map(|cmd| {
                vec![
                    Cell::styled(format!("/{}", cmd.id), Semantic::Accent),
                    Cell::styled(cmd.name.clone(), Semantic::Default),
                    Cell::styled(cmd.description.clone(), Semantic::Muted),
                ]
            })
            .collect();

        out.push(Block::Table { columns: cmd_columns, rows: cmd_rows });
    }

    out
}

/// PRESETS section: available presets table.
pub(super) fn presets_blocks() -> Vec<Block> {
    let mut out = Vec::new();

    let presets = list_presets_with_info();
    if presets.is_empty() {
        return out;
    }

    out.push(Block::Header(vec![
        Span::styled("PRESETS".to_owned(), Semantic::Muted),
        Span::muted(format!("  ({} available)", presets.len())),
    ]));
    out.push(Block::Empty);

    let columns = vec![
        Column { header: "Name".to_owned(), align: Align::Left },
        Column { header: "Type".to_owned(), align: Align::Left },
        Column { header: "Description".to_owned(), align: Align::Left },
    ];

    let rows: Vec<Vec<Cell>> = presets
        .iter()
        .map(|p| {
            let (type_label, type_semantic) =
                if p.built_in { ("built-in", Semantic::AccentDim) } else { ("custom", Semantic::Success) };

            let display_name = truncate_str(&p.name, 25);
            let display_desc = truncate_str(&p.description, 35);

            vec![
                Cell::styled(display_name, Semantic::Default),
                Cell::styled(type_label.to_owned(), type_semantic),
                Cell::styled(display_desc, Semantic::Muted),
            ]
        })
        .collect();

    out.push(Block::Table { columns, rows });

    out
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        let boundary = s.floor_char_boundary(max_len.saturating_sub(3));
        format!("{}...", s.get(..boundary).unwrap_or(""))
    } else {
        s.to_owned()
    }
}
