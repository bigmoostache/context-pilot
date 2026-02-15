use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::modules::prompt::types::PromptType;
use crate::state::State;
use crate::ui::helpers::{Cell, render_table};
use crate::ui::theme;

pub struct LibraryPanel;

impl Panel for LibraryPanel {
    fn title(&self, state: &State) -> String {
        if let Some((pt, id)) = &state.library_preview {
            format!("Library: {} ({})", id, pt)
        } else {
            "Library".to_string()
        }
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // If previewing a specific prompt, show its content
        if let Some((pt, id)) = &state.library_preview {
            let items = match pt {
                PromptType::Agent => &state.agents,
                PromptType::Skill => &state.skills,
                PromptType::Command => &state.commands,
            };
            if let Some(item) = items.iter().find(|i| &i.id == id) {
                lines.push(Line::from(vec![
                    Span::styled("Preview: ", Style::default().fg(theme::text_muted())),
                    Span::styled(format!("[{}] {}", item.id, item.name), Style::default().fg(theme::accent()).bold()),
                    if item.is_builtin {
                        Span::styled(" (built-in)", Style::default().fg(theme::text_muted()))
                    } else {
                        Span::styled(" (custom)", Style::default().fg(theme::success()))
                    },
                ]));
                lines.push(Line::from(Span::styled(
                    item.description.clone(),
                    Style::default().fg(theme::text_secondary()),
                )));
                lines.push(Line::from(""));
                for line in item.content.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(theme::text()))));
                }
                return lines;
            }
        }

        // Current state summary
        let active_name = state
            .active_agent_id
            .as_ref()
            .and_then(|id| state.agents.iter().find(|a| &a.id == id))
            .map(|a| a.name.as_str())
            .unwrap_or("(none)");

        lines.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("Active Agent: ", Style::default().fg(theme::text_muted())),
            Span::styled(active_name.to_string(), Style::default().fg(theme::accent()).bold()),
        ]));

        if !state.loaded_skill_ids.is_empty() {
            let skill_names: Vec<String> = state
                .loaded_skill_ids
                .iter()
                .filter_map(|id| state.skills.iter().find(|s| &s.id == id).map(|s| s.name.clone()))
                .collect();
            lines.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled("Loaded Skills: ", Style::default().fg(theme::text_muted())),
                Span::styled(skill_names.join(", "), Style::default().fg(theme::success())),
            ]));
        }
        lines.push(Line::from(""));

        // ── AGENTS ──
        lines.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("AGENTS", Style::default().fg(theme::text_muted()).bold()),
            Span::styled(format!("  ({} available)", state.agents.len()), Style::default().fg(theme::text_muted())),
        ]));
        lines.push(Line::from(""));

        let agent_header = [
            Cell::new("ID", Style::default()),
            Cell::new("Name", Style::default()),
            Cell::new("Active", Style::default()),
            Cell::new("Type", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let agent_rows: Vec<Vec<Cell>> = state
            .agents
            .iter()
            .map(|agent| {
                let is_active = state.active_agent_id.as_deref() == Some(&agent.id);
                let (active_str, active_color) =
                    if is_active { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };
                let (type_str, type_color) =
                    if agent.is_builtin { ("built-in", theme::accent_dim()) } else { ("custom", theme::success()) };

                vec![
                    Cell::new(&agent.id, Style::default().fg(theme::accent_dim())),
                    Cell::new(agent.name.clone(), Style::default().fg(theme::text())),
                    Cell::new(active_str, Style::default().fg(active_color)),
                    Cell::new(type_str, Style::default().fg(type_color)),
                    Cell::new(agent.description.clone(), Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        lines.extend(render_table(&agent_header, &agent_rows, None, 1));

        // ── SKILLS ──
        if !state.skills.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled("SKILLS", Style::default().fg(theme::text_muted()).bold()),
                Span::styled(
                    format!("  ({} available, {} loaded)", state.skills.len(), state.loaded_skill_ids.len()),
                    Style::default().fg(theme::text_muted()),
                ),
            ]));
            lines.push(Line::from(""));

            let skill_header = [
                Cell::new("ID", Style::default()),
                Cell::new("Name", Style::default()),
                Cell::new("Loaded", Style::default()),
                Cell::new("Type", Style::default()),
                Cell::new("Description", Style::default()),
            ];

            let skill_rows: Vec<Vec<Cell>> = state
                .skills
                .iter()
                .map(|skill| {
                    let is_loaded = state.loaded_skill_ids.contains(&skill.id);
                    let (loaded_str, loaded_color) =
                        if is_loaded { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };
                    let (type_str, type_color) =
                        if skill.is_builtin { ("built-in", theme::accent_dim()) } else { ("custom", theme::success()) };

                    vec![
                        Cell::new(&skill.id, Style::default().fg(theme::accent_dim())),
                        Cell::new(skill.name.clone(), Style::default().fg(theme::text())),
                        Cell::new(loaded_str, Style::default().fg(loaded_color)),
                        Cell::new(type_str, Style::default().fg(type_color)),
                        Cell::new(skill.description.clone(), Style::default().fg(theme::text_muted())),
                    ]
                })
                .collect();

            lines.extend(render_table(&skill_header, &skill_rows, None, 1));
        }

        // ── COMMANDS ──
        if !state.commands.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled("COMMANDS", Style::default().fg(theme::text_muted()).bold()),
                Span::styled(
                    format!("  ({} available)", state.commands.len()),
                    Style::default().fg(theme::text_muted()),
                ),
            ]));
            lines.push(Line::from(""));

            let cmd_header = [
                Cell::new("Command", Style::default()),
                Cell::new("Name", Style::default()),
                Cell::new("Type", Style::default()),
                Cell::new("Description", Style::default()),
            ];

            let cmd_rows: Vec<Vec<Cell>> = state
                .commands
                .iter()
                .map(|cmd| {
                    let (type_str, type_color) =
                        if cmd.is_builtin { ("built-in", theme::accent_dim()) } else { ("custom", theme::success()) };

                    vec![
                        Cell::new(format!("/{}", cmd.id), Style::default().fg(theme::accent())),
                        Cell::new(cmd.name.clone(), Style::default().fg(theme::text())),
                        Cell::new(type_str, Style::default().fg(type_color)),
                        Cell::new(cmd.description.clone(), Style::default().fg(theme::text_muted())),
                    ]
                })
                .collect();

            lines.extend(render_table(&cmd_header, &cmd_rows, None, 1));
        }

        lines
    }

    fn refresh(&self, state: &mut State) {
        // Compute token count from context content
        let items = self.context(state);
        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == crate::state::ContextType::Library) {
            let total: usize = items.iter().map(|i| crate::state::estimate_tokens(&i.content)).sum();
            ctx.token_count = total;
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let Some(ctx) = state.context.iter().find(|c| c.context_type == crate::state::ContextType::Library) else {
            return Vec::new();
        };

        let mut content = String::new();

        // Agents table
        content.push_str("Agents (system prompts):\n\n");
        content.push_str("| ID | Name | Active | Description |\n");
        content.push_str("|------|------|--------|-------------|\n");
        for agent in &state.agents {
            let active = if state.active_agent_id.as_deref() == Some(&agent.id) { "✓" } else { "" };
            content.push_str(&format!("| {} | {} | {} | {} |\n", agent.id, agent.name, active, agent.description));
        }

        // Skills table
        if !state.skills.is_empty() {
            content.push_str("\nSkills (use skill_load/skill_unload):\n\n");
            content.push_str("| ID | Name | Loaded | Description |\n");
            content.push_str("|------|------|--------|-------------|\n");
            for skill in &state.skills {
                let loaded = if state.loaded_skill_ids.contains(&skill.id) { "✓" } else { "" };
                content.push_str(&format!("| {} | {} | {} | {} |\n", skill.id, skill.name, loaded, skill.description));
            }
        }

        // Commands table
        if !state.commands.is_empty() {
            content.push_str("\nCommands:\n\n");
            content.push_str("| Command | Name | Description |\n");
            content.push_str("|---------|------|-------------|\n");
            for cmd in &state.commands {
                content.push_str(&format!("| /{} | {} | {} |\n", cmd.id, cmd.name, cmd.description));
            }
        }

        vec![ContextItem::new(&ctx.id, "Library", content, ctx.last_refresh_ms)]
    }
}
