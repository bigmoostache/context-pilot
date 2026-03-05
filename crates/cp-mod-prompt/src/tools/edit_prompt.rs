use crate::storage;
use crate::types::{PromptState, PromptType};
use cp_base::panels::now_ms;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use std::fmt::Write as _;

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
        state.touch_panel(Kind::LIBRARY);
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
    let ps_readonly = PromptState::get(state);
    let (is_builtin, current_content) = match entity_type {
        EntityType::Agent => {
            let Some(a) = ps_readonly.agents.iter().find(|a| a.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Agent '{id}' vanished mid-edit"), true);
            };
            (a.is_builtin, a.content.clone())
        }
        EntityType::Skill => {
            let Some(s) = ps_readonly.skills.iter().find(|s| s.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Skill '{id}' vanished mid-edit"), true);
            };
            (s.is_builtin, s.content.clone())
        }
        EntityType::Command => {
            let Some(c) = ps_readonly.commands.iter().find(|c| c.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Command '{id}' vanished mid-edit"), true);
            };
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
            format!("{}...", &old_string.get(..old_string.floor_char_boundary(50)).unwrap_or(""))
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
    let ps_mut = PromptState::get_mut(state);
    match entity_type {
        EntityType::Agent => {
            let Some(a) = ps_mut.agents.iter_mut().find(|a| a.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Agent '{id}' vanished mid-edit"), true);
            };
            a.content = new_content;
            storage::save_prompt_to_dir(&storage::dir_for(PromptType::Agent), a);
            state.touch_panel(Kind::SYSTEM);
        }
        EntityType::Skill => {
            let Some(s) = ps_mut.skills.iter_mut().find(|s| s.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Skill '{id}' vanished mid-edit"), true);
            };
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
            let Some(c) = ps_mut.commands.iter_mut().find(|c| c.id == id) else {
                return ToolResult::new(tool.id.clone(), format!("Command '{id}' vanished mid-edit"), true);
            };
            c.content = new_content;
            let cmd_clone = c.clone();
            storage::save_prompt_to_dir(&storage::dir_for(PromptType::Command), &cmd_clone);
        }
    }

    state.touch_panel(Kind::LIBRARY);

    // Format result as unified diff (same format as file Edit tool)
    let lines_changed = new_string.lines().count().max(old_string.lines().count());
    let mut result_msg = String::new();

    if replace_all && replaced > 1 {
        let _r = writeln!(
            result_msg,
            "Edited {} '{}': {} replacements (~{} lines changed each)",
            entity_type.label(),
            id,
            replaced,
            lines_changed
        );
    } else {
        let _r = writeln!(result_msg, "Edited {} '{}': ~{} lines changed", entity_type.label(), id, lines_changed);
    }

    result_msg.push_str("```diff\n");
    result_msg.push_str(&generate_unified_diff(old_string, new_string));
    result_msg.push_str("```");

    ToolResult::new(tool.id.clone(), result_msg, false)
}

/// The kind of prompt entity being edited.
enum EntityType {
    /// An agent prompt.
    Agent,
    /// A skill prompt.
    Skill,
    /// A command prompt.
    Command,
}

impl EntityType {
    /// Return a human-readable label for this entity type.
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
        if let Some(&(lcs_old, lcs_new)) = lcs.get(lcs_idx) {
            while old_idx < lcs_old {
                if let Some(line) = old_lines.get(old_idx) {
                    let _r = writeln!(result, "- {line}");
                }
                old_idx = old_idx.saturating_add(1);
            }
            while new_idx < lcs_new {
                if let Some(line) = new_lines.get(new_idx) {
                    let _r = writeln!(result, "+ {line}");
                }
                new_idx = new_idx.saturating_add(1);
            }
            if let Some(line) = old_lines.get(old_idx) {
                let _r = writeln!(result, "  {line}");
            }
            old_idx = old_idx.saturating_add(1);
            new_idx = new_idx.saturating_add(1);
            lcs_idx = lcs_idx.saturating_add(1);
        } else {
            while old_idx < old_lines.len() {
                if let Some(line) = old_lines.get(old_idx) {
                    let _r = writeln!(result, "- {line}");
                }
                old_idx = old_idx.saturating_add(1);
            }
            while new_idx < new_lines.len() {
                if let Some(line) = new_lines.get(new_idx) {
                    let _r = writeln!(result, "+ {line}");
                }
                new_idx = new_idx.saturating_add(1);
            }
        }
    }

    result
}

/// Compute the longest common subsequence index pairs between two line slices.
fn lcs<'src>(old: &[&'src str], new: &[&'src str]) -> Vec<(usize, usize)> {
    let old_len = old.len();
    let new_len = new.len();
    let mut lengths = vec![vec![0usize; new_len.saturating_add(1)]; old_len.saturating_add(1)];

    for i in 1..=old_len {
        for j in 1..=new_len {
            let Some(old_val) = old.get(i.saturating_sub(1)) else { continue };
            let Some(new_val) = new.get(j.saturating_sub(1)) else { continue };
            let value = if old_val == new_val {
                lengths
                    .get(i.saturating_sub(1))
                    .and_then(|r| r.get(j.saturating_sub(1)))
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1)
            } else {
                let up = lengths.get(i.saturating_sub(1)).and_then(|r| r.get(j)).copied().unwrap_or(0);
                let left = lengths.get(i).and_then(|r| r.get(j.saturating_sub(1))).copied().unwrap_or(0);
                up.max(left)
            };
            let Some(cell) = lengths.get_mut(i).and_then(|r| r.get_mut(j)) else { continue };
            *cell = value;
        }
    }

    let mut result = Vec::new();
    let mut i = old_len;
    let mut j = new_len;
    while i > 0 && j > 0 {
        let old_val = old.get(i.saturating_sub(1));
        let new_val = new.get(j.saturating_sub(1));
        if old_val == new_val {
            result.push((i.saturating_sub(1), j.saturating_sub(1)));
            i = i.saturating_sub(1);
            j = j.saturating_sub(1);
        } else {
            let up = lengths.get(i.saturating_sub(1)).and_then(|r| r.get(j)).copied().unwrap_or(0);
            let left = lengths.get(i).and_then(|r| r.get(j.saturating_sub(1))).copied().unwrap_or(0);
            if up > left {
                i = i.saturating_sub(1);
            } else {
                j = j.saturating_sub(1);
            }
        }
    }
    result.reverse();
    result
}
