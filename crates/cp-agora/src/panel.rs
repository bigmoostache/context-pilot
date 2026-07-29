use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

use crate::types::AgoraState;

/// Render the identity as a YAML-style string for the LLM context block.
/// Empty (never-introduced) identity yields a short placeholder.
fn format_identity_for_context(state: &State) -> String {
    let ag = AgoraState::get(state);
    if ag.identity.is_empty() {
        return "identity: (not set - use Agora_set_identity to define it)".to_owned();
    }
    let mut out = String::new();
    for (key, val) in ag.identity.pairs() {
        let _r = writeln!(out, "{key}: {val}");
    }
    out.trim_end().to_owned()
}

/// Fixed panel rendering the agent's self-identity as a YAML block.
pub(crate) struct AgoraPanel;

impl Panel for AgoraPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let ag = AgoraState::get(state);
        if ag.identity.is_empty() {
            return vec![Block::Line(vec![S::muted("  Identity not set - use Agora_set_identity.".into()).italic()])];
        }

        // One YAML-style `key: value` line per identity field.
        ag.identity
            .pairs()
            .into_iter()
            .map(|(key, val)| {
                Block::KeyValue(vec![(
                    vec![S::muted(format!("  {key}: "))],
                    vec![S::styled(val.to_owned(), Semantic::Code)],
                )])
            })
            .collect()
    }

    fn title(&self, _state: &State) -> String {
        "Agora".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let content = format_identity_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::AGORA {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = format_identity_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::AGORA)
            .map_or(("P10", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Agora", content, last_refresh_ms)]
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
}
