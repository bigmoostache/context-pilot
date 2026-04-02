//! Spine module — auto-continuation, notifications, guard rails, and scheduled reminders.
//!
//! Three tools: `notification_mark_processed`, `spine_configure`, and `coucou`
//! (timer/datetime scheduling). Drives the autonomous continuation loop and
//! manages guard rails (max tokens, cost, duration, messages, retries).

pub(crate) mod coucou;
/// Auto-continuation engine: `should_auto_continue()`, message injection, guard rail checks.
pub mod engine;
/// Guard rail implementations: safety limits for auto-continuation.
pub(crate) mod guard_rail;
/// Spine panel: notification display and context rendering.
mod panel;
/// Tool execution: `notification_mark_processed`, `spine_configure`.
pub(crate) mod tools;
/// Notification, spine config, and state types.
pub mod types;

use types::{Notification, SpineState};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::SpinePanel;
use cp_base::cast::Safe as _;
use cp_base::modules::Module;

/// Lazily-parsed tool text definitions for spine tools.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/spine.yaml")));

/// Spine module: auto-continuation, notifications, guard rails, coucou timers.
#[derive(Debug, Clone, Copy)]
pub struct SpineModule;

impl Module for SpineModule {
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
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

    fn id(&self) -> &'static str {
        "spine"
    }
    fn name(&self) -> &'static str {
        "Spine"
    }
    fn description(&self) -> &'static str {
        "Unified auto-continuation and stream control"
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(SpineState::new());
        // Initialize the watcher registry (cross-cutting concern managed by spine)
        state.set_ext(cp_base::state::watchers::WatcherRegistry::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(SpineState::new());
        state.set_ext(cp_base::state::watchers::WatcherRegistry::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ss = SpineState::get(state);
        // Prune old processed notifications: keep all unprocessed + latest 10 processed
        let mut to_save: Vec<_> = ss.notifications.iter().filter(|n| !n.is_processed()).cloned().collect();
        let mut processed: Vec<_> = ss.notifications.iter().filter(|n| n.is_processed()).cloned().collect();
        // Keep only the latest 10 processed (they're in chronological order)
        if processed.len() > 10 {
            processed = processed.split_off(processed.len().saturating_sub(10));
        }
        to_save.extend(processed);
        // Sort by ID number to maintain order
        to_save.sort_by_key(|n| n.id.trim_start_matches('N').parse::<usize>().unwrap_or(0));

        // Collect pending coucou watchers for persistence
        let pending_coucous = coucou::collect_pending_coucous(state);

        json!({
            "notifications": to_save,
            "next_notification_id": ss.next_notification_id,
            "spine_config": ss.config,
            "pending_coucous": pending_coucous,
        })
    }
    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("notifications")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            SpineState::get_mut(state).notifications = v;
        }
        if let Some(v) = data.get("next_notification_id").and_then(serde_json::Value::as_u64) {
            SpineState::get_mut(state).next_notification_id = v.to_usize();
        }
        if let Some(cfg) = data.get("spine_config")
            && let Ok(v) = serde_json::from_value(cfg.clone())
        {
            SpineState::get_mut(state).config = v;
        }
        // Prune stale processed notifications on load too
        prune_notifications(&mut SpineState::get_mut(state).notifications);

        // Restore pending coucou watchers into the WatcherRegistry
        if let Some(coucous) = data.get("pending_coucous")
            && let Ok(coucou_list) = serde_json::from_value::<Vec<coucou::CoucouData>>(coucous.clone())
        {
            let registry = cp_base::state::watchers::WatcherRegistry::get_mut(state);
            for cd in coucou_list {
                // Register all coucous — expired ones will fire on next poll_all
                // and create a notification, which is the desired behavior
                registry.register(Box::new(cd.into_watcher()));
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::SPINE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::SPINE), "Spine", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::SPINE => Some(Box::new(SpinePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("notification_mark_processed", t)
                .short_desc("Mark notification as handled")
                .category("Spine")
                .param_array("ids", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("spine_configure", t)
                .short_desc("Configure auto-continuation and guard rails")
                .category("Spine")
                .param("continue_until_todos_done", ParamType::Boolean, false)
                .param("max_output_tokens", ParamType::Integer, false)
                .param("max_cost", ParamType::Number, false)
                .param("max_duration_secs", ParamType::Integer, false)
                .param("max_messages", ParamType::Integer, false)
                .param("max_auto_retries", ParamType::Integer, false)
                .param("reset_counters", ParamType::Boolean, false)
                .build(),
            ToolDefinition::from_yaml("coucou", t)
                .short_desc("Schedule a reminder notification")
                .category("Spine")
                .param("mode", ParamType::String, true)
                .param("message", ParamType::String, true)
                .param("delay", ParamType::String, false)
                .param("datetime", ParamType::String, false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "notification_mark_processed" => {
                let mut pf = Verdict::new();
                if let Some(ids) = tool.input.get("ids").and_then(|v| v.as_array()) {
                    let ss = SpineState::get(state);
                    for id_val in ids {
                        if let Some(id) = id_val.as_str()
                            && !ss.notifications.iter().any(|n| n.id == id)
                        {
                            pf.warnings.push(format!("Notification '{id}' not found"));
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "notification_mark_processed" => Some(tools::execute_mark_processed(tool, state)),
            "spine_configure" => Some(tools::execute_configure(tool, state)),
            "coucou" => Some(coucou::execute_coucou(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("notification_mark_processed", visualize_spine_output), ("spine_configure", visualize_spine_output)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "spine",
            icon_id: "spine",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(5),
            display_name: "spine",
            short_name: "spine",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Spine", "Auto-continuation and stream control")]
    }

    fn on_user_message(&self, state: &mut State) {
        // Human input resets auto-continuation counters — human is back in the loop
        let ss = SpineState::get_mut(state);
        ss.config.auto_continuation_count = 0;
        ss.config.autonomous_start_ms = None;
        ss.config.user_stopped = false;
        // Reset error backoff — human can immediately trigger a new stream
        ss.config.consecutive_continuation_errors = 0;
        ss.config.last_continuation_error_ms = None;
    }

    fn on_stream_stop(&self, state: &mut State) {
        // User pressed Esc — prevent spine from immediately relaunching
        SpineState::get_mut(state).config.user_stopped = true;
    }

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
}

/// Visualizer for spine tool results.
/// Shows configuration changes with before/after values and highlights notification IDs.
fn visualize_spine_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.starts_with("Marked") {
                Semantic::Success
            } else if line.starts_with("Updated") || line.contains("→") || line.contains('=') || line.contains(':') {
                Semantic::Info
            } else if line.starts_with('N') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
                Semantic::Warning
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}

/// Prune processed notifications: keep all unprocessed + latest 10 processed.
fn prune_notifications(notifications: &mut Vec<Notification>) {
    let processed_count = notifications.iter().filter(|n| n.is_processed()).count();
    if processed_count <= 10 {
        return;
    }
    let mut processed_seen = 0_usize;
    let drop_count = processed_count.saturating_sub(10);
    notifications.retain(|n| {
        if n.is_processed() {
            processed_seen = processed_seen.saturating_add(1);
            processed_seen > drop_count
        } else {
            true
        }
    });
}
