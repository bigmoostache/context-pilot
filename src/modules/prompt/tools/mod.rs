pub mod agent;
pub mod command;
pub mod skill;

use crate::state::State;
use crate::tools::{ToolResult, ToolUse};

pub fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "agent_create" => Some(agent::create(tool, state)),
        "agent_edit" => Some(agent::edit(tool, state)),
        "agent_delete" => Some(agent::delete(tool, state)),
        "agent_load" => Some(agent::load(tool, state)),
        "skill_create" => Some(skill::create(tool, state)),
        "skill_edit" => Some(skill::edit(tool, state)),
        "skill_delete" => Some(skill::delete(tool, state)),
        "skill_load" => Some(skill::load(tool, state)),
        "skill_unload" => Some(skill::unload(tool, state)),
        "command_create" => Some(command::create(tool, state)),
        "command_edit" => Some(command::edit(tool, state)),
        "command_delete" => Some(command::delete(tool, state)),
        _ => None,
    }
}
