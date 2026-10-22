//! Agents module — one conversation and runtime per agent.
//!
//! Holds the agent roster (a parent/child forest, see [`types::tree`]). An
//! agent's messages live in its human group and its `MY_TURN` / `THEIR_TURN`
//! status is derived from group turn state (see [`types::turn`]).
//!
//! One tool: `Send` (post a message into a group). `Read(group_id)` lives
//! in cp-coordination.

/// Incoming-message behavior for the focused agent (idle auto-read + streaming push).
pub mod incoming;
/// Panel rendering for the agent list.
mod panel;
/// Send-time checks: form-block validation and the staleness guard.
mod send;
/// Tool execution handler: `Send`.
pub mod tools;
/// Agent state types: `Agent`, `AgentMessage`, `AgentsState`, `FocusState`.
pub mod types;
/// Display-only mirror of per-agent execution state (see the module docs).
pub mod view_state;
/// Persistent watcher: fires a notification when idle + `MY_TURN` agent exists.
pub mod watcher;

use types::{AgentsState, FocusState};
use view_state::FleetExecMirror;

use serde_json::json;

use cp_base::cast::Safe as _;
use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolResult, ToolTexts, ToolUse};

/// Lazily-parsed tool descriptions loaded from the agents YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/agents.yaml")));

use self::panel::AgentsPanel;

/// Agents module: roster, per-agent human groups and derived turn status.
#[derive(Debug, Clone, Copy)]
pub struct AgentsModule;

