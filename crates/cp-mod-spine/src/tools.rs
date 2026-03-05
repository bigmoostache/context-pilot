use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::SpineState;
use cp_base::cast::Safe as _;

/// Execute the `notification_mark_processed` tool
pub(crate) fn execute_mark_processed(tool: &ToolUse, state: &mut State) -> ToolResult {
    let all_ids: Vec<String> = match tool.input.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(ToString::to_string)).collect(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'ids' parameter.".to_string(), true);
        }
    };

    if all_ids.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'ids' array.".to_string(), true);
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
                let _ = SpineState::mark_notification_processed(state, id);
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

/// Execute the `spine_configure` tool — update spine auto-continuation and guard rail settings
pub(crate) fn execute_configure(tool: &ToolUse, state: &mut State) -> ToolResult {
    let mut changes: Vec<String> = Vec::new();

    // === Auto-continuation toggles ===
    if let Some(v) = tool.input.get("continue_until_todos_done").and_then(serde_json::Value::as_bool) {
        SpineState::get_mut(state).config.continue_until_todos_done = v;
        changes.push(format!("continue_until_todos_done = {v}"));
    }

    // === Guard rail limits (pass null to disable) ===
    // Zero values are rejected — they would permanently block all auto-continuation.
    if let Some(v) = tool.input.get("max_output_tokens") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_output_tokens = None;
            changes.push("max_output_tokens = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            if n == 0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_output_tokens = 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_output_tokens = Some(n.to_usize());
            changes.push(format!("max_output_tokens = {n}"));
        }
    }

    if let Some(v) = tool.input.get("max_cost") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_cost = None;
            changes.push("max_cost = disabled".to_string());
        } else if let Some(n) = v.as_f64() {
            if n <= 0.0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_cost <= 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_cost = Some(n);
            changes.push(format!("max_cost = ${n:.2}"));
        }
    }

    if let Some(v) = tool.input.get("max_stream_cost") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_stream_cost = None;
            changes.push("max_stream_cost = disabled".to_string());
        } else if let Some(n) = v.as_f64() {
            if n <= 0.0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_stream_cost <= 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_stream_cost = Some(n);
            changes.push(format!("max_stream_cost = ${n:.2}"));
        }
    }

    if let Some(v) = tool.input.get("max_duration_secs") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_duration_secs = None;
            changes.push("max_duration_secs = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            if n == 0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_duration_secs = 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_duration_secs = Some(n);
            changes.push(format!("max_duration_secs = {n}s"));
        }
    }

    if let Some(v) = tool.input.get("max_messages") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_messages = None;
            changes.push("max_messages = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            if n == 0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_messages = 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_messages = Some(n.to_usize());
            changes.push(format!("max_messages = {n}"));
        }
    }

    if let Some(v) = tool.input.get("max_auto_retries") {
        if v.is_null() {
            SpineState::get_mut(state).config.max_auto_retries = None;
            changes.push("max_auto_retries = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            if n == 0 {
                return ToolResult::new(
                    tool.id.clone(),
                    "Error: max_auto_retries = 0 would permanently block all auto-continuation. Use null to disable."
                        .to_string(),
                    true,
                );
            }
            SpineState::get_mut(state).config.max_auto_retries = Some(n.to_usize());
            changes.push(format!("max_auto_retries = {n}"));
        }
    }

    // === Reset runtime counters ===
    if tool.input.get("reset_counters").and_then(serde_json::Value::as_bool) == Some(true) {
        SpineState::get_mut(state).config.auto_continuation_count = 0;
        SpineState::get_mut(state).config.autonomous_start_ms = None;
        changes.push("reset runtime counters".to_string());
    }

    state.touch_panel(Kind::SPINE);

    if changes.is_empty() {
        ToolResult::new(
            tool.id.clone(),
            "No changes made. Pass at least one parameter to configure.".to_string(),
            false,
        )
    } else {
        ToolResult::new(
            tool.id.clone(),
            format!("Spine configured:\n{}", changes.iter().map(|c| format!("  • {c}")).collect::<Vec<_>>().join("\n")),
            false,
        )
    }
}
