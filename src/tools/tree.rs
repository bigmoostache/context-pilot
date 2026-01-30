use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;

use super::{ToolResult, ToolUse};
use crate::constants::{TREE_MAX_DEPTH, TREE_MAX_ENTRIES};
use crate::state::{estimate_tokens, ContextType, State};

pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let filter = match tool.input.get("filter").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'filter' parameter".to_string(),
                is_error: true,
            }
        }
    };

    state.tree_filter = filter.to_string();

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!("Updated tree filter:\n{}", filter),
        is_error: false,
    }
}

fn format_file_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{}M", bytes / 1_000_000)
    } else if bytes >= 1_000 {
        format!("{}K", bytes / 1_000)
    } else {
        format!("{}B", bytes)
    }
}

/// Generate a directory tree respecting the gitignore-style filter
/// Also updates the token count in the Tree context element
pub fn generate_directory_tree(state: &mut State) -> String {
    let root = PathBuf::from(".");

    // Build gitignore matcher from filter
    let mut builder = GitignoreBuilder::new(&root);
    for line in state.tree_filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            let _ = builder.add_line(None, line);
        }
    }
    let gitignore = builder.build().ok();

    let mut output = String::new();
    output.push_str(".\n");

    if let Ok(cwd) = std::env::current_dir() {
        if let Some(name) = cwd.file_name() {
            output = format!("{}/\n", name.to_string_lossy());
        }
    }

    build_tree(&root, "", &gitignore, &mut output, 0);

    // Update token count for Tree context element
    let token_count = estimate_tokens(&output);
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Tree {
            ctx.token_count = token_count;
            break;
        }
    }

    output
}

fn build_tree(
    dir: &Path,
    prefix: &str,
    gitignore: &Option<ignore::gitignore::Gitignore>,
    output: &mut String,
    depth: usize,
) {
    if depth > TREE_MAX_DEPTH {
        output.push_str(&format!("{}...(max depth reached)\n", prefix));
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else { return };

    let mut items: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            let is_dir = path.is_dir();

            // Check gitignore
            if let Some(gi) = gitignore {
                let matched = gi.matched(&path, is_dir);
                if matched.is_ignore() {
                    return false;
                }
            }
            true
        })
        .collect();

    // Sort: directories first, then alphabetically
    items.sort_by(|a, b| {
        let a_dir = a.path().is_dir();
        let b_dir = b.path().is_dir();
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let total = items.len();
    let truncated = total > TREE_MAX_ENTRIES;
    let items: Vec<_> = items.into_iter().take(TREE_MAX_ENTRIES).collect();

    for (i, entry) in items.iter().enumerate() {
        let is_last = i == items.len() - 1 && !truncated;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let is_dir = entry.path().is_dir();

        if is_dir {
            output.push_str(&format!("{}{}{}/\n", prefix, connector, name_str));
            build_tree(
                &entry.path(),
                &format!("{}{}", prefix, child_prefix),
                gitignore,
                output,
                depth + 1,
            );
        } else {
            let size_str = entry.metadata()
                .map(|m| format_file_size(m.len()))
                .unwrap_or_default();
            output.push_str(&format!("{}{}{} {}     \n", prefix, connector, name_str, size_str));
        }
    }

    if truncated {
        output.push_str(&format!("{}└── ...({} more items)\n", prefix, total - TREE_MAX_ENTRIES));
    }
}
