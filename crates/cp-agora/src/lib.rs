//! Agora module — the agent's durable self-identity.
//!
//! One tool `Agora_set_identity` writes a fixed-key value store into GLOBAL
//! state (shared across every worker, not per-worker), rendered as a YAML block
//! in the fixed Agora panel. Agent-side only: no orchestrator or frontend leg.

/// Panel rendering + context generation for the identity.
mod panel;
/// Tool execution handler for `Agora_set_identity` plus the shared
/// `set_identity` core (also driven by the bridge `SetIdentity` command).
pub mod tools;
/// Identity + `AgoraState` types.
pub mod types;

use serde_json::json;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::AgoraPanel;
use self::types::AgoraState;

/// Lazily parsed tool descriptions from the agora YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/agora.yaml")));

/// The 10 identity parameter names, in canonical order (mirrors the tool YAML
/// and the `Identity` fields 1:1).
const IDENTITY_PARAMS: [&str; 10] = [
    "identity",
    "values",
    "principles",
    "character",
    "expertise",
    "role",
    "operational_responsibilities",
    "knowledge_responsibilities",
    "organic_responsibilities",
    "direct_management",
];

/// Agora module: the agent's durable self-identity store.
#[derive(Debug, Clone, Copy)]
pub struct AgoraModule;

impl Default for AgoraModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AgoraModule {
    /// Construct the module marker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for AgoraModule {
    fn id(&self) -> &'static str {
        "agora"
    }
    fn name(&self) -> &'static str {
        "Agora"
    }
    fn description(&self) -> &'static str {
        "The agent's durable self-identity"
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(AgoraState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(AgoraState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ag = AgoraState::get(state);
        json!({ "identity": ag.identity })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ag = AgoraState::get_mut(state);
        if let Some(v) = data.get("identity")
            && let Ok(identity) = serde_json::from_value(v.clone())
        {
            ag.identity = identity;
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::AGORA)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::AGORA), "Agora", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::AGORA => Some(Box::new(AgoraPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        let mut builder = ToolDefinition::from_yaml("Agora_set_identity", t)
            .short_desc("Set the agent's self-identity")
            .category("Agora");
        for param in IDENTITY_PARAMS {
            builder = builder.param(param, ParamType::String, true);
        }
        vec![builder.build()]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Agora_set_identity" => Some(tools::execute_set_identity(tool, state)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "agora",
            icon_id: "agora",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(10),
            display_name: "agora",
            short_name: "identity",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Agora", "Set the agent's durable self-identity")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
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
