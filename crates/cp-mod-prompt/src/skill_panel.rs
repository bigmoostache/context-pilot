use crossterm::event::KeyEvent;

use crate::types::PromptState;

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

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let selected = state.context.get(state.selected_context);
        if let Some(ctx) = selected
            && ctx.context_type == Kind::new(Kind::SKILL)
            && let Some(skill_id) = ctx.get_meta_str("skill_prompt_id")
            && let Some(skill) = PromptState::get(state).skills.iter().find(|s| s.id == skill_id)
        {
            let mut blocks = vec![
                Block::Line(vec![
                    S::muted("Skill: ".into()),
                    S::accent(format!("[{}] {}", skill.id, skill.name)).bold(),
                ]),
                Block::Line(vec![S::styled(skill.description.clone(), Semantic::Code)]),
                Block::Empty,
            ];
            for line in skill.content.lines() {
                blocks.push(Block::text(line.to_string()));
            }
            return blocks;
        }

        vec![Block::styled_text("Skill not found".into(), Semantic::Error)]
    }
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

    fn max_freezes(&self) -> u8 {
        0
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
