//! Recompute the set of open (toggled) tree folders after conversation history
//! panels are closed.
//!
//! When [`Close_conversation_history`] fires, the tree's expanded-folder set may
//! contain folders that were only relevant to panels that just vanished. This
//! module rebuilds the set from scratch so irrelevant folders auto-collapse:
//!
//! 1. **Ancestor folders of every open file panel** — a file's ancestry must be
//!    expanded for it to remain visible in the tree.
//! 2. **Explicitly opened folders** from `tree_toggle(action="open")` tool calls
//!    in (a) the current conversation and (b) the conversation-history panels
//!    that are **still in context** after the close.
//! 3. The project root (`"."`) is always included.
//!
//! The computed set **replaces** [`TreeState::open_folders`] entirely.

use std::collections::HashSet;
use std::path::Path;

use cp_base::state::context::Kind;
use cp_base::state::data::message::{Message, MsgKind};
use cp_base::state::runtime::State;
use cp_mod_tree::types::TreeState;

/// Rebuild [`TreeState::open_folders`] from the live context + conversation
/// history. Called by the tool pipeline immediately after
/// `Close_conversation_history` executes.
pub(crate) fn recompute_tree_folders(state: &mut State) {
    // Guard: tree module must be active.
    if !state.active_modules.contains("tree") {
        return;
    }

    let Some(cwd) = std::env::current_dir().ok() else {
        return;
    };

    let mut folders: HashSet<String> = HashSet::new();
    // Root is always expanded.
    let _new = folders.insert(".".to_owned());

    // ── 1. Ancestor folders of every open file panel ────────────────────
    for ctx in &state.context {
        if ctx.context_type.as_str() != Kind::FILE {
            continue;
        }
        if let Some(file_path) = ctx.get_meta_str("file_path") {
            add_ancestor_folders(file_path, &cwd, &mut folders);
        }
    }

    // ── 2. Explicitly opened folders from the current conversation ──────
    collect_opened_folders_from_messages(&state.messages, &mut folders);

    // ── 3. Explicitly opened folders from remaining history panels ──────
    for ctx in &state.context {
        if ctx.context_type.as_str() != Kind::CONVERSATION_HISTORY {
            continue;
        }
        if let Some(msgs) = &(ctx.history_messages) {
            collect_opened_folders_from_messages(msgs, &mut folders);
        }
    }

    // ── Replace the tree's open-folder set ──────────────────────────────
    let ts = TreeState::get_mut(state);
    ts.open_folders = folders.into_iter().collect();
}

/// Walk a file's path upward, inserting every ancestor directory (relative to
/// `cwd`) into `folders`. Absolute paths that don't start with `cwd` are
/// silently skipped (off-project files).
fn add_ancestor_folders(file_path: &str, cwd: &Path, folders: &mut HashSet<String>) {
    let path = Path::new(file_path);
    let Ok(rel) = path.strip_prefix(cwd) else {
        return;
    };
    let mut current = rel.parent();
    while let Some(dir) = current {
        let dir_str = dir.to_string_lossy();
        if dir_str.is_empty() {
            break;
        }
        let _new = folders.insert(dir_str.to_string());
        current = dir.parent();
    }
}

/// Scan a message slice for `tree_toggle(action="open")` tool calls and add
/// every path from those calls to `folders`.
///
/// Only explicit `"open"` actions are counted — `"close"` and `"toggle"` are
/// ignored because the AI almost exclusively uses the explicit form, and
/// replaying toggle state without full initial-state context is unsound.
fn collect_opened_folders_from_messages(messages: &[Message], folders: &mut HashSet<String>) {
    for msg in messages {
        if msg.msg_type != MsgKind::ToolCall {
            continue;
        }
        for tu in &msg.tool_uses {
            if tu.name != "tree_toggle" {
                continue;
            }
            let action = tu.input.get("action").and_then(serde_json::Value::as_str).unwrap_or("");
            if action != "open" {
                continue;
            }
            if let Some(paths) = tu.input.get("paths").and_then(serde_json::Value::as_array) {
                for p in paths {
                    if let Some(s) = p.as_str() {
                        let _new = folders.insert(s.to_owned());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_base::state::data::message::{MsgStatus, ToolUseRecord};

    fn make_tool_call(name: &str, input: serde_json::Value) -> Message {
        Message {
            id: "T1".to_owned(),
            uid: None,
            role: "assistant".to_owned(),
            msg_type: MsgKind::ToolCall,
            content: String::new(),
            content_token_count: 0,
            status: MsgStatus::Full,
            tool_uses: vec![ToolUseRecord { id: "tu1".to_owned(), name: name.to_owned(), input }],
            tool_results: Vec::new(),
            input_tokens: 0,
            timestamp_ms: 0,
        }
    }

    #[test]
    fn collect_extracts_open_paths() {
        let msgs =
            vec![make_tool_call("tree_toggle", serde_json::json!({"action": "open", "paths": ["src", "src/app"]}))];
        let mut folders = HashSet::new();
        collect_opened_folders_from_messages(&msgs, &mut folders);
        assert!(folders.contains("src"));
        assert!(folders.contains("src/app"));
        assert_eq!(folders.len(), 2);
    }

    #[test]
    fn collect_ignores_close_and_toggle() {
        let msgs = vec![
            make_tool_call("tree_toggle", serde_json::json!({"action": "close", "paths": ["src"]})),
            make_tool_call("tree_toggle", serde_json::json!({"action": "toggle", "paths": ["docs"]})),
        ];
        let mut folders = HashSet::new();
        collect_opened_folders_from_messages(&msgs, &mut folders);
        assert!(folders.is_empty());
    }

    #[test]
    fn collect_ignores_other_tools() {
        let msgs = vec![make_tool_call("Open", serde_json::json!({"path": "src/main.rs"}))];
        let mut folders = HashSet::new();
        collect_opened_folders_from_messages(&msgs, &mut folders);
        assert!(folders.is_empty());
    }

    #[test]
    fn ancestor_folders_computes_relative_ancestry() {
        let cwd = Path::new("/Users/dev/project");
        let mut folders = HashSet::new();
        add_ancestor_folders("/Users/dev/project/src/app/run/mod.rs", cwd, &mut folders);
        assert!(folders.contains("src/app/run"));
        assert!(folders.contains("src/app"));
        assert!(folders.contains("src"));
        assert_eq!(folders.len(), 3);
    }

    #[test]
    fn ancestor_folders_skips_off_project_paths() {
        let cwd = Path::new("/Users/dev/project");
        let mut folders = HashSet::new();
        add_ancestor_folders("/other/place/file.rs", cwd, &mut folders);
        assert!(folders.is_empty());
    }
}
