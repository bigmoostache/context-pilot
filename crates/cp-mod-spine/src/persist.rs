//! Global (fleet-shared) module owning the [`CoucouRegistry`].
//!
//! Split from [`SpineModule`](crate::SpineModule), which is per-thread: the
//! registry must persist ONCE in the shared config, not in every thread's
//! worker file. No tools or panels — the `coucou` tool stays on the spine
//! module and writes here through the shared `State` extension.

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use crate::schedule::CoucouRegistry;

/// Persists the fleet-shared coucou registry.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoucouModule;

impl CoucouModule {
    /// Construct the module marker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for CoucouModule {
    fn id(&self) -> &'static str {
        "coucou"
    }

    fn name(&self) -> &'static str {
        "Coucou"
    }

    fn description(&self) -> &'static str {
        "Fleet-shared scheduled reminders"
    }

    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext_global(CoucouRegistry::default());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext_global(CoucouRegistry::default());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        serde_json::to_value(CoucouRegistry::get(state)).unwrap_or(serde_json::Value::Null)
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let mut registry: CoucouRegistry = serde_json::from_value(data.clone()).unwrap_or_default();
        registry.reseed_counter();
        state.set_ext_global(registry);
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![]
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }
}
