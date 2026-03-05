use std::collections::HashMap;

use ratatui::prelude::{Line, Span, Style};

use cp_mod_memory::types::{MemoryImportance, MemoryState};
use cp_mod_prompt::types::PromptState;
use cp_mod_todo::types::{TodoState, TodoStatus};

use crate::modules::all_modules;
use crate::state::State;
use crate::ui::{
    helpers::{Cell, render_table},
    theme,
};

use super::render::separator;

/// Render the STATISTICS section.
pub(super) fn render_statistics(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("STATISTICS".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    // Open panels: context elements + system prompt + tool definitions
    let panel_count = state.context.len().saturating_add(2);
    let fixed_count = state.context.iter().filter(|c| c.context_type.is_fixed()).count();
    let dynamic_count = panel_count.saturating_sub(fixed_count).saturating_sub(2); // subtract system + tools

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("Panels: ".to_string(), Style::default().fg(theme::text_secondary())),
        Span::styled(format!("{panel_count}"), Style::default().fg(theme::text()).bold()),
        Span::styled(
            format!(" ({fixed_count} fixed, {dynamic_count} dynamic, 1 system, 1 tools)"),
            Style::default().fg(theme::text_muted()),
        ),
    ]));

    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let total_msgs = state.messages.len();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("Messages: ".to_string(), Style::default().fg(theme::text_secondary())),
        Span::styled(format!("{total_msgs}"), Style::default().fg(theme::text()).bold()),
        Span::styled(
            format!(" ({user_msgs} user, {assistant_msgs} assistant)"),
            Style::default().fg(theme::text_muted()),
        ),
    ]));

    let ts = TodoState::get(state);
    let total_todos = ts.todos.len();
    if total_todos > 0 {
        let done_todos = ts.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        let in_progress = ts.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let pending = total_todos.saturating_sub(done_todos).saturating_sub(in_progress);

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Todos: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(format!("{done_todos}/{total_todos}"), Style::default().fg(theme::success()).bold()),
            Span::styled(" done".to_string(), Style::default().fg(theme::text_muted())),
            Span::styled(
                format!(", {in_progress} in progress, {pending} pending"),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
    }

    let mems = &MemoryState::get(state).memories;
    let total_memories = mems.len();
    if total_memories > 0 {
        let critical = mems.iter().filter(|m| m.importance == MemoryImportance::Critical).count();
        let high = mems.iter().filter(|m| m.importance == MemoryImportance::High).count();
        let medium = mems.iter().filter(|m| m.importance == MemoryImportance::Medium).count();
        let low = mems.iter().filter(|m| m.importance == MemoryImportance::Low).count();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Memories: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(format!("{total_memories}"), Style::default().fg(theme::text()).bold()),
            Span::styled(
                format!(" ({critical} critical, {high} high, {medium} medium, {low} low)"),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
    }

    text
}

/// Render the AGENTS section (system prompts table).
pub(super) fn render_seeds(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();
    let ps = PromptState::get(state);

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("AGENTS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(format!("  ({} available)", ps.agents.len()), Style::default().fg(theme::text_muted())),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("ID", Style::default()),
        Cell::new("Name", Style::default()),
        Cell::new("Active", Style::default()),
        Cell::new("Description", Style::default()),
    ];

    let rows: Vec<Vec<Cell>> = ps
        .agents
        .iter()
        .map(|agent| {
            let is_active = ps.active_agent_id.as_deref() == Some(&agent.id);
            let (active_str, active_color) =
                if is_active { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };

            let display_name = if agent.name.len() > 20 {
                format!("{}...", &agent.name.get(..agent.name.floor_char_boundary(17)).unwrap_or(""))
            } else {
                agent.name.clone()
            };

            let display_desc = if agent.description.len() > 35 {
                format!("{}...", &agent.description.get(..agent.description.floor_char_boundary(32)).unwrap_or(""))
            } else {
                agent.description.clone()
            };

            vec![
                Cell::new(&agent.id, Style::default().fg(theme::accent_dim())),
                Cell::new(display_name, Style::default().fg(theme::text())),
                Cell::new(active_str, Style::default().fg(active_color)),
                Cell::new(display_desc, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    text.extend(render_table(&header, &rows, None, 1));

    // Skills section
    if !ps.skills.is_empty() {
        text.extend(separator());
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("SKILLS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(
                format!("  ({} available, {} loaded)", ps.skills.len(), ps.loaded_skill_ids.len()),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
        text.push(Line::from(""));

        let skill_header = [
            Cell::new("ID", Style::default()),
            Cell::new("Name", Style::default()),
            Cell::new("Loaded", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let skill_rows: Vec<Vec<Cell>> = ps
            .skills
            .iter()
            .map(|skill| {
                let is_loaded = ps.loaded_skill_ids.contains(&skill.id);
                let (loaded_str, loaded_color) =
                    if is_loaded { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };
                vec![
                    Cell::new(&skill.id, Style::default().fg(theme::accent_dim())),
                    Cell::new(skill.name.clone(), Style::default().fg(theme::text())),
                    Cell::new(loaded_str, Style::default().fg(loaded_color)),
                    Cell::new(skill.description.clone(), Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&skill_header, &skill_rows, None, 1));
    }

    // Commands section
    if !ps.commands.is_empty() {
        text.extend(separator());
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("COMMANDS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(format!("  ({} available)", ps.commands.len()), Style::default().fg(theme::text_muted())),
        ]));
        text.push(Line::from(""));

        let cmd_header = [
            Cell::new("ID", Style::default()),
            Cell::new("Name", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let cmd_rows: Vec<Vec<Cell>> = ps
            .commands
            .iter()
            .map(|cmd| {
                vec![
                    Cell::new(format!("/{}", cmd.id), Style::default().fg(theme::accent())),
                    Cell::new(cmd.name.clone(), Style::default().fg(theme::text())),
                    Cell::new(cmd.description.clone(), Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&cmd_header, &cmd_rows, None, 1));
    }

    text
}

/// Render the PRESETS section.
pub(super) fn render_presets(base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();

    let presets = cp_mod_preset::tools::list_presets_with_info();
    if presets.is_empty() {
        return text;
    }

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("PRESETS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(format!("  ({} available)", presets.len()), Style::default().fg(theme::text_muted())),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("Name", Style::default()),
        Cell::new("Type", Style::default()),
        Cell::new("Description", Style::default()),
    ];

    let rows: Vec<Vec<Cell>> = presets
        .iter()
        .map(|p| {
            let (type_label, type_color) =
                if p.built_in { ("built-in", theme::accent_dim()) } else { ("custom", theme::success()) };

            let display_name = if p.name.len() > 25 {
                format!("{}...", &p.name.get(..p.name.floor_char_boundary(22)).unwrap_or(""))
            } else {
                p.name.clone()
            };

            let display_desc = if p.description.len() > 35 {
                format!("{}...", &p.description.get(..p.description.floor_char_boundary(32)).unwrap_or(""))
            } else {
                p.description.clone()
            };

            vec![
                Cell::new(display_name, Style::default().fg(theme::text())),
                Cell::new(type_label, Style::default().fg(type_color)),
                Cell::new(display_desc, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    text.extend(render_table(&header, &rows, None, 1));

    text
}

/// Render the TOOLS section (grouped by category).
pub(super) fn render_tools(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line<'_>> = Vec::new();

    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("TOOLS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(
            format!("  ({enabled_count} enabled, {disabled_count} disabled)"),
            Style::default().fg(theme::text_muted()),
        ),
    ]));
    text.push(Line::from(""));

    // Build category descriptions from modules
    let cat_descs: HashMap<&str, &str> = all_modules().iter().flat_map(|m| m.tool_category_descriptions()).collect();

    // Collect unique categories in order of first appearance
    let mut seen_cats = std::collections::HashSet::new();
    let categories: Vec<String> =
        state.tools.iter().filter(|t| seen_cats.insert(t.category.clone())).map(|t| t.category.clone()).collect();

    for category in &categories {
        let category_tools: Vec<_> = state.tools.iter().filter(|t| t.category == *category).collect();

        if category_tools.is_empty() {
            continue;
        }

        let cat_name = category.to_uppercase();
        let cat_desc = cat_descs.get(category.as_str()).copied().unwrap_or("");

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(cat_name.clone(), Style::default().fg(theme::accent()).bold()),
            Span::styled(format!("  {cat_desc}"), Style::default().fg(theme::text_muted())),
        ]));

        let header = [
            Cell::new("Tool", Style::default()),
            Cell::new("On", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let rows: Vec<Vec<Cell>> = category_tools
            .iter()
            .map(|tool| {
                let (status_icon, status_color) =
                    if tool.enabled { ("\u{2713}", theme::success()) } else { ("\u{2717}", theme::error()) };

                vec![
                    Cell::new(&tool.id, Style::default().fg(theme::text())),
                    Cell::new(status_icon, Style::default().fg(status_color)),
                    Cell::new(&tool.short_desc, Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&header, &rows, None, 2));
        text.push(Line::from(""));
    }

    text
}
