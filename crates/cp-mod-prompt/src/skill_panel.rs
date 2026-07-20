use crossterm::event::KeyEvent;

use crate::types::PromptType;

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
        {
            let all_skills = crate::storage::load_prompts_for(PromptType::Skill);
            if let Some(skill) = all_skills.iter().find(|s| s.id == skill_id) {
                let mut blocks = vec![
                    Block::Line(vec![
                        S::muted("Skill: ".into()),
                        S::accent(format!("[{}] {}", skill.id, skill.name)).bold(),
                    ]),
                    Block::Line(vec![S::styled(skill.description.clone(), Semantic::Code)]),
                    Block::Empty,
                ];
                blocks.extend(cp_render::markdown::to_blocks(&skill.content));
                return blocks;
            }
        }

        vec![Block::styled_text("Skill not found".into(), Semantic::Error)]
    }

    fn title(&self, state: &State) -> String {
        let selected = state.context.get(state.selected_context);
        if let Some(ctx) = selected
            && ctx.context_type == Kind::new(Kind::SKILL)
            && let Some(skill_id) = ctx.get_meta_str("skill_prompt_id")
        {
            let all_skills = crate::storage::load_prompts_for(PromptType::Skill);
            if let Some(skill) = all_skills.iter().find(|s| s.id == skill_id) {
                return format!("Skill: {}", skill.name);
            }
        }
        "Skill".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        // Collect skill panel info first
        let skills: Vec<(String, usize)> = state
            .context
            .iter()
            .enumerate()
            .filter(|entry| entry.1.context_type == Kind::new(Kind::SKILL))
            .filter_map(|(idx, c)| c.get_meta_str("skill_prompt_id").map(|sid| (sid.to_owned(), idx)))
            .collect();

        // Load all skills from disk once
        let all_skills = crate::storage::load_prompts_for(PromptType::Skill);

        // Update cached content for each loaded skill
        let updates: Vec<(usize, String, usize)> = skills
            .iter()
            .filter_map(|entry| {
                let skill_id = &entry.0;
                let idx = entry.1;
                all_skills.iter().find(|s| s.id == *skill_id).map(|skill| {
                    let content = format!("[{}] {}\n\n{}", skill.id, skill.name, skill.content);
                    let tokens = estimate_tokens(&content);
                    (idx, content, tokens)
                })
            })
            .collect();

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
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type == Kind::new(Kind::SKILL)
                && let Some(content) = ctx.cached_content.as_ref()
            {
                items.push(ContextItem::new(&ctx.id, &ctx.name, content.clone(), ctx.last_refresh_ms));
            }
        }
        items
    }
}
