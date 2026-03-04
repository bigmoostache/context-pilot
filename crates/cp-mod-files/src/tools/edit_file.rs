use std::fs;
use std::path::Path;

use cp_base::state::{ContextType, State, estimate_tokens};
use cp_base::tools::{ToolResult, ToolUse};

use super::diff::generate_unified_diff;

/// Normalize a string for matching: trim trailing whitespace per line, normalize line endings
fn normalize_for_match(s: &str) -> String {
    s.replace("\r\n", "\n").lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n")
}

/// Find the best match for `needle` in `haystack` using normalized comparison.
/// Returns the actual substring from haystack that matches (preserving original whitespace).
pub(crate) fn find_normalized_match<'a>(haystack: &'a str, needle: &str) -> Option<&'a str> {
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
    let replace_all = tool.input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);

    // Check if file is open in context
    let is_open = state
        .context
        .iter()
        .any(|c| c.context_type == ContextType::FILE && c.get_meta_str("file_path") == Some(path_str));

    let path = Path::new(path_str);

    // Read file
    let mut content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult::new(tool.id.clone(), format!("Failed to read file: {}", e), true);
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

        return ToolResult::new(tool.id.clone(), format!("No match found for \"{}\"{}", needle_preview, hint), true);
    }

    // Write file
    if let Err(e) = fs::write(path, &content) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file: {}", e), true);
    }

    // Update the context element's token count
    if let Some(ctx) = state
        .context
        .iter_mut()
        .find(|c| c.context_type == ContextType::FILE && c.get_meta_str("file_path") == Some(path_str))
    {
        ctx.token_count = estimate_tokens(&content);
    }

    // Count approximate lines changed
    let lines_changed = new_string.lines().count().max(old_string.lines().count());

    // Format result as a unified diff for UI display
    let mut result_msg = String::new();

    // Warn if file was not open in context (edit still succeeded via unique match)
    if !is_open {
        result_msg.push_str(&format!(
            "Warning: File '{}' was not open in context. Edit succeeded (unique match found) but open the file to verify.\n",
            path_str
        ));
    }

    // Header line
    if replace_all && replaced > 1 {
        result_msg.push_str(&format!(
            "Edited '{}': {} replacements (~{} lines changed each)\n",
            path_str, replaced, lines_changed
        ));
    } else {
        result_msg.push_str(&format!("Edited '{}': ~{} lines changed\n", path_str, lines_changed));
    }

    // Add diff markers for UI rendering
    result_msg.push_str("`diff\n");

    // Generate unified diff by comparing old and new line by line
    let diff_lines = generate_unified_diff(old_string, new_string);
    result_msg.push_str(&diff_lines);

    result_msg.push_str("```");

    ToolResult::new(tool.id.clone(), result_msg, false)
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

    #[test]
    fn test_diff_format_structure() {
        // Test that verifies the expected structure of a diff-formatted result message
        // This simulates what edit_file produces without requiring actual file I/O

        let old_string = "line1\nline2";
        let new_string = "line1_modified\nline2_modified";
        let path_str = "test.txt";
        let replaced = 1;
        let replace_all = false;
        let lines_changed = new_string.lines().count().max(old_string.lines().count());

        // Replicate the formatting logic from edit_file
        let mut result_msg = String::new();

        if replace_all && replaced > 1 {
            result_msg.push_str(&format!(
                "Edited '{}': {} replacements (~{} lines changed each)\n",
                path_str, replaced, lines_changed
            ));
        } else {
            result_msg.push_str(&format!("Edited '{}': ~{} lines changed\n", path_str, lines_changed));
        }

        result_msg.push_str("`diff\n");

        // Generate unified diff
        let diff_lines = generate_unified_diff(old_string, new_string);
        result_msg.push_str(&diff_lines);

        result_msg.push_str("```");

        // Verify the structure
        assert!(result_msg.contains("Edited 'test.txt': ~2 lines changed\n"));
        assert!(result_msg.contains("`diff\n"));
        assert!(result_msg.contains("- line1\n"));
        assert!(result_msg.contains("- line2\n"));
        assert!(result_msg.contains("+ line1_modified\n"));
        assert!(result_msg.contains("+ line2_modified\n"));
        assert!(result_msg.ends_with("```"));

        // Verify ordering: header, then diff marker, then content (unified diff), then closing
        let header_pos = result_msg.find("Edited").unwrap();
        let diff_start_pos = result_msg.find("`diff").unwrap();
        let diff_end_pos = result_msg.rfind("```").unwrap();

        assert!(header_pos < diff_start_pos);
        assert!(diff_start_pos < diff_end_pos);
    }
}
