use crate::types::PromptState;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Opens a prompt's content in the Library panel editor for reading/editing.
/// Max one prompt open at a time — opening a new one replaces the previous.
pub(crate) fn open_editor(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true),
    };

    // Verify the ID exists
    let ps = PromptState::get(state);
    let found = ps.agents.iter().any(|a| a.id == id)
        || ps.skills.iter().any(|s| s.id == id)
        || ps.commands.iter().any(|c| c.id == id);

    if !found {
        return ToolResult::new(tool.id.clone(), format!("ID '{id}' not found in agents, skills, or commands"), true);
    }

    let previous = PromptState::get(state).open_prompt_id.clone();
    PromptState::get_mut(state).open_prompt_id = Some(id.clone());
    state.touch_panel(Kind::LIBRARY);

    let msg = previous.map_or_else(
        || format!("Opened '{id}' in Library editor. Content is now visible in the Library panel."),
        |prev| format!("Opened '{id}' in Library editor (closed previous: '{prev}'). Content is now visible in the Library panel."),
    );

    ToolResult::new(tool.id.clone(), msg, false)
}

/// Closes the prompt editor in the Library panel.
pub(crate) fn close_editor(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(previous) = PromptState::get(state).open_prompt_id.clone() else {
        return ToolResult::new(tool.id.clone(), "No prompt editor is currently open.".to_string(), true);
    };

    PromptState::get_mut(state).open_prompt_id = None;
    state.touch_panel(Kind::LIBRARY);

    ToolResult::new(
        tool.id.clone(),
        format!("Closed prompt editor (was editing '{previous}'). Library panel restored to normal view."),
        false,
    )
}
