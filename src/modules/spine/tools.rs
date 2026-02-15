use crate::state::{ContextType, State};
use crate::tools::{ToolResult, ToolUse};

/// Execute the notification_mark_processed tool
pub fn execute_mark_processed(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required 'id' parameter".to_string(),
                is_error: true,
            };
        }
    };

    // Check if notification exists and its current state
    let already_processed = state.notifications.iter().find(|n| n.id == id).map(|n| n.processed);

    match already_processed {
        Some(true) => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Notification {} is already processed", id),
            is_error: false,
        },
        Some(false) => {
            state.mark_notification_processed(id);
            ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Marked notification {} as processed", id),
                is_error: false,
            }
        }
        None => ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Notification '{}' not found", id),
            is_error: true,
        },
    }
}

/// Execute the spine_configure tool — update spine auto-continuation and guard rail settings
pub fn execute_configure(tool: &ToolUse, state: &mut State) -> ToolResult {
    let mut changes: Vec<String> = Vec::new();

    // === Auto-continuation toggles ===
    if let Some(v) = tool.input.get("max_tokens_auto_continue").and_then(|v| v.as_bool()) {
        state.spine_config.max_tokens_auto_continue = v;
        changes.push(format!("max_tokens_auto_continue = {}", v));
    }

    if let Some(v) = tool.input.get("continue_until_todos_done").and_then(|v| v.as_bool()) {
        state.spine_config.continue_until_todos_done = v;
        changes.push(format!("continue_until_todos_done = {}", v));
    }

    // === Guard rail limits (pass null to disable) ===
    if let Some(v) = tool.input.get("max_output_tokens") {
        if v.is_null() {
            state.spine_config.max_output_tokens = None;
            changes.push("max_output_tokens = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            state.spine_config.max_output_tokens = Some(n as usize);
            changes.push(format!("max_output_tokens = {}", n));
        }
    }

    if let Some(v) = tool.input.get("max_cost") {
        if v.is_null() {
            state.spine_config.max_cost = None;
            changes.push("max_cost = disabled".to_string());
        } else if let Some(n) = v.as_f64() {
            state.spine_config.max_cost = Some(n);
            changes.push(format!("max_cost = ${:.2}", n));
        }
    }

    if let Some(v) = tool.input.get("max_duration_secs") {
        if v.is_null() {
            state.spine_config.max_duration_secs = None;
            changes.push("max_duration_secs = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            state.spine_config.max_duration_secs = Some(n);
            changes.push(format!("max_duration_secs = {}s", n));
        }
    }

    if let Some(v) = tool.input.get("max_messages") {
        if v.is_null() {
            state.spine_config.max_messages = None;
            changes.push("max_messages = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            state.spine_config.max_messages = Some(n as usize);
            changes.push(format!("max_messages = {}", n));
        }
    }

    if let Some(v) = tool.input.get("max_auto_retries") {
        if v.is_null() {
            state.spine_config.max_auto_retries = None;
            changes.push("max_auto_retries = disabled".to_string());
        } else if let Some(n) = v.as_u64() {
            state.spine_config.max_auto_retries = Some(n as usize);
            changes.push(format!("max_auto_retries = {}", n));
        }
    }

    // === Reset runtime counters ===
    if let Some(true) = tool.input.get("reset_counters").and_then(|v| v.as_bool()) {
        state.spine_config.auto_continuation_count = 0;
        state.spine_config.autonomous_start_ms = None;
        changes.push("reset runtime counters".to_string());
    }

    state.touch_panel(ContextType::Spine);

    if changes.is_empty() {
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: "No changes made. Pass at least one parameter to configure.".to_string(),
            is_error: false,
        }
    } else {
        ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!(
                "Spine configured:\n{}",
                changes.iter().map(|c| format!("  • {}", c)).collect::<Vec<_>>().join("\n")
            ),
            is_error: false,
        }
    }
}
