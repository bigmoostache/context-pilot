use std::fs;
use std::path::Path;

use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use super::diff::generate_unified_diff;
use std::fmt::Write as _;

/// Normalize a string for matching: trim trailing whitespace per line, normalize line endings.
fn normalize_for_match(s: &str) -> String {
    s.replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// Find the best match for `needle` in `haystack` using normalized comparison.
/// Returns the actual substring from haystack that matches (preserving original whitespace).
pub(crate) fn find_normalized_match<'haystack>(haystack: &'haystack str, needle: &str) -> Option<&'haystack str> {
    let norm_needle = normalize_for_match(needle);
    let needle_lines: Vec<&str> = norm_needle.lines().collect();

    if needle_lines.is_empty() {
        return None;
    }

    // Split haystack into lines while tracking byte positions
    let mut line_positions: Vec<(usize, usize)> = vec![]; // (start, end) for each line
    let mut pos: usize = 0;
    for line in haystack.lines() {
        let start = pos;
        let end = pos.saturating_add(line.len());
        line_positions.push((start, end));
        pos = end.saturating_add(1); // +1 for newline (might overshoot at EOF, that's ok)
    }

    let haystack_lines: Vec<&str> = haystack.lines().collect();
    let haystack_lines_normalized: Vec<String> = haystack_lines.iter().map(|l| l.trim_end().to_string()).collect();

    // Try to find needle_lines sequence in haystack_lines_normalized
    'outer: for start_idx in 0..haystack_lines.len() {
        if start_idx.saturating_add(needle_lines.len()) > haystack_lines.len() {
            break;
        }

        for (i, needle_line) in needle_lines.iter().enumerate() {
            let Some(hay_line) = haystack_lines_normalized.get(start_idx.saturating_add(i)) else {
                continue 'outer;
            };
            if *hay_line != *needle_line {
                continue 'outer;
            }
        }

        // Found a match! Return the original substring from haystack
        let Some(&(match_start, _)) = line_positions.get(start_idx) else {
            continue;
        };
        let match_end_idx = start_idx.saturating_add(needle_lines.len()).saturating_sub(1);
        let Some(&(_, match_end)) = line_positions.get(match_end_idx) else {
            continue;
        };

        return Some(haystack.get(match_start..match_end).unwrap_or(""));
    }

    None
}

/// Find closest match for error reporting (returns line number and preview)
fn find_closest_match(haystack: &str, needle: &str) -> Option<(usize, String)> {
    let norm_needle = normalize_for_match(needle);
    let first_needle_line = norm_needle.lines().next()?;

    if first_needle_line.trim().is_empty() {
        return None;
    }

    let haystack_lines: Vec<&str> = haystack.lines().collect();

    // Find lines that partially match the first line of needle
    let mut best_match: Option<(usize, usize, String)> = None; // (line_num, score, preview)

    for (idx, line) in haystack_lines.iter().enumerate() {
        let norm_line = line.trim_end();

        // Simple similarity: count matching characters
        let score = first_needle_line.chars().zip(norm_line.chars()).filter(|(a, b)| a == b).count();

        // Also check if it contains the trimmed needle line
        let contains_score = if norm_line.contains(first_needle_line.trim()) { first_needle_line.len() } else { 0 };

        let total_score = score.max(contains_score);

        if total_score > 0 && best_match.as_ref().is_none_or(|b| total_score > b.1) {
            let preview = if norm_line.len() > 60 {
                format!("{}...", &norm_line.get(..norm_line.floor_char_boundary(60)).unwrap_or(""))
            } else {
                norm_line.to_string()
            };
            best_match = Some((idx.saturating_add(1), total_score, preview));
        }
    }

    best_match.map(|(line, _, preview)| (line, preview))
}

