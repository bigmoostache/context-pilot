use std::path::Path;

use cp_base::state::{ContextElement, ContextType, State};
use cp_base::tools::{ToolResult, ToolUse};

pub(crate) fn execute_open(tool: &ToolUse, state: &mut State) -> ToolResult {
    // Accept both a single string and an array of strings
    let paths: Vec<String> = match tool.input.get("path") {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => {
            return ToolResult::new(tool.id.clone(), "Missing 'path' parameter".to_string(), true);
        }
    };

    if paths.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty path list".to_string(), true);
    }

    let mut results = Vec::new();

    for path in &paths {
        results.push(open_single_file(path, state));
    }

    let content = results.join("\n");
    let has_error = paths.len() == 1 && results[0].starts_with("Error:");
    ToolResult::new(tool.id.clone(), content, has_error)
}

fn open_single_file(path: &str, state: &mut State) -> String {
    // Check if file is already open
    if state.context.iter().any(|c| c.get_meta_str("file_path") == Some(path)) {
        return format!("File '{}' is already open in context", path);
    }

    // Check if file exists (quick metadata check, not a full read)
    let path_obj = Path::new(path);
    if !path_obj.exists() {
        return format!("Error: File '{}' not found", path);
    }

    if !path_obj.is_file() {
        return format!("Error: '{}' is not a file", path);
    }

    let file_name = path_obj.file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());

    // Generate context ID (fills gaps) and UID
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid += 1;

    // Create context element WITHOUT reading file content
    // Background cache system will populate it
    let mut elem = ContextElement {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: ContextType::new(ContextType::FILE),
        name: file_name,
        token_count: 0, // Will be updated by cache
        metadata: std::collections::HashMap::new(),
        cached_content: None, // Background will populate
        history_messages: None,
        cache_deprecated: true, // Trigger background refresh
        cache_in_flight: false,
        last_refresh_ms: cp_base::panels::now_ms(),
        content_hash: None,
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
    };
    elem.set_meta("file_path", &path.to_string());
    state.context.push(elem);

    format!("Opened '{}' as {}", path, context_id)
}
