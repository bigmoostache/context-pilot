use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::TodoState;
use cp_base::panels::scroll_key_action;

/// Panel that renders the focused thread's task tree as canonical YAML.
pub(crate) struct TodoPanel;

impl TodoPanel {
    /// Render the focused thread's tasks as canonical YAML for LLM context.
    ///
    /// Identical to what the model edits with the `Todo` tool (via `{prev,new}`
    /// diffs) — one rigorous, byte-stable projection shared by panel + tool.
    fn format_todos_for_context(state: &State) -> String {
        let ts = TodoState::get(state);
        let Some(focus) = ts.focus_filter.as_deref() else {
            return "No focused thread".to_owned();
        };
        let yaml = crate::yaml::render(state, focus);
        if yaml.trim().is_empty() { "No tasks".to_owned() } else { yaml.trim_end().to_owned() }
    }
}

impl Panel for TodoPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};
        let ts = TodoState::get(state);
        let Some(focus) = ts.focus_filter.as_deref() else {
            return vec![Block::Line(vec![S::muted("  No focused thread".into()).italic()])];
        };
        let yaml = crate::yaml::render(state, focus);
        if yaml.trim().is_empty() {
            return vec![Block::Line(vec![S::muted("  No tasks".into()).italic()])];
        }
        // Render each canonical-YAML line verbatim as a code-styled row so the
        // panel mirrors exactly what the `Todo` tool edits.
        yaml.lines()
            .map(|line| Block::Line(vec![S::new(" ".into()), S::styled(line.to_owned(), Semantic::Code)]))
            .collect()
    }
    fn title(&self, _state: &State) -> String {
        "Todo".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let todo_content = Self::format_todos_for_context(state);
        let token_count = estimate_tokens(&todo_content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::TODO {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &todo_content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        5
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_todos_for_context(state);
        // Find the Todo context element to get its ID and timestamp
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::TODO)
            .map_or(("P3", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Todo List", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}
