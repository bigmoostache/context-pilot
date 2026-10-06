//! Threads module — parallel discussion and work topics.
//!
//! Provides structured async back-and-forth between the user and the AI
//! across multiple concurrent threads. Each thread has a turn-based status
//! (`MY_TURN` / `THEIR_TURN`) and its own message history.
//!
//! Two tools: `Send` (post a message to a thread) and
//! `Read` (retrieve thread messages, sets focus).

/// Send-time validation of agent-authored ` ```form ` blocks.
mod forms;
/// Incoming-message behavior for the focused thread (idle auto-read + streaming push).
pub mod incoming;
/// Panel rendering for the thread list.
mod panel;
/// Tool execution handlers: `Send` and `Read`.
pub mod tools;
/// Thread state types: `Thread`, `ThreadMessage`, `ThreadsState`, `FocusState`.
pub mod types;
/// Display-only mirror of per-thread execution state (see the module docs).
pub mod view_state;
/// Persistent watcher: fires a notification when idle + `MY_TURN` thread exists.
pub mod watcher;

use types::{FocusState, ThreadsState};
use view_state::FleetExecMirror;

use serde_json::json;

use cp_base::cast::Safe as _;
use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolResult, ToolTexts, ToolUse};

/// Lazily-parsed tool descriptions loaded from the threads YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/threads.yaml")));

use self::panel::ThreadsPanel;

/// Threads module: parallel discussion and work topics with turn-based focus.
#[derive(Debug, Clone, Copy)]
pub struct ThreadsModule;

impl Default for ThreadsModule {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadsModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for ThreadsModule {
    fn id(&self) -> &'static str {
        "threads"
    }
    fn name(&self) -> &'static str {
        "Threads"
    }
    fn description(&self) -> &'static str {
        "Parallel discussion and work topics"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(ThreadsState::new());
        // FocusState is UI-global: "which thread the human is looking at" is one
        // singleton pointer, NOT per-thread state. set_ext_global pins it to the
        // shared map so it never rides the resident-thread swap (`ThreadRuntime`).
        state.set_ext_global(FocusState::new());
        // FleetExecMirror describes EVERY thread, so it too must be shared — a
        // per-thread copy would only ever hold that thread's own state. It is
        // runtime-only (rebuilt from the fleet registry on the first tick after
        // boot), hence no save/load arm.
        state.set_ext_global(FleetExecMirror::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(ThreadsState::new());
        state.set_ext_global(FocusState::new());
        state.set_ext_global(FleetExecMirror::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = ThreadsState::get(state);
        // `focus_file` records which thread's `states/<id>.json` holds the
        // focused context. It cannot live in that file (boot needs it to
        // choose the file), so it is written here, in the shared config, in the
        // same save as the keyed write — the two land together or the boot-side
        // fallback covers the gap.
        let focus_file = FocusState::get(state).focused_thread_id.clone().unwrap_or_default();
        // Messages are NOT serialized here: they live in `threads/<id>.json`,
        // written by the save batch only for threads that changed.
        let threads: Vec<types::persist::ThreadMeta<'_>> = ts.threads.iter().map(Into::into).collect();
        json!({
            "threads": threads,
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
        let ts = ThreadsState::get_mut(state);
        if let Some(arr) = data.get("threads")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ts.threads = v;
            types::persist::load_thread_messages(&mut ts.threads);
        }
        if let Some(v) = data.get("next_id").and_then(serde_json::Value::as_u64) {
            ts.next_id = v.to_u32();
        }
        if let Some(v) = data.get("panel_content").and_then(serde_json::Value::as_str) {
            v.clone_into(&mut ts.panel_content);
        }
        // Register the persistent MY_TURN watcher. Placed here (not init_state)
        // because WatcherRegistry is created by SpineModule's init_state, which
        // may run after ThreadsModule's init_state. load_module_data runs in a
        // second pass after ALL init_state calls, so the registry exists.
        cp_base::state::watchers::WatcherRegistry::get_mut(state)
            .register(Box::new(watcher::IdleMyTurnDetector::new()));
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        let fs = FocusState::get(state);
        serde_json::to_value(fs).unwrap_or(serde_json::Value::Null)
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(fs) = serde_json::from_value::<FocusState>(data.clone()) {
            // UI-global (see init_state): pin to the shared map, never swapped.
            state.set_ext_global(fs);
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::THREADS)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::THREADS), "Threads", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::THREADS => Some(Box::new(ThreadsPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Send", t)
                .short_desc("Post message to thread")
                .category("Threads")
                .reverie_allowed(false)
                .param("thread_id", ParamType::String, true)
                .param("markdown", ParamType::String, false)
                .param("file_path", ParamType::String, false)
                .param("still_my_turn", ParamType::Boolean, false)
                .build(),
            ToolDefinition::from_yaml("Read", t)
                .short_desc("Refresh the Threads panel")
                .category("Threads")
                .reverie_allowed(false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        let mut pf = Verdict::new();

        if tool.name.as_str() == "Send" {
            preflight_send(tool, ThreadsState::get(state), &mut pf);
        }

        if pf.errors.is_empty() && pf.warnings.is_empty() { None } else { Some(pf) }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Send" => Some(tools::execute_send(tool, state)),
            "Read" => Some(tools::execute_read(tool, state)),
            _ => None,
        }
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: Kind::THREADS,
            icon_id: "threads",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(5),
            display_name: "threads",
            short_name: "threads",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Threads", "Parallel discussion and work topics with turn-based messaging")]
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

/// Pre-flight for `Send`: thread must exist, at least one content param, and
/// any agent-authored ` ```form ` block must be well-formed (design doc §7).
///
/// Sending to a `THEIR_TURN` thread is allowed — the AI may post follow-ups
/// without waiting; status simply stays `THEIR_TURN`.
fn preflight_send(tool: &ToolUse, ts: &ThreadsState, pf: &mut Verdict) {
    if let Some(tid) = tool.input.get("thread_id").and_then(|v| v.as_str())
        && !ts.threads.iter().any(|t| t.id == tid)
    {
        pf.errors.push(format!("Thread '{tid}' not found"));
    }
    let markdown = tool.input.get("markdown").and_then(|v| v.as_str());
    let has_markdown = markdown.is_some_and(|s| !s.is_empty());
    let has_file = tool.input.get("file_path").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
    if !has_markdown && !has_file {
        pf.errors.push("Send requires at least one of: markdown, file_path".to_owned());
    }
    if let Some(md) = markdown {
        for err in forms::validate_form_blocks(md) {
            pf.errors.push(err);
        }
    }
}
