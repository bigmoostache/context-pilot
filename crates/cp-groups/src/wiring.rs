//! `GroupsModule`: registers [`GroupsState`] and persists group metadata.
//!
//! Metadata lives in `config.json` (`modules.groups`); messages live in
//! `groups/<id>.json` ([`crate::persist`]), written by the save batch.

use serde_json::json;

use cp_base::cast::Safe as _;
use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use crate::panels::{GROUP_KIND, GROUPS_KIND, GroupPanel, GroupsPanel};
use crate::persist::{GroupMeta, load_group_messages};
use crate::types::GroupsState;

/// Groups module: fleet-shared conversations (no tools yet).
#[derive(Debug, Clone, Copy)]
pub struct GroupsModule;

impl Default for GroupsModule {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupsModule {
    /// Construct the module marker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for GroupsModule {
    fn id(&self) -> &'static str {
        "groups"
    }
    fn name(&self) -> &'static str {
        "Groups"
    }
    fn description(&self) -> &'static str {
        "Groups: conversations between agents and the human"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(GroupsState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(GroupsState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let gs = GroupsState::get(state);
        let groups: Vec<GroupMeta<'_>> = gs.groups.iter().map(Into::into).collect();
        json!({ "groups": groups, "next_id": gs.next_id })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let gs = GroupsState::get_mut(state);
        if let Some(arr) = data.get("groups")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            gs.groups = v;
            load_group_messages(&mut gs.groups);
        }
        if let Some(v) = data.get("next_id").and_then(serde_json::Value::as_u64) {
            gs.next_id = v.to_u32();
        }
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(GROUPS_KIND)]
    }
    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(GROUPS_KIND), "Groups", false)]
    }
    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            GROUPS_KIND => Some(Box::new(GroupsPanel)),
            GROUP_KIND => Some(Box::new(GroupPanel)),
            _ => None,
        }
    }
    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![]
    }
    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }
    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }
    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        use cp_base::state::context::TypeMeta;
        vec![
            TypeMeta {
                context_type: GROUPS_KIND,
                icon_id: "groups",
                is_fixed: true,
                needs_cache: false,
                fixed_order: Some(10),
                display_name: "groups",
                short_name: "groups",
                needs_async_wait: false,
            },
            TypeMeta {
                context_type: GROUP_KIND,
                icon_id: "group",
                is_fixed: false,
                needs_cache: false,
                fixed_order: None,
                display_name: "group",
                short_name: "group",
                needs_async_wait: false,
            },
        ]
    }
    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }
    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }
    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(GROUP_KIND)]
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }
    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }
    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }
    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }
    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }
    fn on_user_message(&self, _state: &mut State) {}
    fn on_stream_stop(&self, _state: &mut State) {}
    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}
    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}
    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }
    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}
