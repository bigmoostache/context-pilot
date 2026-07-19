use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::ThreadsState;

/// Panel that renders the thread list and provides thread context to the LLM.
///
/// **Static panel**: the LLM-facing content (`panel_content`) is only updated
/// when `Read` is called. The TUI-facing `blocks()` renders live data for
/// visual display but this has no token cost.
pub(crate) struct ThreadsPanel;

impl Panel for ThreadsPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Span as S};

        let ts = ThreadsState::get(state);

        if ts.panel_content.is_empty() {
            return vec![Block::Line(vec![S::muted("  (empty — AI must call Read to populate)".into())])];
        }

        // Render the same panel_content the LLM sees, line by line
        ts.panel_content
            .lines()
            .map(|line| if line.is_empty() { Block::Empty } else { Block::Line(vec![S::new(format!("  {line}"))]) })
            .collect()
    }

    fn title(&self, _state: &State) -> String {
        "Threads".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        // Static panel: content is the pre-rendered panel_content set by Read.
        let content = ThreadsState::get(state).panel_content.clone();
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::THREADS {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Return the static panel_content — only updated by Read tool.
        let content = ThreadsState::get(state).panel_content.clone();
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::THREADS)
            .map_or(("P?", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Threads", content, last_refresh_ms)]
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
