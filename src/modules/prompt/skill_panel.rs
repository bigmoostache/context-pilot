use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::state::{State, estimate_tokens};
use crate::ui::theme;

pub struct SkillPanel;

impl Panel for SkillPanel {
    fn title(&self, state: &State) -> String {
        // Find the skill name from the selected context element
        let selected = state.context.get(state.selected_context);
        if let Some(ctx) = selected
            && ctx.context_type == crate::state::ContextType::Skill
            && let Some(skill_id) = &ctx.skill_prompt_id
            && let Some(skill) = state.skills.iter().find(|s| &s.id == skill_id)
        {
            return format!("Skill: {}", skill.name);
        }
        "Skill".to_string()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Find the skill panel context element that is currently selected
        let selected = state.context.get(state.selected_context);
        if let Some(ctx) = selected
            && let Some(skill_id) = &ctx.skill_prompt_id
            && let Some(skill) = state.skills.iter().find(|s| &s.id == skill_id)
        {
            lines.push(Line::from(vec![
                Span::styled("Skill: ", Style::default().fg(theme::text_muted())),
                Span::styled(format!("[{}] {}", skill.id, skill.name), Style::default().fg(theme::accent()).bold()),
            ]));
            lines.push(Line::from(Span::styled(
                skill.description.clone(),
                Style::default().fg(theme::text_secondary()),
            )));
            lines.push(Line::from(""));
            for line in skill.content.lines() {
                lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(theme::text()))));
            }
            return lines;
        }

        lines.push(Line::from(Span::styled("Skill not found", Style::default().fg(theme::error()))));
        lines
    }

    fn refresh(&self, state: &mut State) {
        // Update cached_content from the matching PromptItem
        // We need to find all Skill panels and update them
        let skills: Vec<(String, String, usize)> = state
            .context
            .iter()
            .enumerate()
            .filter(|(_, c)| c.context_type == crate::state::ContextType::Skill)
            .filter_map(|(idx, c)| c.skill_prompt_id.as_ref().map(|sid| (sid.clone(), c.id.clone(), idx)))
            .collect();

        for (skill_id, _panel_id, idx) in skills {
            if let Some(skill) = state.skills.iter().find(|s| s.id == skill_id) {
                let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
                let tokens = estimate_tokens(&content);
                let ctx = &mut state.context[idx];
                ctx.cached_content = Some(content);
                ctx.token_count = tokens;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Skill panels are sent to LLM as context
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type == crate::state::ContextType::Skill
                && let Some(content) = &ctx.cached_content
            {
                items.push(ContextItem::new(&ctx.id, &ctx.name, content.clone(), ctx.last_refresh_ms));
            }
        }
        items
    }
}
