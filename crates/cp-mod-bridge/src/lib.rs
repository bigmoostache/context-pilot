//! Agent-side orchestration bridge — the **entire** agent-side footprint.
//!
//! This module is **behaviorally inert when the bridge is OFF** (the default):
//! the agent reasons and acts identically whether or not this module is
//! registered.  When ON, it owns:
//!
//! - **Oplog** — append-only, `fdatasync`'d WAL for durable command effects,
//!   `rev` assignment, `seen`-marks, phase transitions, lifecycle, and cost.
//! - **Stream tee** — lock-free SPSC enqueue of each `StreamEvent`; dedicated
//!   publisher thread writes `stream.sock`.
//! - **Command intake** — bearer-auth'd, journal-then-ack, injected via the
//!   existing user-message entry (never the spine).
//! - **Heartbeat** — dedicated thread, aligned in-place write.
//!
//! The module does **not** touch `writer.rs` — tier-② persistence is unchanged.
//! See `docs/design-orchestration-backend.md` §11 / §24.

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::data::model_helpers::ModelPricing as _;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use crate::boot::Boot;

pub mod body;
pub mod boot;
pub mod command;
pub mod error;
pub mod heartbeat;
pub mod register;
pub mod tee;

/// Runtime state for the bridge (stored in [`State`]'s `TypeMap`).
///
/// Not serializable — [`Boot`] holds OS resources (folder lock, stream socket,
/// oplog commit thread, heartbeat beacon) that are created fresh each session.
/// `save_module_data` / `load_module_data` return `Null`.
#[derive(Debug)]
pub struct BridgeState {
    /// The held boot resources, or `None` when the bridge is OFF or boot failed.
    pub boot: Option<Boot>,
}

/// Agent-side orchestration bridge module.
///
/// Inert when `CP_BRIDGE` is unset or not `"1"`.  When active, [`init_state`]
/// acquires the folder lock, opens the oplog, binds the stream socket, starts
/// the heartbeat beacon, and writes a registry record so the backend discovers
/// this agent.
///
/// [`init_state`]: Module::init_state
#[derive(Clone, Copy, Debug)]
pub struct BridgeModule;

impl Module for BridgeModule {
    fn id(&self) -> &'static str {
        "bridge"
    }

    fn name(&self) -> &'static str {
        "Bridge"
    }

    fn description(&self) -> &'static str {
        "Agent-side orchestration bridge (oplog, stream tee, command intake)"
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

    fn init_state(&self, state: &mut State) {
        let active = std::env::var("CP_BRIDGE").as_deref() == Ok("1");
        if !active {
            state.set_ext(BridgeState { boot: None });
            return;
        }

        let folder = match std::env::current_dir() {
            Ok(f) => f,
            Err(e) => {
                log::error!("bridge: cannot determine working directory: {e}");
                state.set_ext(BridgeState { boot: None });
                return;
            }
        };

        let model = state.current_model();

        match Boot::start(&folder, &model) {
            Ok(boot) => {
                log::info!(
                    "bridge: activated for {} ({})",
                    boot.id(),
                    folder.display(),
                );
                state.set_ext(BridgeState { boot: Some(boot) });
            }
            Err(e) => {
                log::error!("bridge: boot failed: {e:?}");
                state.set_ext(BridgeState { boot: None });
            }
        }
    }

    fn reset_state(&self, _state: &mut State) {}

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // No tools exposed to the LLM — the bridge is infrastructure,
        // not an interactive surface.
        Vec::new()
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
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

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_module_identity() {
        let m = BridgeModule;
        assert_eq!(m.id(), "bridge");
        assert_eq!(m.name(), "Bridge");
        assert!(!m.is_core());
        assert!(m.is_global());
    }

    #[test]
    fn bridge_module_no_tools() {
        let m = BridgeModule;
        assert!(m.tool_definitions().is_empty());
    }

    #[test]
    fn bridge_state_default_is_none() {
        let bs = BridgeState { boot: None };
        assert!(bs.boot.is_none());
    }
}
