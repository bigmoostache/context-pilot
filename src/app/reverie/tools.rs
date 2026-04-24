//! Reverie tool definitions, dispatch, and the Report tool.
//!
//! The reverie has access to a curated subset of tools for context management,
//! plus a mandatory Report tool to end its run.

use crate::infra::tools::{ParamType, ToolDefinition, ToolResult, ToolTexts, ToolUse};
use crate::state::State;
use cp_base::config::REVERIE;
use std::fmt::Write as _;

/// Parsed tool text definitions for the reverie subsystem.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/reverie.yaml")));

/// Build a human-readable text describing which tools the reverie is allowed to use.
/// This is injected at the top of the reverie's conversation panel (P-reverie) so the
/// LLM knows its constraints, even though it sees ALL tool definitions in the prompt.
pub(crate) fn build_tool_restrictions_text(tools: &[ToolDefinition]) -> String {
    let r = &REVERIE.tool_restrictions;
    let mut text = r.header.trim_end().to_string();
    text.push('\n');

    for tool in tools {
        if tool.reverie_allowed {
            let _r = write!(text, "\n- {}", tool.id);
        }
    }

    text.push_str("\n\n");
    text.push_str(r.footer.trim_end());
    text.push_str("\n\n");
    text.push_str(r.report_instructions.trim_end());
    text.push('\n');
    text
}
///
/// Build the `optimize_context` tool definition for the main AI.
///
/// This tool lets the main AI explicitly invoke a reverie sub-agent
/// with an optional directive and agent selection.
pub(crate) fn optimize_context_tool_definition() -> ToolDefinition {
    let t = &*TOOL_TEXTS;
    ToolDefinition::from_yaml("optimize_context", t)
        .short_desc("Invoke the reverie context optimizer")
        .category("Reverie")
        .param("directive", ParamType::String, false)
        .param("agent", ParamType::String, false)
        .build()
}

/// Execute the Report tool: create a spine notification and signal reverie destruction.
///
/// Returns the `ToolResult`. The caller (event loop) is responsible for actually
/// destroying the reverie state after processing this result.
pub(crate) fn execute_report(tool: &ToolUse, state: &State) -> ToolResult {
    // Block report if queue has unflushed actions
    let qs = cp_mod_queue::types::QueueState::get(state);
    if !qs.queued_calls.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: REVERIE.errors.queue_not_empty.replace("{count}", &qs.queued_calls.len().to_string()),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    }

    let summary = tool.input.get("summary").and_then(|v| v.as_str()).unwrap_or("Reverie completed without summary.");

    // The actual spine notification creation and reverie destruction
    // happens in the event loop when it processes this result.
    // We return the summary text as content so the event loop knows what to notify.
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("REVERIE_REPORT:{summary}"),
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// Execute the `optimize_context` tool from the main AI.
///
/// Validates preconditions and returns an ack. The actual reverie start
/// happens in the event loop when it processes this result.
pub(crate) fn execute_optimize_context(tool: &ToolUse, state: &State) -> ToolResult {
    // Guard: reverie disabled
    if !state.flags.config.reverie_enabled {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: REVERIE.errors.reverie_disabled.clone(),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    }

    // Agent is configurable — default to "cleaner" if not provided
    let agent_id =
        tool.input.get("agent").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("cleaner").to_string();

    // Guard: this specific agent type is already running
    if state.reveries.contains_key(&agent_id) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: REVERIE.errors.already_running.replace(concat!("{", "agent_id", "}"), &agent_id),
            display: None,
            is_error: true,
            tool_name: tool.name.clone(),
        };
    }

    let context = tool.input.get("directive").and_then(|v| v.as_str()).map(ToString::to_string);

    // Signal to the event loop that a reverie should be started.
    // Sentinel format: REVERIE_START:<agent_id>\n<context_or_empty>\n<human_readable_msg>
    let msg = match &context {
        Some(c) if !c.is_empty() => format!(
            "Context optimizer activated with directive: \"{c}\". It will run in the background and report when done."
        ),
        _ => "Context optimizer activated. It will run in the background and report when done.".to_string(),
    };

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("REVERIE_START:{}\n{}\n{}", agent_id, context.as_deref().unwrap_or(""), msg),
        display: None,
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// Dispatch a reverie tool call.
///
/// Routes Report to our handler, everything else to the normal module dispatch.
/// Returns None if the tool should be dispatched to modules (caller handles it).
pub(crate) fn dispatch_reverie_tool(tool: &ToolUse, state: &State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "reverie_report" => Some(execute_report(tool, state)),
        _ => {
            // Verify tool is allowed for reveries via the reverie_allowed flag
            if state.tools.iter().any(|t| t.id == tool.name && t.reverie_allowed) {
                // Delegate to normal module dispatch
                None
            } else {
                Some(ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: REVERIE.errors.tool_not_available.replace("{tool_name}", &tool.name),
                    display: None,
                    is_error: true,
                    tool_name: tool.name.clone(),
                })
            }
        }
    }
}
