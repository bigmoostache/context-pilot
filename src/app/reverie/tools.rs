//! Reverie tool definitions, dispatch, and the Report tool.
//!
//! The reverie has access to a curated subset of tools for context management,
//! plus a mandatory Report tool to end its run.

use crate::infra::tools::{ParamType, ToolDefinition, ToolResult, ToolTexts, ToolUse};
use crate::state::State;
use cp_base::config::REVERIE;

static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
    serde_yaml::from_str(include_str!("../../../yamls/tools/reverie.yaml")).expect("Failed to parse reverie tool YAML")
});

/// Build a human-readable text describing which tools the reverie is allowed to use.
/// This is injected at the top of the reverie's conversation panel (P-reverie) so the
/// LLM knows its constraints, even though it sees ALL tool definitions in the prompt.
pub(crate) fn build_tool_restrictions_text(tools: &[ToolDefinition]) -> String {
    let r = &REVERIE.tool_restrictions;
    let mut text = r.header.trim_end().to_string();
    text.push('\n');

    for tool in tools {
        if tool.reverie_allowed {
            text.push_str(&format!("\n- {}", tool.id));
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
/// Build the optimize_context tool definition for the main AI.
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
/// Returns the ToolResult. The caller (event loop) is responsible for actually
/// destroying the reverie state after processing this result.
pub(crate) fn execute_report(tool: &ToolUse, state: &State) -> ToolResult {
    // Block report if queue has unflushed actions
    let qs = cp_mod_queue::QueueState::get(state);
    if !qs.queued_calls.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: REVERIE.errors.queue_not_empty.replace("{count}", &qs.queued_calls.len().to_string()),
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
        content: format!("REVERIE_REPORT:{}", summary),
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// Execute the optimize_context tool from the main AI.
///
/// Validates preconditions and returns an ack. The actual reverie start
/// happens in the event loop when it processes this result.
pub(crate) fn execute_optimize_context(tool: &ToolUse, state: &State) -> ToolResult {
    // Guard: reverie disabled
    if !state.reverie_enabled {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: REVERIE.errors.reverie_disabled.clone(),
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
            content: REVERIE.errors.already_running.replace("{agent_id}", &agent_id),
            is_error: true,
            tool_name: tool.name.clone(),
        };
    }

    let context = tool.input.get("directive").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Signal to the event loop that a reverie should be started.
    // Sentinel format: REVERIE_START:<agent_id>\n<context_or_empty>\n<human_readable_msg>
    let msg = match &context {
        Some(c) if !c.is_empty() => format!(
            "Context optimizer activated with directive: \"{}\". It will run in the background and report when done.",
            c
        ),
        _ => "Context optimizer activated. It will run in the background and report when done.".to_string(),
    };

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("REVERIE_START:{}\n{}\n{}", agent_id, context.as_deref().unwrap_or(""), msg),
        is_error: false,
        tool_name: tool.name.clone(),
    }
}

/// Dispatch a reverie tool call.
///
/// Routes Report to our handler, everything else to the normal module dispatch.
/// Returns None if the tool should be dispatched to modules (caller handles it).
pub(crate) fn dispatch_reverie_tool(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
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
                    is_error: true,
                    tool_name: tool.name.clone(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(id: &str, reverie_allowed: bool) -> ToolDefinition {
        ToolDefinition {
            id: id.to_string(),
            name: id.to_string(),
            short_desc: String::new(),
            description: String::new(),
            params: vec![],
            enabled: true,
            reverie_allowed,
            category: String::new(),
        }
    }

    #[test]
    fn report_tool_returns_sentinel() {
        let tool = ToolUse {
            id: "test_id".to_string(),
            name: "reverie_report".to_string(),
            input: serde_json::json!({"summary": "Closed 3 panels"}),
        };
        let mut state = State::default();
        state.set_ext(cp_mod_queue::QueueState::new());
        let result = execute_report(&tool, &state);
        assert!(result.content.starts_with("REVERIE_REPORT:"));
        assert!(result.content.contains("Closed 3 panels"));
    }

    #[test]
    fn dispatch_report_routes_correctly() {
        let tool = ToolUse {
            id: "t1".to_string(),
            name: "reverie_report".to_string(),
            input: serde_json::json!({"summary": "done"}),
        };
        let mut state = State::default();
        state.set_ext(cp_mod_queue::QueueState::new());
        let result = dispatch_reverie_tool(&tool, &mut state);
        assert!(result.is_some());
        assert!(result.unwrap().content.starts_with("REVERIE_REPORT:"));
    }

    #[test]
    fn dispatch_forbidden_tool_returns_error() {
        let tool = ToolUse { id: "t2".to_string(), name: "Edit".to_string(), input: serde_json::json!({}) };
        let mut state = State::default();
        // Edit is not in state.tools at all, so dispatch treats it as forbidden
        let result = dispatch_reverie_tool(&tool, &mut state);
        assert!(result.is_some());
        assert!(result.unwrap().is_error);
    }

    #[test]
    fn dispatch_allowed_tool_delegates() {
        let tool = ToolUse { id: "t3".to_string(), name: "Close_panel".to_string(), input: serde_json::json!({}) };
        let mut state = State::default();
        // Add Close_panel with reverie_allowed: true to state.tools
        state.tools.push(make_tool("Close_panel", true));
        let result = dispatch_reverie_tool(&tool, &mut state);
        // Allowed tools return None (delegate to module dispatch)
        assert!(result.is_none());
    }

    #[test]
    fn dispatch_non_reverie_tool_rejected() {
        let tool = ToolUse { id: "t4".to_string(), name: "Edit".to_string(), input: serde_json::json!({}) };
        let mut state = State::default();
        // Add Edit with reverie_allowed: false
        state.tools.push(make_tool("Edit", false));
        let result = dispatch_reverie_tool(&tool, &mut state);
        assert!(result.is_some());
        assert!(result.unwrap().is_error);
    }

    #[test]
    fn build_tool_restrictions_includes_allowed() {
        let tools = vec![make_tool("Close_panel", true), make_tool("Edit", false)];
        let text = build_tool_restrictions_text(&tools);
        assert!(text.contains("Close_panel"));
        assert!(!text.contains("- Edit"));
        assert!(text.contains("reverie_report"));
    }
}
