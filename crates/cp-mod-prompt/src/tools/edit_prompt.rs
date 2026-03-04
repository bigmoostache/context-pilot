use crate::storage;
use crate::types::{PromptState, PromptType};
use cp_base::panels::now_ms;
use cp_base::state::{ContextType, State, estimate_tokens};
use cp_base::tools::{ToolResult, ToolUse};

/// Unified diff-based edit tool for agents, skills, and commands.
/// Uses the same `old_string/new_string` pattern as the file Edit tool.
/// Routes to agent/skill/command based on the provided ID.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(id) if !id.is_empty() => id,
        _ => return ToolResult::new(tool.id.clone(), "Missing required 'id' parameter".to_string(), true),
    };

    let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'old_string' parameter".to_string(), true);
    };

    let Some(new_string) = tool.input.get("new_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required 'new_string' parameter".to_string(), true);
    };

    let replace_all = tool.input.get("replace_all").and_then(serde_json::Value::as_bool).unwrap_or(false);

    // Try to find the ID in agents, skills, then commands
    let ps = PromptState::get(state);
    let entity_type = if ps.agents.iter().any(|a| a.id == id) {
        EntityType::Agent
    } else if ps.skills.iter().any(|s| s.id == id) {
        EntityType::Skill
    } else if ps.commands.iter().any(|c| c.id == id) {
        EntityType::Command
    } else {
        return ToolResult::new(tool.id.clone(), format!("ID '{id}' not found in agents, skills, or commands"), true);
    };

    // Check that the prompt is open in the Library editor
    let is_open = PromptState::get(state).open_prompt_id.as_deref() == Some(id);
    if !is_open {
        // Auto-open the prompt and fail with a helpful message
        PromptState::get_mut(state).open_prompt_id = Some(id.to_string());
        state.touch_panel(ContextType::new(ContextType::LIBRARY));
        return ToolResult::new(
            tool.id.clone(),
            format!(
                "Cannot edit '{id}': prompt was not open in the Library editor.\n\
                 I've automatically opened it for you — its content is now visible in the Library panel.\n\
                 Please review the content and retry your Edit_prompt call."
            ),
            true,
        );
    }

    // Get the item and check if builtin
    let ps = PromptState::get(state);
    let (is_builtin, current_content) = match entity_type {
        EntityType::Agent => {
            let a = ps.agents.iter().find(|a| a.id == id).unwrap();
            (a.is_builtin, a.content.clone())
        }
        EntityType::Skill => {
            let s = ps.skills.iter().find(|s| s.id == id).unwrap();
            (s.is_builtin, s.content.clone())
        }
        EntityType::Command => {
            let c = ps.commands.iter().find(|c| c.id == id).unwrap();
            (c.is_builtin, c.content.clone())
        }
    };

    if is_builtin {
        return ToolResult::new(
            tool.id.clone(),
            format!("Cannot edit built-in {} '{}'", entity_type.label(), id),
            true,
        );
    }

    // Perform the replacement
    let count = current_content.matches(old_string).count();
    if count == 0 {
        let preview = if old_string.len() > 50 {
            format!("{}...", &old_string[..old_string.floor_char_boundary(50)])
        } else {
            old_string.to_string()
        };
        return ToolResult::new(
            tool.id.clone(),
            format!("No match found for \"{}\" in {} '{}'", preview, entity_type.label(), id),
            true,
        );
    }

    let new_content = if replace_all {
        current_content.replace(old_string, new_string)
    } else {
        current_content.replacen(old_string, new_string, 1)
    };
    let replaced = if replace_all { count } else { 1 };

    // Apply the change
    let ps = PromptState::get_mut(state);
    match entity_type {
        EntityType::Agent => {
            let a = ps.agents.iter_mut().find(|a| a.id == id).unwrap();
            a.content = new_content;
            storage::save_prompt_to_dir(&storage::dir_for(PromptType::Agent), a);
            state.touch_panel(ContextType::new(ContextType::SYSTEM));
        }
        EntityType::Skill => {
            let s = ps.skills.iter_mut().find(|s| s.id == id).unwrap();
            s.content = new_content;
            let skill_clone = s.clone();
            storage::save_prompt_to_dir(&storage::dir_for(PromptType::Skill), &skill_clone);

            // If loaded, update the panel's cached_content
            let is_loaded = PromptState::get(state).loaded_skill_ids.contains(&id.to_string());
            if is_loaded {
                let content_str = format!("[{}] {}\n\n{}", skill_clone.id, skill_clone.name, skill_clone.content);
                let tokens = estimate_tokens(&content_str);
                if let Some(ctx) = state.context.iter_mut().find(|c| c.get_meta_str("skill_prompt_id") == Some(id)) {
                    ctx.cached_content = Some(content_str);
                    ctx.token_count = tokens;
                    ctx.last_refresh_ms = now_ms();
                }
            }
        }
        EntityType::Command => {
            let c = ps.commands.iter_mut().find(|c| c.id == id).unwrap();
            c.content = new_content;
            let cmd_clone = c.clone();
            storage::save_prompt_to_dir(&storage::dir_for(PromptType::Command), &cmd_clone);
        }
    }

    state.touch_panel(ContextType::new(ContextType::LIBRARY));

    // Format result as unified diff (same format as file Edit tool)
    let lines_changed = new_string.lines().count().max(old_string.lines().count());
    let mut result_msg = String::new();

    if replace_all && replaced > 1 {
        result_msg.push_str(&format!(
            "Edited {} '{}': {} replacements (~{} lines changed each)\n",
            entity_type.label(),
            id,
            replaced,
            lines_changed
        ));
    } else {
        result_msg.push_str(&format!("Edited {} '{}': ~{} lines changed\n", entity_type.label(), id, lines_changed));
    }

    result_msg.push_str("```diff\n");
    result_msg.push_str(&generate_unified_diff(old_string, new_string));
    result_msg.push_str("```");

    ToolResult::new(tool.id.clone(), result_msg, false)
}

enum EntityType {
    Agent,
    Skill,
    Command,
}

impl EntityType {
    const fn label(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Skill => "skill",
            Self::Command => "command",
        }
    }
}

/// Generate a unified diff showing changes between old and new strings.
/// Same format as the file Edit tool's output.
fn generate_unified_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let lcs = lcs(&old_lines, &new_lines);
    let mut result = String::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    let mut lcs_idx = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if lcs_idx < lcs.len() {
            let (lcs_old, lcs_new) = lcs[lcs_idx];
            while old_idx < lcs_old {
                result.push_str(&format!("- {}\n", old_lines[old_idx]));
                old_idx += 1;
            }
            while new_idx < lcs_new {
                result.push_str(&format!("+ {}\n", new_lines[new_idx]));
                new_idx += 1;
            }
            result.push_str(&format!("  {}\n", old_lines[old_idx]));
            old_idx += 1;
            new_idx += 1;
            lcs_idx += 1;
        } else {
            while old_idx < old_lines.len() {
                result.push_str(&format!("- {}\n", old_lines[old_idx]));
                old_idx += 1;
            }
            while new_idx < new_lines.len() {
                result.push_str(&format!("+ {}\n", new_lines[new_idx]));
                new_idx += 1;
            }
        }
    }

    result
}

fn lcs<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(usize, usize)> {
    let m = old.len();
    let n = new.len();
    let mut lengths = vec![vec![0; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                lengths[i][j] = lengths[i - 1][j - 1] + 1;
            } else {
                lengths[i][j] = lengths[i - 1][j].max(lengths[i][j - 1]);
            }
        }
    }

    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] {
            result.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if lengths[i - 1][j] > lengths[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    result.reverse();
    result
}
