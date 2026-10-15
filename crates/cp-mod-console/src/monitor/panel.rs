//! Fixed `Monitor` panel: alive monitors plus the last ended ones, as YAML.

use std::sync::PoisonError;

use cp_base::panels::{ContextItem, Panel};
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use serde::Serialize;

use super::spec::Record;
use crate::types::ConsoleState;

/// Header shown above the YAML.
const HEADER: &str = "# To terminate a monitor early, close its console panel (Close_panel on its panel_id).\n\
# Monitors survive TUI reloads (resumed where they stopped).\n";

/// YAML shape of the panel body.
#[derive(Serialize)]
struct View {
    /// Monitors still watching.
    alive: Vec<Record>,
    /// Last ended monitors, most recent first.
    ended: Vec<Record>,
}

/// Panel listing this agent's monitors.
pub(crate) struct MonitorPanel;

impl MonitorPanel {
    /// Panel text, shared by `refresh`, `context` and `blocks`.
    fn text(state: &State) -> String {
        let (alive, mut ended): (Vec<Record>, Vec<Record>) = ConsoleState::get(state)
            .monitors
            .iter()
            .map(|m| m.lock().unwrap_or_else(PoisonError::into_inner).clone())
            .partition(Record::is_alive);
        if alive.is_empty() && ended.is_empty() {
            return format!("{HEADER}No monitors.\n");
        }
        ended.sort_by_key(|r| std::cmp::Reverse(r.ended_at_ms().unwrap_or(0)));
        let yaml = serde_yaml::to_string(&View { alive, ended }).unwrap_or_else(|e| format!("# render failed: {e}\n"));
        format!("{HEADER}{yaml}")
    }
}

impl Panel for MonitorPanel {
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Span as S};
        Self::text(state).lines().map(|l| Block::Line(vec![S::muted(format!("  {l}"))])).collect()
    }

    fn title(&self, state: &State) -> String {
        let alive = ConsoleState::get(state)
            .monitors
            .iter()
            .filter(|m| m.lock().unwrap_or_else(PoisonError::into_inner).is_alive())
            .count();
        if alive == 0 { "Monitor".to_owned() } else { format!("Monitor ({alive})") }
    }

    fn refresh(&self, state: &mut State) {
        ConsoleState::get_mut(state).prune_monitors();
        let content = Self::text(state);
        let token_count = estimate_tokens(&content);
        if let Some(ctx) = state.agent_mut().context.iter_mut().find(|c| c.context_type.as_str() == Kind::MONITOR) {
            ctx.token_count = token_count;
            let _changed = cp_base::panels::update_if_changed(ctx, &content);
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::text(state);
        let (id, last_refresh_ms) = state
            .agent()
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::MONITOR)
            .map_or(("", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Monitor", content, last_refresh_ms)]
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
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
