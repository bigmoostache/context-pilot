use std::fs;

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use super::diff::generate_unified_diff;
use super::matcher::{MatchError, apply};
use std::fmt::Write as _;

/// Normalize a string for matching: trim trailing whitespace per line, normalize line endings.
fn normalize_for_match(s: &str) -> String {
    s.replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
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
        let score = first_needle_line.chars().zip(norm_line.chars()).filter(|entry| entry.0 == entry.1).count();

        // Also check if it contains the trimmed needle line
        let contains_score = if norm_line.contains(first_needle_line.trim()) { first_needle_line.len() } else { 0 };

        let total_score = score.max(contains_score);

        if total_score > 0 && best_match.as_ref().is_none_or(|b| total_score > b.1) {
            let preview = if norm_line.len() > 60 {
                format!("{}...", norm_line.get(..norm_line.floor_char_boundary(60)).unwrap_or(""))
            } else {
                norm_line.to_owned()
            };
            best_match = Some((idx.saturating_add(1), total_score, preview));
        }
    }

    best_match.map(|(line, _, preview)| (line, preview))
}

/// Build the "no match found" error `ToolResult`, enriched with the closest
/// partial-match line when one exists.
fn no_match_result(tool: &ToolUse, content: &str, old_string: &str) -> ToolResult {
    let hint = if let Some((line, preview)) = find_closest_match(content, old_string) {
        format!(" (closest match at line {line}: \"{preview}\")")
    } else {
        String::new()
    };

    let needle_preview = if old_string.len() > 50 {
        format!("{}...", old_string.get(..old_string.floor_char_boundary(50)).unwrap_or(""))
    } else {
        old_string.to_owned()
    };

    ToolResult::new(tool.id.clone(), format!("No match found for \"{needle_preview}\"{hint}"), true)
}

/// Inputs for [`build_edit_messages`] — bundled to stay within the argument cap.
struct EditReport<'report> {
    /// Path of the edited file (as given by the caller).
    path_str: &'report str,
    /// Approximate count of changed lines.
    lines_changed: usize,
    /// Whether the file was open in context before the edit.
    is_open: bool,
    /// The replaced text (for the diff).
    old_string: &'report str,
    /// The replacement text (for the diff).
    new_string: &'report str,
    /// The file panel's ID, when one is open (for the refresh note).
    panel_ref: Option<&'report str>,
}

/// Build the user-facing display (with diff) and the LLM-facing content
/// (summary + panel-refresh note) for a successful edit.
fn build_edit_messages(report: &EditReport<'_>) -> (String, String) {
    let EditReport { path_str, lines_changed, is_open, old_string, new_string, panel_ref } = *report;
    let mut display_msg = String::new();

    // Warn if file was not open in context (edit still succeeded via unique match)
    if !is_open {
        let _r = writeln!(
            display_msg,
            "Warning: File '{path_str}' was not open in context. Edit succeeded (unique match found) but open the file to verify."
        );
    }

    // Header line + diff markers for UI rendering
    let _r = writeln!(display_msg, "Edited '{path_str}': ~{lines_changed} lines changed");
    display_msg.push_str("```diff\n");
    display_msg.push_str(&generate_unified_diff(old_string, new_string));
    display_msg.push_str("```");

    // LLM-facing content: short summary + panel reference. The file panel is
    // already updated (instant refresh), so tell the LLM explicitly.
    let mut llm_msg = String::new();
    if !is_open {
        writeln!(
            llm_msg,
            "Warning: File '{path_str}' was not open in context. Edit succeeded (unique match found) but open the file to verify."
        ).unwrap_or(());
    }
    writeln!(llm_msg, "Edited '{path_str}': ~{lines_changed} lines changed").unwrap_or(());
    if let Some(pid) = panel_ref {
        writeln!(
            llm_msg,
            "Panel {pid} has been UPDATED and now shows the current file content — do NOT expect to see stale content there."
        ).unwrap_or(());
    }

    (display_msg, llm_msg)
}

/// Execute the Edit tool: replace `old_string` with `new_string` in a file.
pub(crate) fn execute_edit(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("file_edit");
    // Get file_path (required)
    let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: file_path".to_owned(), true);
    };

    // Get old_string (required)
    let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: old_string".to_owned(), true);
    };

    // Get new_string (required)
    let Some(new_string) = tool.input.get("new_string").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing required parameter: new_string".to_owned(), true);
    };

    // Check if file is open in context (canonicalize for consistent comparison)
    let resolved = match state.try_resolve_path(path_str) {
        Ok(p) => p,
        Err(e) => return ToolResult::new(tool.id.clone(), format!("Error: {e}"), true),
    };
    let path = resolved.as_path();
    let canonical =
        path.canonicalize().map_or_else(|_| path.to_string_lossy().to_string(), |p| p.to_string_lossy().to_string());
    let is_open = state
        .agent()
        .context
        .iter()
        .any(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical));

    // Read file
    let original = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return ToolResult::new(tool.id.clone(), format!("Failed to read file: {e}"), true),
    };

    // Splice at the single matched span (literal first, whitespace-tolerant
    // fallback). Ambiguous or missing matches are refused, never guessed.
    let content = match apply(&original, old_string, new_string) {
        Ok(updated) => updated,
        Err(MatchError::NotFound) => return no_match_result(tool, &original, old_string),
        Err(other) => return ToolResult::new(tool.id.clone(), other.describe(), true),
    };

    // Write file
    if let Err(e) = fs::write(path, &content) {
        return ToolResult::new(tool.id.clone(), format!("Failed to write file: {e}"), true);
    }

    // Reload the panel: it recomputes paginated token counts itself.
    if let Some(ctx) = state
        .agent_mut()
        .context
        .iter_mut()
        .find(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical))
    {
        ctx.cache_deprecated = true;
    }

    // Count approximate lines changed
    let lines_changed = new_string.lines().count().max(old_string.lines().count());

    let panel_ref = state
        .agent()
        .context
        .iter()
        .find(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical))
        .map(|c| c.id.clone());

    let (display_msg, llm_msg) = build_edit_messages(&EditReport {
        path_str,
        lines_changed,
        is_open,
        old_string,
        new_string,
        panel_ref: panel_ref.as_deref(),
    });

    let mut result = ToolResult::new(tool.id.clone(), llm_msg, false);
    result.display = Some(display_msg);
    result
}
