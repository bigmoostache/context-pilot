//! Browser module — drive a real Chrome over CDP, embedded in Context Pilot.
//!
//! Two-channel architecture: Chrome's *lifecycle* is owned by
//! `cp-console-server` (survives TUI reloads, keeping authenticated sessions),
//! while all *control* (navigate/click/extract/screenshot) goes straight to
//! Chrome's `DevTools` WebSocket via `headless_chrome`, bypassing the daemon.
//! Heavy state (snapshot e-ref tables) lives in a paginated Browser panel,
//! never inline. See `docs/design-browser-module.md`.

/// CDP client wrapper: connect, navigate, act, extract (Channel B).
pub mod client;
/// Chrome lifecycle via the console-server daemon (Channel A).
pub mod lifecycle;
/// Browser panel: digest + paginated snapshot table.
mod panel;
/// Page snapshot: JS DOM walk → compact e-ref table.
pub mod snapshot;
/// Tool implementations: open/goto/snapshot/click/type/extract/….
pub mod tools;
/// State types: `BrowserState`, `ChromeMeta`, `Eref`.
pub mod types;

/// Context-type id for the Browser panel.
pub const BROWSER_KIND: &str = "browser";

use cp_base::cast::Safe as _;
use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use types::{BrowserState, ChromeMeta};

/// Lazily parsed tool descriptions from the browser YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/browser.yaml")));

/// Browser module: Chrome automation over CDP.
#[derive(Debug, Clone, Copy)]
pub struct BrowserModule;

impl Module for BrowserModule {
    fn id(&self) -> &'static str {
        "browser"
    }
    fn name(&self) -> &'static str {
        "Browser"
    }
    fn description(&self) -> &'static str {
        "Drive a real Chrome browser via CDP"
    }
    fn is_global(&self) -> bool {
        false
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(BrowserState::new());
    }

    fn reset_state(&self, state: &mut State) {
        lifecycle::kill_chrome(BrowserState::get_mut(state));
        state.set_ext(BrowserState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let bs = BrowserState::get(state);
        let alive = bs.handle.as_ref().is_some_and(|h| !h.get_status().is_terminal());
        if !alive && bs.next_session_id <= 1 {
            return serde_json::Value::Null;
        }
        let mut data = serde_json::json!({ "next_session_id": bs.next_session_id });
        if alive
            && let Some(meta) = bs.meta.as_ref()
            && let Ok(m) = serde_json::to_value(meta)
            && let Some(obj) = data.as_object_mut()
        {
            let _prev = obj.insert("meta".to_string(), m);
        }
        data
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("next_session_id").and_then(serde_json::Value::as_u64) {
            BrowserState::get_mut(state).next_session_id = v.to_usize();
        }
        let meta: Option<ChromeMeta> = data.get("meta").and_then(|v| serde_json::from_value(v.clone()).ok());

        // Reap orphaned browser_* daemon sessions (ours only — see lifecycle).
        let known: std::collections::HashSet<String> = meta.iter().map(|m| m.session_key.clone()).collect();
        lifecycle::cleanup_orphans(&known);

        let reconnected = meta.is_some_and(|m| {
            let ok = lifecycle::reconnect_chrome(BrowserState::get_mut(state), m);
            log::info!("browser: reconnect after reload — {}", if ok { "ok" } else { "process gone" });
            ok
        });
        if reconnected {
            panel::mark_dirty(state);
        } else {
            // No live Chrome — drop any persisted Browser panel.
            state.context.retain(|c| c.context_type.as_str() != BROWSER_KIND);
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(BROWSER_KIND)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            BROWSER_KIND => Some(Box::new(panel::BrowserPanel)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "browser",
            icon_id: "tmux", // Reuse console icon for now
            is_fixed: false,
            needs_cache: false,
            fixed_order: None,
            display_name: "browser",
            short_name: "browser",
            needs_async_wait: false,
        }]
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("browser_open", t)
                .short_desc("Launch or reuse Chrome")
                .category("Browser")
                .param("headless", ParamType::Boolean, false)
                .param("url", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("browser_goto", t)
                .short_desc("Navigate to a URL")
                .category("Browser")
                .param("url", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("browser_snapshot", t)
                .short_desc("Snapshot interactive elements")
                .category("Browser")
                .build(),
            ToolDefinition::from_yaml("browser_click", t)
                .short_desc("Click an element")
                .category("Browser")
                .param("ref", ParamType::String, false)
                .param("selector", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("browser_type", t)
                .short_desc("Type into an element")
                .category("Browser")
                .param("ref", ParamType::String, false)
                .param("selector", ParamType::String, false)
                .param("text", ParamType::String, true)
                .param("submit", ParamType::Boolean, false)
                .build(),
            ToolDefinition::from_yaml("browser_extract", t)
                .short_desc("Extract page content")
                .category("Browser")
                .param("selector", ParamType::String, false)
                .param("format", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("browser_screenshot", t)
                .short_desc("Screenshot to PNG file")
                .category("Browser")
                .param("full_page", ParamType::Boolean, false)
                .build(),
            ToolDefinition::from_yaml("browser_eval", t)
                .short_desc("Evaluate JavaScript in the page")
                .category("Browser")
                .param("expression", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("browser_close", t).short_desc("Close the browser").category("Browser").build(),
        ]
    }

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        tool.name.starts_with("browser_").then(|| tools::execute(tool, state))
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let bs = BrowserState::get(state);
        let alive = bs.handle.as_ref().is_some_and(|h| !h.get_status().is_terminal());
        let m = bs.meta.as_ref()?;
        alive.then(|| format!("Browser: Chrome running ({})\n", if m.headless { "headless" } else { "headed" }))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Browser", "Drive a real Chrome browser via CDP")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &["console"]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

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
        ctx: &cp_base::state::context::Entry,
        state: &mut State,
    ) -> Option<Result<String, String>> {
        if ctx.context_type.as_str() != BROWSER_KIND {
            return None;
        }
        // Closing the panel kills Chrome (profile stays on disk for next open).
        lifecycle::kill_chrome(BrowserState::get_mut(state));
        Some(Ok("browser: Chrome closed".to_string()))
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