impl Default for AgentsModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentsModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for AgentsModule {
    fn id(&self) -> &'static str {
        "agents"
    }
    fn name(&self) -> &'static str {
        "Agents"
    }
    fn description(&self) -> &'static str {
        "Agents: one conversation and runtime each"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(AgentsState::new());
        // FocusState is UI-global: "which agent the human is looking at" is one
        // singleton pointer, NOT per-agent state. set_ext_global pins it to the
        // shared map so it never rides the agent's runtime (`AgentRuntime`).
        state.set_ext_global(FocusState::new());
        // FleetExecMirror describes EVERY agent, so it too must be shared — a
        // per-agent copy would only ever hold that agent's own state. It is
        // runtime-only (rebuilt from the fleet registry on the first tick after
        // boot), hence no save/load arm.
        state.set_ext_global(FleetExecMirror::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(AgentsState::new());
        state.set_ext_global(FocusState::new());
        state.set_ext_global(FleetExecMirror::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = AgentsState::get(state);
        // `focus_file` records which agent's `states/<id>.json` holds the
        // focused context. It cannot live in that file (boot needs it to
        // choose the file), so it is written here, in the shared config, in the
        // same save as the keyed write — the two land together or the boot-side
        // fallback covers the gap.
        let focus_file = FocusState::get(state).focused_agent_id.clone().unwrap_or_default();
        // Messages are NOT serialized here: they live in the agent's human
        // group (`groups/<id>.json`, written by the save batch when dirty).
        let groups = &cp_groups::types::GroupsState::get(state).groups;
        let agents: Vec<types::persist::AgentMeta<'_>> =
            ts.agents.iter().map(|a| types::persist::AgentMeta::new(a, groups)).collect();
        json!({
            "agents": agents,
            "next_id": ts.next_id,
            "panel_content": ts.panel_content,
            "focus_file": focus_file,
        })
    }

    // Deliberately ignores `focus_file`: it is a boot-time *pointer* consumed
    // by `persistence::boot_load_config` before this module is even initialised,
    // never module state. Rehydrating it into a `FocusState` field would be
    // circular — the focus it names is what that field holds.
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let raw_agents = data.get("agents").or_else(|| data.get("threads"));
        let legacy_my_turn = raw_agents.map(types::turn::legacy_my_turn_ids).unwrap_or_default();
        let ts = AgentsState::get_mut(state);
        if let Some(arr) = raw_agents
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ts.agents = v;
        }
        // Every agent owns a human group (T860 C). GroupsModule loads first;
        // agents without one get it here, seeded from their legacy file.
        let mut agents = std::mem::take(&mut AgentsState::get_mut(state).agents);
        let _created = types::msgs::ensure_human_groups(
            &mut agents,
            cp_groups::types::GroupsState::get_mut(state),
            types::persist::read_legacy_messages,
        );
        // Tool traces belong to agents (T860 H8): load them, then move any
        // still stored in human groups by older builds.
        types::activity::load(&mut agents);
        let _moved = types::activity::migrate_out_of_groups(&mut agents, cp_groups::types::GroupsState::get_mut(state));
        // Groups left behind by agents deleted with older builds (owner gone,
        // or a coordination member gone). Skipped on an empty roster so a
        // config that failed to parse never wipes every group.
        if !agents.is_empty() {
            let removed = cp_groups::social::departed::purge(
                &mut cp_groups::types::GroupsState::get_mut(state).groups,
                &|m: &str| agents.iter().any(|a| a.id == m),
            );
            for gid in &removed {
                cp_groups::persist::remove_group_file(gid);
            }
        }
        AgentsState::get_mut(state).agents = agents;
        // Pre-T860-D `"status": "MyTurn"` becomes a SimpleTurn in the human
        // group. The field is no longer written, so this runs once.
        for id in &legacy_my_turn {
            types::turn::give(state, id);
        }
        let store = AgentsState::get_mut(state);
        if let Some(v) = data.get("next_id").and_then(serde_json::Value::as_u64) {
            store.next_id = v.to_u32();
        }
        if let Some(v) = data.get("panel_content").and_then(serde_json::Value::as_str) {
            v.clone_into(&mut store.panel_content);
        }
        // Register the persistent MY_TURN watcher. Placed here (not init_state)
        // because WatcherRegistry is created by SpineModule's init_state, which
        // may run after AgentsModule's init_state. load_module_data runs in a
        // second pass after ALL init_state calls, so the registry exists.
        watcher::ensure_idle_detector(state);
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        let fs = FocusState::get(state);
        serde_json::to_value(fs).unwrap_or(serde_json::Value::Null)
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(fs) = serde_json::from_value::<FocusState>(data.clone()) {
            // UI-global (see init_state): pin to the shared map, never per-agent.
            state.set_ext_global(fs);
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::AGENTS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::AGENTS), "Agents", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::AGENTS => Some(Box::new(AgentsPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Send", t)
                .short_desc("Post message to agent")
                .category("Agents")
                .reverie_allowed(false)
                .param("markdown", ParamType::String, true)
                .param("group_id", ParamType::String, true)
                .param("next", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        let mut pf = Verdict::new();

        if tool.name.as_str() == "Send" {
            preflight_send(tool, state, &mut pf);
        }

        if pf.errors.is_empty() && pf.warnings.is_empty() { None } else { Some(pf) }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Send" => Some(tools::execute_send(tool, state)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: Kind::AGENTS,
            icon_id: "agents",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(5),
            display_name: "agents",
            short_name: "agents",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Agents", "Agents (one conversation each) with turn-based messaging")]
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

/// Pre-flight for `Send`: the calling agent must exist, `group_id` + `next`
/// must be legal for it (see [`tools::resolve_send`]), the markdown non-empty,
/// and any agent-authored ` ```form ` block well-formed (design doc §7).
///
/// The sender is always the executing agent, so an agent can never post in
/// another agent's voice.
fn preflight_send(tool: &ToolUse, state: &State, pf: &mut Verdict) {
    let tid = tools::executing_agent_id_or_empty(state);
    let known = AgentsState::get(state).agents.iter().any(|t| t.id == tid);
    let resolved = if known {
        tools::resolve_send(tool, state, &tid).err()
    } else {
        Some(format!("Send has no owning agent (executing agent '{tid}' not found)"))
    };
    pf.errors.extend(resolved);
    let markdown = tool.input.get("markdown").and_then(|v| v.as_str());
    if markdown.is_none_or(str::is_empty) {
        pf.errors.push("Send requires a non-empty markdown".to_owned());
    }
    if let Some(md) = markdown {
        for err in send::forms::validate_form_blocks(md) {
            pf.errors.push(err);
        }
    }
}
