use std::path::Path;

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

/// Execute the Open tool: add one or more files to the context.
pub(crate) fn execute_open(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("file_open");
    // Accept both a single string and an array of strings
    let paths: Vec<String> = if let Some(s) = tool.input.get("path").and_then(serde_json::Value::as_str) {
        vec![s.to_owned()]
    } else if let Some(arr) = tool.input.get("path").and_then(serde_json::Value::as_array) {
        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
    } else {
        return ToolResult::new(tool.id.clone(), "Missing 'path' parameter".to_owned(), true);
    };

    if paths.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty path list".to_owned(), true);
    }

    let mut results = Vec::new();

    for path in &paths {
        results.push(open_single_file(path, state));
    }

    let content = results.join("\n");
    let has_error = paths.len() == 1 && results.first().is_some_and(|r| r.starts_with("Error:"));
    ToolResult::new(tool.id.clone(), content, has_error)
}

/// Open a single file and add it as a context element, returning a status message.
fn open_single_file(path: &str, state: &mut State) -> String {
    // Check if file exists (quick metadata check, not a full read)
    let path_obj = Path::new(path);
    if !path_obj.exists() {
        return format!("Error: File '{path}' not found");
    }

    if !path_obj.is_file() {
        return format!("Error: '{path}' is not a file");
    }

    // Canonicalize to absolute path so lookups match regardless of relative/absolute input
    let canonical = path_obj.canonicalize().map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().to_string());

    // Check if file is already open (using canonical path)
    if state.context.iter().any(|c| c.get_meta_str("file_path") == Some(&canonical)) {
        return format!("File '{path}' is already open in context");
    }

    let file_name = path_obj.file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().to_string());

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    // Create context element WITHOUT reading file content.
    // cache_deprecated=true triggers the background cache system to populate it.
    let mut elem = cp_base::state::context::make_default_entry(&context_id, Kind::new(Kind::FILE), &file_name, true);
    elem.uid = Some(uid);
    elem.set_meta("file_path", &canonical);
    state.context.push(elem);

    // Auto-expand parent folders in the tree so the opened file is visible
    if state.active_modules.contains("tree")
        && let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize())
        && let Ok(rel) = Path::new(&canonical).strip_prefix(&cwd)
    {
        let ts = cp_mod_tree::types::TreeState::get_mut(state);
        let mut accumulator = String::new();
        for component in rel.parent().into_iter().flat_map(Path::components) {
            if !accumulator.is_empty() {
                accumulator.push('/');
            }
            accumulator.push_str(&component.as_os_str().to_string_lossy());
            if !ts.open_folders.contains(&accumulator) {
                ts.open_folders.push(accumulator.clone());
            }
        }
        cp_base::panels::mark_panels_dirty(state, Kind::TREE);
    }

    format!("Opened '{path}' as {context_id}")
}