/// Execute the Edit tool: replace `old_string` with `new_string` in a file.
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Get file_path (required)
    let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: file_path".to_string(), true);
    };

    // Get old_string (required)
    let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: old_string".to_string(), true);
    };

    // Get new_string (required)
    let Some(new_string) = tool.input.get("new_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: new_string".to_string(), true);
    };

    // Get replace_all (optional, default false)
    let replace_all = tool.input.get("replace_all").and_then(serde_json::Value::as_bool).unwrap_or(false);

    // Check if file is open in context (canonicalize for consistent comparison)
    let path = Path::new(path_str);
    let canonical = path.canonicalize().map_or_else(|_| path_str.to_string(), |p| p.to_string_lossy().to_string());
    let is_open = state
        .context
        .iter()
        .any(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical));

    // Read file
    let mut content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Failed to read file: {e}"), true);
        }
    };

    // Try normalized matching (handles trailing whitespace differences).
    // Clone the match to break the borrow on `content`, allowing mutation below.
    let actual_match = find_normalized_match(&content, old_string).map(str::to_owned);
    let replaced = actual_match.map_or(0, |actual_match| {
        if replace_all {
            let count = content.matches(actual_match.as_str()).count();
            content = content.replace(actual_match.as_str(), new_string);
            count
        } else {
            content = content.replacen(actual_match.as_str(), new_string, 1);
            1
        }
    });

    if replaced == 0 {
        // Provide helpful error with closest match
        let hint = if let Some((line, preview)) = find_closest_match(&content, old_string) {
            format!(" (closest match at line {line}: \"{preview}\")")
        } else {
            String::new()
        };

        let needle_preview = if old_string.len() > 50 {
            format!("{}...", &old_string.get(..old_string.floor_char_boundary(50)).unwrap_or(""))
        } else {
            old_string.to_string()
        };

        return ToolResult::new(tool.id.clone(), format!("No match found for \"{needle_preview}\"{hint}"), true);
    }

    // Write file
    if let Err(e) = fs::write(path, &content) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file: {e}"), true);
    }

    // Update the context element's token count
    if let Some(ctx) = state
        .context
        .iter_mut()
        .find(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical))
    {
        ctx.token_count = estimate_tokens(&content);
    }

    // Count approximate lines changed
    let lines_changed = new_string.lines().count().max(old_string.lines().count());

    // Build the user-facing display with full diff
    let mut display_msg = String::new();

    // Warn if file was not open in context (edit still succeeded via unique match)
    if !is_open {
        let _r = writeln!(
            display_msg,
            "Warning: File '{path_str}' was not open in context. Edit succeeded (unique match found) but open the file to verify."
        );
    }

    // Header line
    if replace_all && replaced > 1 {
        let _r =
            writeln!(display_msg, "Edited '{path_str}': {replaced} replacements (~{lines_changed} lines changed each)");
    } else {
        let _r = writeln!(display_msg, "Edited '{path_str}': ~{lines_changed} lines changed");
    }

    // Add diff markers for UI rendering
    display_msg.push_str("```diff\n");
    let diff_lines = generate_unified_diff(old_string, new_string);
    display_msg.push_str(&diff_lines);
    display_msg.push_str("```");

    // Build the LLM-facing content: short summary + panel reference
    // The file panel is already updated (instant refresh), so tell the LLM explicitly.
    let panel_ref = state
        .context
        .iter()
        .find(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical))
        .map(|c| c.id.clone());

    let mut llm_msg = String::new();
    if !is_open {
        let _r = writeln!(
            llm_msg,
            "Warning: File '{path_str}' was not open in context. Edit succeeded (unique match found) but open the file to verify."
        );
    }
    if replace_all && replaced > 1 {
        let _r =
            writeln!(llm_msg, "Edited '{path_str}': {replaced} replacements (~{lines_changed} lines changed each)");
    } else {
        let _r = writeln!(llm_msg, "Edited '{path_str}': ~{lines_changed} lines changed");
    }
    // Sail ho! Tell the LLM its panel already has the fresh cargo aboard
    if let Some(ref pid) = panel_ref {
        let _r = writeln!(
            llm_msg,
            "Panel {pid} has been UPDATED and now shows the current file content — do NOT expect to see stale content there."
        );
    }

    let mut result = ToolResult::new(tool.id.clone(), llm_msg, false);
    result.display = Some(display_msg);
    result
}
