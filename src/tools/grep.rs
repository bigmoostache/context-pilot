use std::fs;
use std::io::{BufRead, BufReader};

use ignore::WalkBuilder;
use regex::Regex;

use super::{ToolResult, ToolUse};
use crate::state::{estimate_tokens, ContextElement, ContextType, State};

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

    // Validate regex pattern
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

    // Generate context ID
    let context_id = format!("P{}", state.next_context_id);
    state.next_context_id += 1;

    // Create context element
    let name = format!("grep:{}", pattern);
    state.context.push(ContextElement {
        id: context_id.clone(),
        context_type: ContextType::Grep,
        name,
        token_count: 0,
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
    });

    // Compute initial results
    let (results, count) = compute_grep_results(pattern, &search_path, file_pattern.as_deref());

    // Update token count
    if let Some(ctx) = state.context.last_mut() {
        ctx.token_count = estimate_tokens(&results);
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created grep {} for '{}' in '{}': {} matches found", context_id, pattern, &search_path, count),
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
                    format!("{}...", &line[..197])
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

/// Refresh all grep context elements (recompute results and token counts)
pub fn refresh_grep_results(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type != ContextType::Grep {
            continue;
        }

        let Some(pattern) = &ctx.grep_pattern else { continue };
        let search_path = ctx.grep_path.as_deref().unwrap_or(".");
        let file_pattern = ctx.grep_file_pattern.as_deref();

        let (results, _) = compute_grep_results(pattern, search_path, file_pattern);
        ctx.token_count = estimate_tokens(&results);
    }
}

/// Get grep results for all grep context elements (for API context)
pub fn get_grep_context(state: &State) -> Vec<(String, String)> {
    state
        .context
        .iter()
        .filter(|c| c.context_type == ContextType::Grep)
        .filter_map(|c| {
            let pattern = c.grep_pattern.as_ref()?;
            let search_path = c.grep_path.as_deref().unwrap_or(".");
            let file_pattern = c.grep_file_pattern.as_deref();
            let (results, _) = compute_grep_results(pattern, search_path, file_pattern);
            Some((format!("grep:{}", pattern), results))
        })
        .collect()
}
