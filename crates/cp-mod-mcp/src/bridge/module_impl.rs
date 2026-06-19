//! [`Module`] trait implementation for [`McpModule`].
//!
//! Separated from `mod.rs` for file-size hygiene. The trait impl wires MCP
//! into the Context Pilot module system: state lifecycle, tool dispatch, panel
//! creation, and overview rendering.

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use super::panel::{MCP_KIND, McpPanel};
use super::servers::McpState;
use super::setup::McpSetupState;
use super::{McpModule, tools};

impl Module for McpModule {
    fn id(&self) -> &'static str {
        "mcp"
    }
    fn name(&self) -> &'static str {
        "MCP"
    }
    fn description(&self) -> &'static str {
        "Connect to MCP servers and expose their tools"
    }

    fn init_state(&self, state: &mut State) {
        let mut mcp = McpState::default();
        Self::connect_all(&mut mcp);
        state.set_ext(mcp);
        state.set_ext(McpSetupState::default());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(McpState::default());
        state.set_ext(McpSetupState::default());
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // All MCP tools are runtime-discovered — none are static.
        vec![]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        // Own any namespaced tool whose server we know about.
        let (server, _rest) = tools::split_id(&tool.name)?;
        if !McpState::get(state).servers.contains_key(server) {
            return None;
        }
        Some(Self::dispatch(tool, state))
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        (context_type.as_str() == MCP_KIND).then(|| {
            let panel: Box<dyn Panel> = Box::new(McpPanel);
            panel
        })
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(MCP_KIND)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(MCP_KIND), "MCP", false)]
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: MCP_KIND,
            icon_id: "tmux",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(20),
            display_name: "mcp",
            short_name: "mcp",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("MCP", "Tools exposed by connected MCP servers")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        false
    }

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<cp_base::tools::pre_flight::Verdict> {
        None
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        use std::fmt::Write as _;

        let mcp = McpState::get(state);
        if mcp.servers.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        for name in mcp.sorted_names() {
            let Some(entry) = mcp.servers.get(&name) else { continue };
            if entry.status.is_connected() {
                parts.push(format!("{name} ({} tools)", entry.tools.len()));
            } else {
                parts.push(format!("{name} ({})", entry.status.label()));
            }
        }

        let total = mcp.total_tools();
        let mut out = String::new();
        let _r = write!(
            out,
            "MCP servers: {}. {total} tools available but disabled — see MCP panel for catalog, use tool_manage to enable.",
            parts.join(", "),
        );
        Some(out)
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
