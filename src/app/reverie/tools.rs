//! Reverie tool definitions, dispatch, and the Report tool.
//!
//! The reverie has access to a curated subset of tools for context management,
//! plus a mandatory Report tool to end its run.

use crate::infra::tools::{ParamType, ToolDefinition, ToolParam, ToolResult, ToolUse};
use crate::state::State;

/// Build the Report tool definition — the reverie's mandatory end-of-run tool.
#[cfg_attr(not(test), allow(dead_code))]
pub fn report_tool_definition() -> ToolDefinition {
    ToolDefinition {
        id: "reverie_report".to_string(),
        name: "Report".to_string(),
        short_desc: "End reverie run with summary".to_string(),
        description: "Mandatory: call this when you're done optimizing context. \
        Writes a summary of what you did to a spine notification, then destroys this reverie session. \
        You MUST call this tool before ending your turn."
            .to_string(),
        params: vec![
            ToolParam::new("summary", ParamType::String)
                .desc("Brief summary of what you optimized (1-3 sentences)")
                .required(),
        ],
        enabled: true,
        reverie_allowed: false,
        category: "Reverie".to_string(),
    }
}

/// Build a human-readable text describing which tools the reverie is allowed to use.
/// This is injected at the top of the reverie's conversation panel (P-reverie) so the
/// LLM knows its constraints, even though it sees ALL tool definitions in the prompt.
pub fn build_tool_restrictions_text(tools: &[crate::infra::tools::ToolDefinition]) -> String {
    let mut text = String::from(
        "## Tool Restrictions\n\
         You are a reverie (context optimizer sub-agent). You may ONLY use the following tools:\n\n",
    );
    for tool in tools {
        if tool.reverie_allowed {
            text.push_str(&format!("- {}\n", tool.id));
        }
    }
    text.push_str(
        "\nIf you call any tool NOT in this list, it will be rejected with an error. \
         Focus on context management only.\n\n",
    );

    // Report instructions — the reverie ends by calling a special tool
    text.push_str(
        "## Ending Your Run (MANDATORY)\n\
         When you are done optimizing, you MUST call the `reverie_report` tool with a brief summary.\n\
         This is how you signal completion. Your run will be force-terminated if you don't.\n\n\
         Call it like this:\n\
         ```\n\
         reverie_report({\"summary\": \"Closed 5 stale panels, summarized 12 logs.\"})\n\
         ```\n\
         The `summary` parameter is a short string (1-3 sentences) describing what you did.\n",
    );
    text
}
///
/// Filters the main tool list to the allowed subset, then appends the Report tool.
#[cfg_attr(not(test), allow(dead_code))]
pub fn reverie_tool_definitions(main_tools: &[ToolDefinition]) -> Vec<ToolDefinition> {
    let mut anchor_tools: Vec<ToolDefinition> =
        main_tools.iter().filter(|t| t.enabled && t.reverie_allowed).cloned().collect();

    anchor_tools.push(report_tool_definition());
    anchor_tools
}

/// Build the optimize_context tool definition for the main AI.
///
/// This tool lets the main AI explicitly invoke a reverie sub-agent
/// with an optional directive and agent selection.
pub fn optimize_context_tool_definition() -> ToolDefinition {
    ToolDefinition {
        id: "optimize_context".to_string(),
        name: "Optimize Context".to_string(),
        short_desc: "Invoke the reverie context optimizer".to_string(),
        description: "Triggers the background context optimizer (reverie). \
        The reverie will analyze current context, close irrelevant panels, \
        summarize logs, and reshape context for the current task. \
        Optionally provide a directive to guide optimization \
        (e.g., 'I'm about to work on the UI, optimize context for that'). \
        Cannot invoke if a reverie is already active or reverie is disabled."
            .to_string(),
        params: vec![
            ToolParam::new("directive", ParamType::String)
                .desc("Optional guidance for the optimizer (e.g., 'focus on git module files')"),
            ToolParam::new("agent", ParamType::String)
                .desc("Agent ID to drive the reverie (default: 'cleaner'). Use 'cartographer' to auto-describe files in the tree."),
        ],
        enabled: true,
        reverie_allowed: false,
        category: "Reverie".to_string(),
    }
}

/// Execute the Report tool: create a spine notification and signal reverie destruction.
///
/// Returns the ToolResult. The caller (event loop) is responsible for actually
/// destroying the reverie state after processing this result.
#[cfg_attr(not(test), allow(dead_code))]
pub fn execute_report(tool: &ToolUse) -> ToolResult {
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
pub fn execute_optimize_context(tool: &ToolUse, state: &State) -> ToolResult {
    // Guard: reverie disabled
    if !state.reverie_enabled {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Reverie is disabled. Enable it in config (Ctrl+H → r) first.".to_string(),
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
            content: format!(
                "A reverie with agent '{}' is already running. Wait for it to complete before invoking again.",
                agent_id
            ),
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
#[cfg_attr(not(test), allow(dead_code))]
pub fn dispatch_reverie_tool(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "reverie_report" => Some(execute_report(tool)),
        _ => {
            // Verify tool is allowed for reveries via the reverie_allowed flag
            if state.tools.iter().any(|t| t.id == tool.name && t.reverie_allowed) {
                // Delegate to normal module dispatch
                None
            } else {
                Some(ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: format!("Tool '{}' is not available to the reverie.", tool.name),
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
    fn reverie_tool_definitions_include_allowed_and_report() {
        let main_tools = vec![make_tool("Close_panel", true), make_tool("Edit", false)];

        let tools = reverie_tool_definitions(&main_tools);
        assert!(tools.iter().any(|t| t.id == "Close_panel"));
        assert!(!tools.iter().any(|t| t.id == "Edit"));
        assert!(tools.iter().any(|t| t.id == "reverie_report"));
    }

    #[test]
    fn report_tool_returns_sentinel() {
        let tool = ToolUse {
            id: "test_id".to_string(),
            name: "reverie_report".to_string(),
            input: serde_json::json!({"summary": "Closed 3 panels"}),
        };
        let result = execute_report(&tool);
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
