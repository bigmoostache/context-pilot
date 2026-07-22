//! Queue flush execution — dequeues and runs all queued tool calls.
//!
//! Extracted from `cleanup.rs` to keep that module under the 500-line limit.

use crate::app::App;
use crate::infra::tools::execute_tool;
use crate::state::{Message, State, ToolUseRecord};

use cp_base::state::context::Kind;
use cp_mod_queue::types::QueueState;

/// Flushed tool execution pair: the original `ToolUse` and its result.
pub(crate) struct FlushedTool {
    /// The original tool-use request that was dequeued and executed.
    pub tool: cp_base::tools::ToolUse,
    /// The execution result for this tool call.
    pub result: crate::infra::tools::ToolResult,
    /// The queue position this tool occupied (for compact display).
    pub queue_index: usize,
}

/// Execute all queued tool calls in order.
/// Returns (`summary_result`, `flushed_tools`) so the pipeline can run callbacks/sentinels
/// on the individual tools — not just the `Queue_execute` wrapper.
pub(crate) fn execute_queue_flush(
    tool: &cp_base::tools::ToolUse,
    state: &mut State,
) -> (crate::infra::tools::ToolResult, Vec<FlushedTool>) {
    let qs = QueueState::get_mut(state);
    if qs.queued_calls.is_empty() {
        return (
            crate::infra::tools::ToolResult::new(
                tool.id.clone(),
                "Queue is empty \u{2014} nothing to execute.".to_owned(),
                false,
            ),
            Vec::new(),
        );
    }
    let calls = qs.flush();
    qs.active = false;

    let summary = format!("Executed {} queued action(s).", calls.len());
    let mut flushed = Vec::with_capacity(calls.len());

    for call in &calls {
        // Generate a fresh tool_use_id to avoid collision with the intercept-time message.
        // The original id was already used in the "Queued as #N" tool_result at intercept time.
        let fresh_id = format!("flush_{}_{}", call.index, call.tool_use_id);
        let queued_tool = cp_base::tools::ToolUse::new(fresh_id, call.tool_name.clone(), call.input.clone());
        let result = execute_tool(&queued_tool, state);
        flushed.push(FlushedTool { tool: queued_tool, result, queue_index: call.index });
    }

    // The summary wrapper preserves tempo — only the individual flushed
    // tool results should drive the tempo decision (transparent queue).
    let mut wrapper = crate::infra::tools::ToolResult::new(tool.id.clone(), summary, false);
    wrapper.preserves_tempo = true;
    (wrapper, flushed)
}

/// Create and persist a compact `tool_call` message for a queue-flushed `ToolUse`.
///
/// Instead of replaying the full parameters (which duplicate the already-visible
/// "Queued as #N" message), this saves a lightweight `Tool_execution` stub with
/// just the tool name, queue position, and parameter byte-size.
pub(crate) fn save_flushed_tool_call_message(app: &mut App, tool: &cp_base::tools::ToolUse, queue_index: usize) {
    let tool_id = format!("T{}", app.state.next_tool_id);
    let tool_global_uid = format!("UID_{}_T", app.state.global_next_uid);
    app.state.next_tool_id = app.state.next_tool_id.saturating_add(1);
    app.state.global_next_uid = app.state.global_next_uid.saturating_add(1);

    let params_size = serde_json::to_string(&tool.input).map_or(0, |s| s.len());
    let compact_input = serde_json::json!({
        "tool_name": tool.name,
        "tool_position": queue_index,
        "tool_parameters_size": params_size,
    });

    let tool_msg = Message::new_tool_call(
        tool_id,
        Some(tool_global_uid),
        vec![ToolUseRecord::new(tool.id.clone(), "Tool_execution".to_owned(), compact_input)],
    );
    app.save_message_async(&tool_msg);
    app.state.messages.push(tool_msg);
}

/// Append "remaining history panels" info to `Close_conversation_history` results.
///
/// Subtracts panels targeted by queued (but not yet flushed) closes so the
/// AI sees an accurate projection of what will remain after the queue flushes.
pub(crate) fn augment_remaining_history_panels(
    state: &State,
    tools: &[cp_base::tools::ToolUse],
    tool_results: &mut [crate::infra::tools::ToolResult],
) {
    let mut remaining: Vec<String> = state
        .context
        .iter()
        .filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY)
        .map(|c| c.id.clone())
        .collect();

    let qs = QueueState::get(state);
    let queued_closes: Vec<&str> = qs
        .queued_calls
        .iter()
        .filter(|q| q.tool_name == "Close_conversation_history")
        .flat_map(|q| {
            q.input
                .get("panels")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|p| p.get("panel_id").and_then(serde_json::Value::as_str))
        })
        .collect();
    remaining.retain(|id| !queued_closes.contains(&id.as_str()));

    let suffix = if remaining.is_empty() {
        "\nNo conversation history panels remaining.".to_owned()
    } else {
        format!("\nRemaining conversation history panels: {}", remaining.join(", "))
    };

    for (tool, tr) in tools.iter().zip(tool_results.iter_mut()) {
        if tool.name == "Close_conversation_history" && !tr.is_error {
            tr.content.push_str(&suffix);
        }
    }
}
