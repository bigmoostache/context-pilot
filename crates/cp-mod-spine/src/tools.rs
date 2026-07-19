use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::SpineState;

/// Execute the `notification_mark_processed` tool
pub(crate) fn execute_mark_processed(tool: &ToolUse, state: &mut State) -> ToolResult {
    let all_ids: Vec<String> = match tool.input.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'ids' parameter.".to_owned(), true);
        }
    };

    if all_ids.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'ids' array.".to_owned(), true);
    }

    let mut marked = Vec::new();
    let mut already = Vec::new();
    let mut not_found = Vec::new();

    for id in &all_ids {
        let status = SpineState::get(state)
            .notifications
            .iter()
            .find(|n| n.id == *id)
            .map(super::types::Notification::is_processed);
        match status {
            Some(true) => already.push(id.as_str()),
            Some(false) => {
                let _marked = SpineState::mark_notification_processed(state, id);
                marked.push(id.as_str());
            }
            None => not_found.push(id.as_str()),
        }
    }

    let mut parts = Vec::new();
    if !marked.is_empty() {
        parts.push(format!("Marked {} as processed", marked.join(", ")));
    }
    if !already.is_empty() {
        parts.push(format!("{} already processed", already.join(", ")));
    }
    if !not_found.is_empty() {
        parts.push(format!("{} not found", not_found.join(", ")));
    }

    ToolResult::new(tool.id.clone(), parts.join("\n"), !not_found.is_empty())
}

/// Parsed intent for one guard-rail limit field.
enum LimitAction {
    /// Key absent from the input — leave the field untouched.
    Absent,
    /// Explicit null — disable the limit.
    Disable,
    /// A positive value to set.
    Set(u64),
    /// Zero — rejected (would permanently block all auto-continuation).
    Zero,
}

/// Classify a guard-rail limit field: absent / null(disable) / zero(reject) / set.
fn read_limit(input: &serde_json::Value, key: &str) -> LimitAction {
    let Some(v) = input.get(key) else {
        return LimitAction::Absent;
    };
    if v.is_null() {
        return LimitAction::Disable;
    }
    let Some(n) = v.as_u64() else {
        return LimitAction::Absent;
    };
    if n == 0 { LimitAction::Zero } else { LimitAction::Set(n) }
}

/// Error `ToolResult` for a zero-valued limit field.
fn zero_limit_error(tool: &ToolUse, key: &str) -> ToolResult {
    ToolResult::new(
        tool.id.clone(),
        format!("Error: {key} = 0 would permanently block all auto-continuation. Use null to disable."),
        true,
    )
}

/// Apply all four guard-rail limit fields. Returns `Some(error)` on a zero value,
/// else `None` after recording any changes into `changes`.
fn apply_limits(tool: &ToolUse, state: &mut State, changes: &mut Vec<String>) -> Option<ToolResult> {
    use cp_base::cast::Safe as _;
    let input = &tool.input;

    match read_limit(input, "max_output_tokens") {
        LimitAction::Disable => {
            SpineState::get_mut(state).config.max_output_tokens = None;
            changes.push("max_output_tokens = disabled".to_owned());
        }
        LimitAction::Set(n) => {
            SpineState::get_mut(state).config.max_output_tokens = Some(n.to_usize());
            changes.push(format!("max_output_tokens = {n}"));
        }
        LimitAction::Zero => return Some(zero_limit_error(tool, "max_output_tokens")),
        LimitAction::Absent => {}
    }

    match read_limit(input, "max_duration_secs") {
        LimitAction::Disable => {
            SpineState::get_mut(state).config.max_duration_secs = None;
            changes.push("max_duration_secs = disabled".to_owned());
        }
        LimitAction::Set(n) => {
            SpineState::get_mut(state).config.max_duration_secs = Some(n);
            changes.push(format!("max_duration_secs = {n}s"));
        }
        LimitAction::Zero => return Some(zero_limit_error(tool, "max_duration_secs")),
        LimitAction::Absent => {}
    }

    match read_limit(input, "max_messages") {
        LimitAction::Disable => {
            SpineState::get_mut(state).config.max_messages = None;
            changes.push("max_messages = disabled".to_owned());
        }
        LimitAction::Set(n) => {
            SpineState::get_mut(state).config.max_messages = Some(n.to_usize());
            changes.push(format!("max_messages = {n}"));
        }
        LimitAction::Zero => return Some(zero_limit_error(tool, "max_messages")),
        LimitAction::Absent => {}
    }

    match read_limit(input, "max_auto_retries") {
        LimitAction::Disable => {
            SpineState::get_mut(state).config.max_auto_retries = None;
            changes.push("max_auto_retries = disabled".to_owned());
        }
        LimitAction::Set(n) => {
            SpineState::get_mut(state).config.max_auto_retries = Some(n.to_usize());
            changes.push(format!("max_auto_retries = {n}"));
        }
        LimitAction::Zero => return Some(zero_limit_error(tool, "max_auto_retries")),
        LimitAction::Absent => {}
    }

    None
}

/// Execute the `spine_configure` tool — update spine auto-continuation and guard rail settings
pub(crate) fn execute_configure(tool: &ToolUse, state: &mut State) -> ToolResult {
    let mut changes: Vec<String> = Vec::new();

    // === Auto-continuation toggles ===
    if let Some(v) = tool.input.get("continue_until_todos_done").and_then(serde_json::Value::as_bool) {
        SpineState::get_mut(state).config.continue_until_todos_done = v;
        changes.push(format!("continue_until_todos_done = {v}"));
    }

    // === Guard rail limits (null disables, zero rejected) ===
    if let Some(err) = apply_limits(tool, state, &mut changes) {
        return err;
    }

    // === Reset runtime counters ===
    if tool.input.get("reset_counters").and_then(serde_json::Value::as_bool) == Some(true) {
        SpineState::get_mut(state).config.auto_continuation_count = 0;
        SpineState::get_mut(state).config.autonomous_start_ms = None;
        changes.push("reset runtime counters".to_owned());
    }

    state.touch_panel(Kind::SPINE);

    if changes.is_empty() {
        ToolResult::new(tool.id.clone(), "No changes made. Pass at least one parameter to configure.".to_owned(), false)
    } else {
        ToolResult::new(
            tool.id.clone(),
            format!("Spine configured:\n{}", changes.iter().map(|c| format!("  • {c}")).collect::<Vec<_>>().join("\n")),
            false,
        )
    }
}
