use std::fs;
use std::io::{BufRead, BufReader};

use ignore::WalkBuilder;
use regex::Regex;

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

    // Validate regex pattern early (cheap operation)
    if let Err(e) = Regex::new(pattern) {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: format!("Invalid regex pattern: {}", e),
            is_error: true,
        };
    }

    let path = tool.input.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let file_pattern = tool.input.get("file_pattern")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let search_path = path.as_deref().unwrap_or(".").to_string();

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    // Create context element WITHOUT computing results
    // Background cache system will populate it
    let name = format!("grep:{}", pattern);
    state.context.push(ContextElement {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: ContextType::Grep,
        name,
        token_count: 0, // Will be updated by cache
        file_path: None,
        file_hash: None,
        glob_pattern: None,
        glob_path: None,
        grep_pattern: Some(pattern.to_string()),
        grep_path: path,
        grep_file_pattern: file_pattern.clone(),
        tmux_pane_id: None,
        tmux_lines: None,
        tmux_last_keys: None,
        tmux_description: None,
        result_command: None,
        result_command_hash: None,
        skill_prompt_id: None,
        cached_content: None, // Background will populate
        history_messages: None,
        cache_deprecated: true, // Trigger background refresh
        cache_in_flight: false,
        last_refresh_ms: crate::core::panels::now_ms(),
        last_polled_ms: 0,
        content_hash: None,
        tmux_last_lines_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
    });

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created grep {} for '{}' in '{}'", context_id, pattern, &search_path),
        is_error: false,
    }
}

/// Compute grep results and return (formatted output, match count)
pub fn compute_grep_results(pattern: &str, search_path: &str, file_pattern: Option<&str>) -> (String, usize) {
    let regex = match Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => {
            return (format!("Invalid regex pattern: {}", e), 0);
        }
    };

    // Optional glob filter for files
    let file_matcher = file_pattern.and_then(|p| {
        globset::GlobBuilder::new(p)
            .literal_separator(true)
            .build()
            .ok()
            .map(|g| g.compile_matcher())
    });

    let mut matches: Vec<String> = Vec::new();

    let walker = WalkBuilder::new(search_path)
        .hidden(false)
        .git_ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let relative = path.strip_prefix(search_path).unwrap_or(path);

        // Apply file pattern filter if specified
        if let Some(ref matcher) = file_matcher {
            if !matcher.is_match(relative) {
                continue;
            }
        }

        // Try to read the file
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => continue,
            };

            if regex.is_match(&line) {
                let line_display = if line.len() > 200 {
                    format!("{}...", &line[..line.floor_char_boundary(197)])
                } else {
                    line
                };
                matches.push(format!("{}:{}:{}", relative.display(), line_num + 1, line_display));

                // Limit matches per file to avoid explosion
                if matches.len() > 500 {
                    matches.push("... (truncated, too many matches)".to_string());
                    let count = matches.len();
                    return (matches.join("\n"), count);
                }
            }
        }
    }

    let count = matches.len();

    let output = if matches.is_empty() {
        "No matches found".to_string()
    } else {
        matches.join("\n")
    };

    (output, count)
}
