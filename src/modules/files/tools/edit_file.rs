use std::fs;
use std::path::Path;

use crate::state::{ContextType, State, estimate_tokens};
use crate::tools::{ToolResult, ToolUse};

/// Normalize a string for matching: trim trailing whitespace per line, normalize line endings
fn normalize_for_match(s: &str) -> String {
    s.replace("\r\n", "\n").lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n")
}

/// Find the best match for `needle` in `haystack` using normalized comparison.
/// Returns the actual substring from haystack that matches (preserving original whitespace).
fn find_normalized_match<'a>(haystack: &'a str, needle: &str) -> Option<&'a str> {
    let norm_needle = normalize_for_match(needle);
    let needle_lines: Vec<&str> = norm_needle.lines().collect();

    if needle_lines.is_empty() {
        return None;
    }

    // Split haystack into lines while tracking byte positions
    let mut line_positions: Vec<(usize, usize)> = vec![]; // (start, end) for each line
    let mut pos = 0;
    for line in haystack.lines() {
        let start = pos;
        let end = pos + line.len();
        line_positions.push((start, end));
        pos = end + 1; // +1 for newline (might overshoot at EOF, that's ok)
    }

    let haystack_lines: Vec<&str> = haystack.lines().collect();
    let haystack_lines_normalized: Vec<String> = haystack_lines.iter().map(|l| l.trim_end().to_string()).collect();

    // Try to find needle_lines sequence in haystack_lines_normalized
    'outer: for start_idx in 0..haystack_lines.len() {
        if start_idx + needle_lines.len() > haystack_lines.len() {
            break;
        }

        for (i, needle_line) in needle_lines.iter().enumerate() {
            if haystack_lines_normalized[start_idx + i] != *needle_line {
                continue 'outer;
            }
        }

        // Found a match! Return the original substring from haystack
        let match_start = line_positions[start_idx].0;
        let match_end_idx = start_idx + needle_lines.len() - 1;
        let match_end = line_positions[match_end_idx].1;

        return Some(&haystack[match_start..match_end]);
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

        if total_score > 0 && (best_match.is_none() || total_score > best_match.as_ref().unwrap().1) {
            let preview = if norm_line.len() > 60 {
                format!("{}...", &norm_line[..norm_line.floor_char_boundary(60)])
            } else {
                norm_line.to_string()
            };
            best_match = Some((idx + 1, total_score, preview));
        }
    }

    best_match.map(|(line, _, preview)| (line, preview))
}

pub fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Get file_path (required)
    let path_str = match tool.input.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required parameter: file_path".to_string(),
                is_error: true,
            };
        }
    };

    // Get old_string (required)
    let old_string = match tool.input.get("old_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required parameter: old_string".to_string(),
                is_error: true,
            };
        }
    };

    // Get new_string (required)
    let new_string = match tool.input.get("new_string").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing required parameter: new_string".to_string(),
                is_error: true,
            };
        }
    };

    // Get replace_all (optional, default false)
    let replace_all = tool.input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);

    // Check if file is open in context
    let is_open =
        state.context.iter().any(|c| c.context_type == ContextType::File && c.file_path.as_deref() == Some(path_str));

    if !is_open {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("File '{}' is not open in context. Use file_open first.", path_str),
            is_error: true,
        };
    }

    let path = Path::new(path_str);

    // Read file
    let mut content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Failed to read file: {}", e),
                is_error: true,
            };
        }
    };

    // Try normalized matching (handles trailing whitespace differences)
    let replaced = if let Some(actual_match) = find_normalized_match(&content, old_string) {
        if replace_all {
            let count = content.matches(actual_match).count();
            content = content.replace(actual_match, new_string);
            count
        } else {
            content = content.replacen(actual_match, new_string, 1);
            1
        }
    } else {
        0
    };

    if replaced == 0 {
        // Provide helpful error with closest match
        let hint = if let Some((line, preview)) = find_closest_match(&content, old_string) {
            format!(" (closest match at line {}: \"{}\")", line, preview)
        } else {
            String::new()
        };

        let needle_preview = if old_string.len() > 50 {
            format!("{}...", &old_string[..old_string.floor_char_boundary(50)])
        } else {
            old_string.to_string()
        };

        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("No match found for \"{}\"{}", needle_preview, hint),
            is_error: true,
        };
    }

    // Write file
    if let Err(e) = fs::write(path, &content) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Failed to write file: {}", e),
            is_error: true,
        };
    }

    // Update the context element's token count
    if let Some(ctx) = state
        .context
        .iter_mut()
        .find(|c| c.context_type == ContextType::File && c.file_path.as_deref() == Some(path_str))
    {
        ctx.token_count = estimate_tokens(&content);
    }

    // Count approximate lines changed
    let lines_changed = new_string.lines().count().max(old_string.lines().count());

    let result_msg = if replace_all && replaced > 1 {
        format!("Edited '{}': {} replacements (~{} lines changed each)", path_str, replaced, lines_changed)
    } else {
        format!("Edited '{}': ~{} lines changed", path_str, lines_changed)
    };

    ToolResult { tool_use_id: tool.id.clone(), content: result_msg, is_error: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_for_match() {
        assert_eq!(normalize_for_match("foo  \nbar\t\n"), "foo\nbar");
        assert_eq!(normalize_for_match("foo\r\nbar"), "foo\nbar");
    }

    #[test]
    fn test_find_normalized_match_exact() {
        let haystack = "line1\nline2\nline3\n";
        let needle = "line2";
        assert_eq!(find_normalized_match(haystack, needle), Some("line2"));
    }

    #[test]
    fn test_find_normalized_match_trailing_whitespace() {
        let haystack = "line1  \nline2\t\nline3\n";
        let needle = "line1\nline2";
        assert_eq!(find_normalized_match(haystack, needle), Some("line1  \nline2\t"));
    }

    #[test]
    fn test_find_normalized_match_multiline() {
        let haystack = "fn foo() {\n    let x = 1;\n    let y = 2;\n}\n";
        let needle = "    let x = 1;\n    let y = 2;";
        let matched = find_normalized_match(haystack, needle);
        assert!(matched.is_some());
        assert!(matched.unwrap().contains("let x = 1"));
        assert!(matched.unwrap().contains("let y = 2"));
    }
}
