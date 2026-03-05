/// Agent CRUD tool handlers.
pub(crate) mod agent;
/// Command CRUD tool handlers.
pub(crate) mod command;
/// Diff-based prompt editing tool handler.
pub(crate) mod edit_prompt;
/// Library editor open/close tool handlers.
pub(crate) mod library_editor;
/// Skill CRUD and load/unload tool handlers.
pub(crate) mod skill;

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Dispatch a tool call to the appropriate prompt sub-handler.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "agent_create" => Some(agent::create(tool, state)),
        "agent_delete" => Some(agent::delete(tool, state)),
        "agent_load" => Some(agent::load(tool, state)),
        "skill_create" => Some(skill::create(tool, state)),
        "skill_delete" => Some(skill::delete(tool, state)),
        "skill_load" => Some(skill::load(tool, state)),
        "skill_unload" => Some(skill::unload(tool, state)),
        "command_create" => Some(command::create(tool, state)),
        "command_delete" => Some(command::delete(tool, state)),
        "Edit_prompt" => Some(edit_prompt::execute(tool, state)),
        "Library_open_prompt_editor" => Some(library_editor::open_editor(tool, state)),
        "Library_close_prompt_editor" => Some(library_editor::close_editor(tool, state)),
        _ => None,
    }
}
