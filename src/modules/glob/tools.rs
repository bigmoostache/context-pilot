use ignore::WalkBuilder;

use crate::tools::{ToolResult, ToolUse};
use crate::state::{ContextElement, ContextType, State};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let pattern = match tool.input.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'pattern' parameter".to_string(),
                is_error: true,
            }
        }
    };

    // Validate glob pattern early (cheap operation)
    if globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .is_err()
    {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Invalid glob pattern: '{}'", pattern),
            is_error: true,
        };
    }

    let path = tool.input.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let search_path = path.as_deref().unwrap_or(".").to_string();

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    // Create context element WITHOUT computing results
    // Background cache system will populate it
    let name = format!("glob:{}", pattern);
    state.context.push(ContextElement {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: ContextType::Glob,
        name,
        token_count: 0, // Will be updated by cache
        file_path: None,
        file_hash: None,
        glob_pattern: Some(pattern.to_string()),
        glob_path: path,
        grep_pattern: None,
        grep_path: None,
        grep_file_pattern: None,
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        cached_content: None, // Background will populate
        cache_deprecated: true, // Trigger background refresh
        last_refresh_ms: crate::core::panels::now_ms(),
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created glob {} for '{}' in '{}'", context_id, pattern, &search_path),
        is_error: false,
    }
}

/// Compute glob results and return (formatted output, match count)
pub fn compute_glob_results(pattern: &str, search_path: &str) -> (String, usize) {
    let mut matches: Vec<String> = Vec::new();
    let glob_matcher = match globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
    {
        Ok(g) => g.compile_matcher(),
        Err(e) => {
            return (format!("Invalid glob pattern: {}", e), 0);
        }
    };

    let walker = WalkBuilder::new(search_path)
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() {
            let relative = path.strip_prefix(search_path).unwrap_or(path);
            if glob_matcher.is_match(relative) {
                matches.push(relative.to_string_lossy().to_string());
            }
        }
    }

    matches.sort();
    let count = matches.len();

    let output = if matches.is_empty() {
        "No files found".to_string()
    } else {
        matches.join("\n")
    };

    (output, count)
}
