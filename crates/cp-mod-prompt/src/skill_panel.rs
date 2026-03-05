use crossterm::event::KeyEvent;
use ratatui::prelude::{Line, Span, Style};

use crate::types::PromptState;
use cp_base::config::accessors::theme;
use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, estimate_tokens};
use cp_base::state::runtime::State;

/// Panel displaying a single loaded skill's content.
pub(crate) struct SkillPanel;

impl Panel for SkillPanel {
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

    fn render(&self, _frame: &mut ratatui::Frame<'_>, _state: &mut State, _area: ratatui::prelude::Rect) {}

    fn title(&self, state: &State) -> String {
        // Find the skill name from the selected context element
        let selected = state.context.get(state.selected_context);
        if let Some(ctx) = selected
            && ctx.context_type == Kind::new(Kind::SKILL)
            && let Some(skill_id) = ctx.get_meta_str("skill_prompt_id")
            && let Some(skill) = PromptState::get(state).skills.iter().find(|s| s.id == skill_id)
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
            && let Some(skill_id) = ctx.get_meta_str("skill_prompt_id")
            && let Some(skill) = PromptState::get(state).skills.iter().find(|s| s.id == skill_id)
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
            .filter(|(_, c)| c.context_type == Kind::new(Kind::SKILL))
            .filter_map(|(idx, c)| c.get_meta_str("skill_prompt_id").map(|sid| (sid.to_string(), c.id.clone(), idx)))
            .collect();

        // Collect content from PromptState first to avoid borrow conflict with state.context
        let updates: Vec<(usize, String, usize)> = {
            let ps = PromptState::get(state);
            skills
                .iter()
                .filter_map(|(skill_id, _panel_id, idx)| {
                    ps.skills.iter().find(|s| s.id == *skill_id).map(|skill| {
                        let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
                        let tokens = estimate_tokens(&content);
                        (*idx, content, tokens)
                    })
                })
                .collect()
        };

        for (idx, content, tokens) in updates {
            if let Some(ctx) = state.context.get_mut(idx) {
                ctx.cached_content = Some(content);
                ctx.token_count = tokens;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Skill panels are sent to LLM as context
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type == Kind::new(Kind::SKILL)
                && let Some(content) = &ctx.cached_content
            {
                items.push(ContextItem::new(&ctx.id, &ctx.name, content.clone(), ctx.last_refresh_ms));
            }
        }
        items
    }
}
