//! Callback module — auto-fire bash scripts when files matching a glob are edited.
//!
//! Four tools: `Callback_upsert` (create/update/delete), `Callback_toggle`
//! (per-worker activation), `Callback_open_editor` / `Callback_close_editor`
//! (inline script editing). Callbacks run as child processes via the console
//! module, with optional blocking and timeout.

// Queue ID test marker — delete me later
/// Script execution: spawn callback processes, capture output, handle timeouts.
pub mod firing;
/// Callback panel: table display and editor rendering.
mod panel;
/// Tool dispatch: upsert, toggle, open/close editor.
pub mod tools;
/// Upsert tool internals: create, update, delete callback definitions.
mod tools_upsert;
/// Glob matching and callback trigger on file edits.
pub mod trigger;
/// Callback state types: `CallbackDefinition`, `CallbackState`.
pub mod types;

use serde_json::json;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::{Kind, TypeMeta};
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::CallbackPanel;
use self::types::CallbackState;
use cp_base::cast::Safe as _;

/// Lazily parsed tool texts from the callback YAML definition file.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/callback.yaml")));

/// Callback module: auto-fire bash scripts on file edits matching glob patterns.
#[derive(Debug, Clone, Copy)]
pub struct CallbackModule;

impl Module for CallbackModule {
    fn id(&self) -> &'static str {
        "callback"
    }
    fn name(&self) -> &'static str {
        "Callback"
    }
    fn description(&self) -> &'static str {
        "Auto-fire bash scripts when files are edited"
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        true
    }

    fn dependencies(&self) -> &[&'static str] {
        &["console"]
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(CallbackState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(CallbackState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let cs = CallbackState::get(state);
        json!({
            "definitions": cs.definitions,
            "next_id": cs.next_id,
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(defs) = data.get("definitions")
            && let Ok(v) = serde_json::from_value(defs.clone())
        {
            CallbackState::get_mut(state).definitions = v;
        }
        if let Some(v) = data.get("next_id").and_then(serde_json::Value::as_u64) {
            CallbackState::get_mut(state).next_id = v.to_usize();
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        let cs = CallbackState::get(state);
        let active: Vec<&String> = cs.active_set.iter().collect();
        json!({ "active_set": active, "editor_open": cs.editor_open })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("active_set")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            CallbackState::get_mut(state).active_set = v.into_iter().collect();
        }
        if let Some(v) = data.get("editor_open") {
            CallbackState::get_mut(state).editor_open = v.as_str().map(ToString::to_string);
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::CALLBACK)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::CALLBACK), "Callbacks", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::CALLBACK => Some(Box::new(CallbackPanel)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<TypeMeta> {
        vec![TypeMeta {
            context_type: "callback",
            icon_id: "spine", // Reuse spine icon (⚡) for now
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(7),
            display_name: "callback",
            short_name: "callback",
            needs_async_wait: false,
        }]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Callback_upsert", t)
                .short_desc("Create, update, or delete a callback")
                .category("Callback")
                .param_enum("action", &["create", "update", "delete"], true)
                .param("id", ParamType::String, false)
                .param("name", ParamType::String, false)
                .param("description", ParamType::String, false)
                .param("pattern", ParamType::String, false)
                .param("script_content", ParamType::String, false)
                .param("blocking", ParamType::Boolean, false)
                .param("timeout", ParamType::Integer, false)
                .param("success_message", ParamType::String, false)
                .param("cwd", ParamType::String, false)
                .param("is_global", ParamType::Boolean, false)
                .param("old_string", ParamType::String, false)
                .param("new_string", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Callback_open_editor", t)
                .short_desc("Open callback script in editor")
                .category("Callback")
                .param("id", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Callback_close_editor", t)
                .short_desc("Close callback script editor")
                .category("Callback")
                .build(),
            ToolDefinition::from_yaml("Callback_toggle", t)
                .short_desc("Activate/deactivate a callback for this worker")
                .category("Callback")
                .param("id", ParamType::String, true)
                .param("active", ParamType::Boolean, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "Callback_upsert" => {
                let mut pf = Verdict::new();
                let action = tool.input.get("action").and_then(|v| v.as_str()).unwrap_or("");
                if (action == "update" || action == "delete")
                    && let Some(id) = tool.input.get("id").and_then(|v| v.as_str())
                {
                    let cs = CallbackState::get(state);
                    if !cs.definitions.iter().any(|d| d.id == id) {
                        pf.errors.push(format!("Callback '{id}' not found"));
                    }
                }
                Some(pf)
            }
            "Callback_open_editor" => {
                let mut pf = Verdict::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    let cs = CallbackState::get(state);
                    if !cs.definitions.iter().any(|d| d.id == id) {
                        pf.errors.push(format!("Callback '{id}' not found"));
                    }
                }
                Some(pf)
            }
            "Callback_close_editor" => {
                let mut pf = Verdict::new();
                let cs = CallbackState::get(state);
                if cs.editor_open.is_none() {
                    pf.warnings.push("No callback editor is currently open".to_string());
                }
                Some(pf)
            }
            "Callback_toggle" => {
                let mut pf = Verdict::new();
                if let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) {
                    let cs = CallbackState::get(state);
                    if !cs.definitions.iter().any(|d| d.id == id) {
                        pf.errors.push(format!("Callback '{id}' not found"));
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Callback_upsert" => Some(tools::execute_upsert(tool, state)),
            "Callback_toggle" => Some(tools::execute_toggle(tool, state)),
            "Callback_open_editor" => Some(tools::execute_open_editor(tool, state)),
            "Callback_close_editor" => Some(tools::execute_close_editor(tool, state)),
            _ => None,
        }
    }

    fn context_detail(&self, ctx: &cp_base::state::context::Entry) -> Option<String> {
        (ctx.context_type.as_str() == Kind::CALLBACK).then_some("callbacks".to_string())
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Callback", "Auto-fire scripts on file edits")]
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
