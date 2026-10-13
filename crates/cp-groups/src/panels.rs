//! `groups` (fixed) and `group` (dynamic, one per open group) panels.
//!
//! Both are per-agent views: they render for the **executing** agent, whose
//! runtime owns the panel entries.

use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, estimate_tokens, make_default_entry};
use cp_base::state::runtime::State;

use crate::render::{group_body, group_line};
use crate::types::GroupsState;

/// Fixed panel kind listing the executing agent's groups.
pub const GROUPS_KIND: &str = "groups";
/// Dynamic panel kind showing one group's conversation.
pub const GROUP_KIND: &str = "group";
/// `Entry::metadata` key holding a `group` panel's group id.
pub const META_GROUP_ID: &str = "group_id";

/// `groups` panel body for the executing agent.
fn groups_content(state: &State) -> String {
    let me = state.executing_agent_id().unwrap_or_default();
    let lines: Vec<String> = GroupsState::get(state).of_member(me).map(group_line).collect();
    if lines.is_empty() {
        return "groups: []\n".to_owned();
    }
    format!("groups:\n{}\n", lines.join("\n"))
}

/// Group id a `group` panel entry shows.
fn entry_group_id(ctx: &Entry) -> Option<&str> {
    ctx.metadata.get(META_GROUP_ID)?.as_str()
}

/// `group` panel body for `gid` (a placeholder when the group is gone).
fn group_content(state: &State, gid: &str) -> String {
    GroupsState::get(state).find(gid).map_or_else(
        || format!("group_id: {gid}\nerror: group not found\n"),
        |g| group_body(g, cp_base::panels::now_ms()),
    )
}

/// Text lines as indented render blocks.
fn text_blocks(content: &str) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span as S};
    content
        .lines()
        .map(|line| if line.is_empty() { Block::Empty } else { Block::Line(vec![S::new(format!("  {line}"))]) })
        .collect()
}

/// Open (or re-select) the `group` panel for `gid` in the executing agent's
/// context; returns its panel id. Idempotent: one panel per group.
pub fn open_group_panel(state: &mut State, gid: &str) -> String {
    if let Some(ctx) =
        state.agent().context.iter().find(|c| c.context_type.as_str() == GROUP_KIND && entry_group_id(c) == Some(gid))
    {
        return ctx.id.clone();
    }
    let name = GroupsState::get(state).find(gid).map_or_else(|| gid.to_owned(), |g| format!("{gid} {}", g.name));
    let content = group_content(state, gid);
    let panel_id = state.next_available_context_id();
    let mut elem = make_default_entry(&panel_id, Kind::new(GROUP_KIND), &name, false);
    elem.uid = Some(format!("UID_{}_P", state.global_next_uid));
    state.global_next_uid = state.global_next_uid.saturating_add(1);
    drop(elem.metadata.insert(META_GROUP_ID.to_owned(), serde_json::Value::String(gid.to_owned())));
    elem.token_count = estimate_tokens(&content);
    let _changed = cp_base::panels::update_if_changed(&mut elem, &content);
    state.agent_mut().context.push(elem);
    panel_id
}

/// Fixed `groups` panel.
#[derive(Debug, Clone, Copy)]
pub struct GroupsPanel;

/// Dynamic `group` panel.
#[derive(Debug, Clone, Copy)]
pub struct GroupPanel;

impl Panel for GroupsPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        text_blocks(&groups_content(state))
    }
    fn title(&self, _state: &State) -> String {
        "Groups".to_owned()
    }
    fn refresh(&self, state: &mut State) {
        let content = groups_content(state);
        let token_count = estimate_tokens(&content);
        if let Some(ctx) = state.agent_mut().context.iter_mut().find(|c| c.context_type.as_str() == GROUPS_KIND) {
            ctx.token_count = token_count;
            let _changed = cp_base::panels::update_if_changed(ctx, &content);
        }
    }
    fn max_freezes(&self) -> u8 {
        3
    }
    fn context(&self, state: &State) -> Vec<ContextItem> {
        let (id, last) = state
            .agent()
            .context
            .iter()
            .find(|c| c.context_type.as_str() == GROUPS_KIND)
            .map_or(("P?", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Groups", groups_content(state), last)]
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

impl Panel for GroupPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        let agent = state.agent();
        let gid = agent
            .context
            .get(agent.selected_context)
            .filter(|c| c.context_type.as_str() == GROUP_KIND)
            .and_then(entry_group_id);
        gid.map_or_else(|| text_blocks("No group panel selected"), |g| text_blocks(&group_content(state, g)))
    }
    fn title(&self, state: &State) -> String {
        let agent = state.agent();
        agent.context.get(agent.selected_context).map_or_else(|| "Group".to_owned(), |c| c.name.clone())
    }
    fn refresh(&self, state: &mut State) {
        let updates: Vec<(String, String)> = state
            .agent()
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == GROUP_KIND)
            .filter_map(|c| Some((c.id.clone(), group_content(state, entry_group_id(c)?))))
            .collect();
        for (id, content) in updates {
            if let Some(ctx) = state.agent_mut().context.iter_mut().find(|c| c.id == id) {
                ctx.token_count = estimate_tokens(&content);
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
            }
        }
    }
    fn max_freezes(&self) -> u8 {
        5
    }
    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .agent()
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == GROUP_KIND)
            .filter_map(|c| {
                let body = group_content(state, entry_group_id(c)?);
                Some(ContextItem::new(&c.id, &c.name, body, c.last_refresh_ms))
            })
            .collect()
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
