//! `CoordinationModule`: registers the `Group_*` tools. Stateless: groups
//! live in [`cp_groups::types::GroupsState`].

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolResult, ToolTexts, ToolUse};

/// Lazily-parsed tool descriptions.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/coordination.yaml")));

/// Coordination module: the `Group_*` tools.
#[derive(Debug, Clone, Copy)]
pub struct CoordinationModule;

impl Default for CoordinationModule {
    fn default() -> Self {
        Self::new()
    }
}

impl CoordinationModule {
    /// Construct the module marker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// `ParamType` of an agent-id list.
fn id_list() -> ParamType {
    ParamType::Array(Box::new(ParamType::String))
}

impl Module for CoordinationModule {
    fn id(&self) -> &'static str {
        "coordination"
    }
    fn name(&self) -> &'static str {
        "Coordination"
    }
    fn description(&self) -> &'static str {
        "Coordination: agents create and manage groups"
    }
    fn init_state(&self, _state: &mut State) {}
    fn reset_state(&self, _state: &mut State) {}
    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }
    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }
    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }
    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Group_create", t)
                .short_desc("Create a group of agents")
                .category("Coordination")
                .reverie_allowed(false)
                .param("agent_ids", id_list(), true)
                .param("name", ParamType::String, true)
                .param("description", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Read", t)
                .short_desc("Open or refresh a group panel")
                .category("Coordination")
                .reverie_allowed(false)
                .param("group_id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Group_update", t)
                .short_desc("Change a group you own")
                .category("Coordination")
                .reverie_allowed(false)
                .param("group_id", ParamType::String, true)
                .param("before_agent_ids", id_list(), true)
                .param("after_agent_ids", id_list(), false)
                .param("name", ParamType::String, false)
                .param("description", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Group_delete", t)
                .short_desc("Delete a group you own")
                .category("Coordination")
                .reverie_allowed(false)
                .param("group_id", ParamType::String, true)
                .build(),
        ]
    }
    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }
    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        let run = match tool.name.as_str() {
            "Group_create" => crate::ops::create,
            "Read" => crate::ops::read,
            "Group_update" => crate::ops::update,
            "Group_delete" => crate::ops::delete,
            _ => return None,
        };
        let mut result = match run(tool, state) {
            Ok(text) => ToolResult::new(tool.id.clone(), text, false),
            Err(e) => ToolResult::new(tool.id.clone(), e, true),
        };
        // Creating a group only adds its panel: no reason to refresh the rest.
        result.preserves_tempo = tool.name == "Group_create";
        Some(result)
    }
    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![]
    }
    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Coordination", "Create and manage groups of agents")]
    }
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        false
    }
    fn is_global(&self) -> bool {
        true
    }
    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }
    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
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
