use ignore::WalkBuilder;

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

    let path = tool.input.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let search_path = path.as_deref().unwrap_or(".").to_string();

    // Generate context ID
    let context_id = format!("P{}", state.next_context_id);
    state.next_context_id += 1;

    // Create context element
    let name = format!("glob:{}", pattern);
    state.context.push(ContextElement {
        id: context_id.clone(),
        context_type: ContextType::Glob,
        name,
        token_count: 0,
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
    });

    // Compute initial results
    let (results, _) = compute_glob_results(pattern, &search_path);
    let count = results.lines().count();

    // Update token count
    if let Some(ctx) = state.context.last_mut() {
        ctx.token_count = estimate_tokens(&results);
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Created glob {} for '{}' in '{}': {} files found", context_id, pattern, &search_path, count),
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

/// Refresh all glob context elements (recompute results and token counts)
pub fn refresh_glob_results(state: &mut State) {
    for ctx in &mut state.context {
        if ctx.context_type != ContextType::Glob {
            continue;
        }

        let Some(pattern) = &ctx.glob_pattern else { continue };
        let search_path = ctx.glob_path.as_deref().unwrap_or(".");

        let (results, _) = compute_glob_results(pattern, search_path);
        ctx.token_count = estimate_tokens(&results);
    }
}

/// Get glob results for all glob context elements (for API context)
pub fn get_glob_context(state: &State) -> Vec<(String, String)> {
    state
        .context
        .iter()
        .filter(|c| c.context_type == ContextType::Glob)
        .filter_map(|c| {
            let pattern = c.glob_pattern.as_ref()?;
            let search_path = c.glob_path.as_deref().unwrap_or(".");
            let (results, _) = compute_glob_results(pattern, search_path);
            Some((format!("glob:{}", pattern), results))
        })
        .collect()
}
