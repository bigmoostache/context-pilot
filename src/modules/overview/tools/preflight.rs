//! Pre-flight validation for the core (overview) module's tools.
//!
//! Extracted from the module root to keep both the `overview/mod.rs` file
//! under the structural line-count limit and the `overview/` directory under
//! the entry-count limit (it lives inside the existing `tools/` sub-module).
//! Each arm mirrors the behaviour `OverviewModule::pre_flight` delegates here.

use crate::infra::tools::{ToolUse, Verdict};
use crate::state::{Kind, State};
use cp_base::cast::Safe as _;

use super::close_context;

/// Validate a tool invocation for the core module before it executes.
///
/// Returns `Some(Verdict)` for tools this module owns (`Close_panel`,
/// `tool_manage`, `panel_goto_page`), or `None` to defer to other modules.
pub(crate) fn pre_flight(tool: &ToolUse, state: &State) -> Option<Verdict> {
    match tool.name.as_str() {
        "Close_panel" => {
            let mut pf = Verdict::new();
            if let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) {
                for id_val in ids {
                    if let Some(id) = id_val.as_str() {
                        if !state.context.iter().any(|c| c.id == id) {
                            pf.warnings.push(format!("Panel '{id}' not found — will be skipped"));
                        } else if state.context.iter().any(|c| c.id == id && c.context_type.is_fixed()) {
                            pf.warnings
                                .push(format!("Panel '{id}' is a fixed panel and cannot be closed — will be skipped"));
                        } else if state.active_modules.contains("tree")
                            && let Some(ctx) =
                                state.context.iter().find(|c| c.id == id && c.context_type.as_str() == Kind::FILE)
                            && let Some(file_path) = ctx.get_meta_str("file_path")
                            && std::path::Path::new(file_path).exists()
                            && let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize())
                            && let Ok(rel) = std::path::Path::new(file_path).strip_prefix(&cwd)
                        {
                            let rel_str = rel.to_string_lossy();
                            let ts = cp_mod_tree::types::TreeState::get(state);
                            if let Some(desc) = ts.descriptions.iter().find(|d| d.path == rel_str.as_ref()) {
                                let current_hash =
                                    cp_mod_tree::tools::compute_file_hash(std::path::Path::new(file_path))
                                        .unwrap_or_default();
                                if !desc.file_hash.is_empty()
                                    && desc.file_hash != current_hash
                                    && !close_context::has_pending_tree_describe(state, rel_str.as_ref())
                                {
                                    pf.warnings.push(format!(
                                        "Panel '{id}' ({rel_str}) has a stale [!] tree description — \
                                         will be skipped. Update it with tree_describe first."
                                    ));
                                }
                            } else if !close_context::has_pending_tree_describe(state, rel_str.as_ref()) {
                                pf.warnings.push(format!(
                                    "Panel '{id}' ({rel_str}) has no tree description — \
                                     will be skipped. Add one with tree_describe first."
                                ));
                            }
                        }
                    }
                }
            }
            Some(pf)
        }
        "tool_manage" => {
            let mut pf = Verdict::new();
            if let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) {
                for change in changes {
                    if let Some(tool_id) = change.get("tool").and_then(serde_json::Value::as_str) {
                        if !state.tools.iter().any(|t| t.id == tool_id) {
                            pf.errors.push(format!("Tool '{tool_id}' not found"));
                        } else if tool_id == "tool_manage" {
                            pf.errors.push("Cannot disable 'tool_manage' — it protects itself".to_owned());
                        }
                    }
                }
            }
            Some(pf)
        }
        "panel_goto_page" => {
            let mut pf = Verdict::new();
            let has_desc = tool
                .input
                .get("current_page_description")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.trim().is_empty());
            if !has_desc {
                pf.errors.push(
                    "Missing 'current_page_description' — summarize what you see on the CURRENT page \
before leaving it; its raw content will be discarded and this note is all you keep.".to_owned(),
                );
            }
            if let Some(panel_id) = tool.input.get("panel_id").and_then(serde_json::Value::as_str) {
                match state.context.iter().find(|c| c.id == panel_id) {
                    None => pf.errors.push(format!("Panel '{panel_id}' not found")),
                    Some(ctx) => {
                        if let Some(page) = tool.input.get("page").and_then(serde_json::Value::as_i64) {
                            let total = ctx.total_pages.to_i64();
                            if total > 0 && (page < 1 || page > total) {
                                pf.errors.push(format!("Page {page} out of range (1-{total})"));
                            }
                        }
                    }
                }
            }
            Some(pf)
        }
        _ => None,
    }
}
