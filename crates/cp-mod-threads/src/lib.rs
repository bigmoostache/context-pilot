//! Threads module — parallel discussion and work topics.
//!
//! Provides structured async back-and-forth between the user and the AI
//! across multiple concurrent threads. Each thread has a turn-based status
//! (`MY_TURN` / `THEIR_TURN`) and its own message history.
//!
//! Two tools: `Send` (post message / questions to a thread) and
//! `Read` (retrieve thread messages, sets focus).

/// Panel rendering for the thread list.
mod panel;
/// Tool execution handlers: `Send` and `Read`.
pub mod tools;
/// Thread state types: `Thread`, `ThreadMessage`, `ThreadsState`, `FocusState`.
pub mod types;

use types::{FocusState, ThreadsState};

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
        state.set_ext(FocusState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(ThreadsState::new());
        state.set_ext(FocusState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = ThreadsState::get(state);
        json!({
            "threads": ts.threads,
            "next_id": ts.next_id,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        let ts = ThreadsState::get_mut(state);
        if let Some(arr) = data.get("threads")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            ts.threads = v;
        }
        if let Some(v) = data.get("next_id").and_then(serde_json::Value::as_u64) {
            ts.next_id = v.to_u32();
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        let fs = FocusState::get(state);
        serde_json::to_value(fs).unwrap_or(serde_json::Value::Null)
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Ok(fs) = serde_json::from_value::<FocusState>(data.clone()) {
            state.set_ext(fs);
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
                .param_array("questions", ParamType::Object(vec![]), false)
                .build(),
            ToolDefinition::from_yaml("Read", t)
                .short_desc("Read thread messages")
                .category("Threads")
                .reverie_allowed(false)
                .param("thread_id", ParamType::String, true)
                .param("count", ParamType::Integer, false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        let mut pf = Verdict::new();
        let tool_name = tool.name.as_str();

        // === Focus enforcement (all tools) ===
        // Exempt tools: Think (reasoning), Read (how you claim focus).
        let is_focus_exempt = matches!(tool_name, "Think" | "Read");
        let ts = ThreadsState::get(state);
        let fs = FocusState::get(state);

        // Only enforce when MY_TURN threads exist and AI is unfocused.
        if ts.has_my_turn_threads() && fs.focused_thread_id.is_none() {
            if fs.dangling_remaining > 0 && !is_focus_exempt {
                // Dangling phase — warn but allow.
                pf.warnings.push(format!(
                    "\u{26a0}\u{fe0f} Dangling phase: {} tool call(s) remaining \
                     before you must focus on a thread",
                    fs.dangling_remaining
                ));
            } else if !is_focus_exempt {
                // Dangling expired, not exempt — BLOCK.
                pf.errors.push(escalation_message(fs.escalation_level));
            }
        }

        // === Tool-specific checks ===
        match tool_name {
            "Send" => {
                // Validate thread_id exists
                if let Some(tid) = tool.input.get("thread_id").and_then(|v| v.as_str())
                    && !ts.threads.iter().any(|t| t.id == tid)
                {
                    pf.errors.push(format!("Thread '{tid}' not found"));
                }
                // Require at least one content param
                let has_markdown = tool.input.get("markdown").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
                let has_file = tool.input.get("file_path").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
                let has_questions = tool.input.get("questions").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty());
                if !has_markdown && !has_file && !has_questions {
                    pf.errors.push("Send requires at least one of: markdown, file_path, questions".to_string());
                }
            }
            "Read" => {
                // Validate thread_id exists
                if let Some(tid) = tool.input.get("thread_id").and_then(|v| v.as_str())
                    && !ts.threads.iter().any(|t| t.id == tid)
                {
                    pf.errors.push(format!("Thread '{tid}' not found"));
                }
                // Validate count is positive
                if let Some(count) = tool.input.get("count").and_then(serde_json::Value::as_i64)
                    && count < 1
                {
                    pf.errors.push("count must be a positive integer".to_string());
                }
            }
            _ => {}
        }

        if pf.errors.is_empty() && pf.warnings.is_empty() {
            None
        } else {
            Some(pf)
        }
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
    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}
    fn on_tool_complete(&self, tool_name: &str, state: &mut State) {
        // Tools exempt from dangling countdown.
        let is_countdown_exempt = matches!(tool_name, "Think" | "Queue_execute");

        let has_my_turn = ThreadsState::get(state).has_my_turn_threads();
        let fs = FocusState::get_mut(state);

        // Dangling countdown: decrement on each non-exempt tool call.
        if fs.dangling_remaining > 0 && !is_countdown_exempt {
            fs.dangling_remaining = fs.dangling_remaining.saturating_sub(1);
        }

        // Escalation: bump when dangling expired, unfocused, and MY_TURN exists.
        // This fires on exempt tools (Think) that complete while the AI
        // still hasn't focused — driving the escalation level up.
        if fs.dangling_remaining <= 0 && fs.focused_thread_id.is_none() && has_my_turn {
            fs.escalation_level = fs.escalation_level.saturating_add(1);
        }
    }
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

/// Returns the focus-enforcement message for the given escalation level.
///
/// - 0–5: polite reminder
/// - 6–15: firm instruction
/// - 16–29: aggressive demand
/// - 30+: nuclear (with level number)
fn escalation_message(level: u32) -> String {
    match level {
        0..=5 => "🧵 Please focus on an available thread using Read.".to_string(),
        6..=15 => "🧵 You MUST focus on a thread. Use Read(thread_id) now.".to_string(),
        16..=29 => "🧵 STOP. Focus on a thread immediately. Use Read(thread_id).".to_string(),
        _ => format!(
            "🧵 FOCUS. ON. A. THREAD. NOW. Read(thread_id). \
             (escalation level {level})"
        ),
    }
}
