use crossterm::event::KeyEvent;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel, paginate_content};
use crate::state::{Kind, State};
use cp_base::panels::scroll_key_action;

/// Panel for frozen conversation history chunks.
/// Content is set once at creation (via `detach_conversation_chunks`) and never refreshed.
/// The TUI shows the exact YAML the LLM reads (`cached_content`), verbatim.
pub(super) struct ConversationHistoryPanel;

impl Panel for ConversationHistoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let yaml = state
            .agent()
            .context
            .get(state.agent().selected_context)
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .and_then(|c| c.cached_content.as_ref())
            .filter(|s| !s.is_empty());
        let Some(content) = yaml else {
            return vec![cp_render::Block::Line(vec![
                cp_render::Span::muted("No conversation history.".to_owned()).italic(),
            ])];
        };
        content.lines().map(|line| cp_render::Block::Line(vec![cp_render::Span::new(line.to_owned())])).collect()
    }
    fn title(&self, state: &State) -> String {
        state
            .agent()
            .context
            .get(state.agent().selected_context)
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .map_or_else(|| "Chat History".to_owned(), |c| c.name.clone())
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .agent()
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn refresh(&self, _state: &mut State) {}

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &crate::state::Entry, _state: &State) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::Entry, _state: &State) -> bool {
        false
    }
}
