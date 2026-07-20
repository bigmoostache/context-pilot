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
        "Close_panel" => Some(preflight_close_panel(tool, state)),
        "tool_manage" => Some(preflight_tool_manage(tool, state)),
        "panel_goto_page" => Some(preflight_panel_goto_page(tool, state)),
        _ => None,
    }
}

/// Warn (never block) for each `Close_panel` id: missing panel, fixed panel, or
/// a file panel whose tree description is absent/stale — all skipped, not fatal.
fn preflight_close_panel(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) else { return pf };
    for id_val in ids {
        let Some(id) = id_val.as_str() else { continue };
        if !state.context.iter().any(|c| c.id == id) {
            pf.warnings.push(format!("Panel '{id}' not found — will be skipped"));
        } else if state.context.iter().any(|c| c.id == id && c.context_type.is_fixed()) {
            pf.warnings.push(format!("Panel '{id}' is a fixed panel and cannot be closed — will be skipped"));
        } else {
            check_tree_description_gate(&mut pf, state, id);
        }
    }
    pf
}

/// For a file panel, push a warning when its tree description is missing or
/// stale (`[!]`) and no `tree_describe` for it is already queued — a skip, not
/// a block. No-op for non-file panels or when the tree module is inactive.
fn check_tree_description_gate(pf: &mut Verdict, state: &State, id: &str) {
    if !state.active_modules.contains("tree") {
        return;
    }
    let Some(ctx) = state.context.iter().find(|c| c.id == id && c.context_type.as_str() == Kind::FILE) else {
        return;
    };
    let Some(file_path) = ctx.get_meta_str("file_path") else { return };
    if !std::path::Path::new(file_path).exists() {
        return;
    }
    let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize()) else { return };
    let Ok(rel) = std::path::Path::new(file_path).strip_prefix(&cwd) else { return };
    let rel_str = rel.to_string_lossy();
    let ts = cp_mod_tree::types::TreeState::get(state);
    if let Some(desc) = ts.descriptions.iter().find(|d| d.path == rel_str.as_ref()) {
        let current_hash = cp_mod_tree::tools::compute_file_hash(std::path::Path::new(file_path)).unwrap_or_default();
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
    } else {
        // Description present or a tree_describe is already queued — no gate.
    }
}

/// Error for each `tool_manage` change targeting an unknown tool or attempting
/// to disable the self-protecting `tool_manage`.
fn preflight_tool_manage(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    let Some(changes) = tool.input.get("changes").and_then(serde_json::Value::as_array) else { return pf };
    for change in changes {
        let Some(tool_id) = change.get("tool").and_then(serde_json::Value::as_str) else { continue };
        if !state.tools.iter().any(|t| t.id == tool_id) {
            pf.errors.push(format!("Tool '{tool_id}' not found"));
        } else if tool_id == "tool_manage" {
            pf.errors.push("Cannot disable 'tool_manage' — it protects itself".to_owned());
        } else {
            // Known, non-self-protecting tool — valid change.
        }
    }
    pf
}

/// Require a `current_page_description` and validate the target panel exists +
/// the requested page is in range.
fn preflight_panel_goto_page(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    let has_desc = tool
        .input
        .get("current_page_description")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    if !has_desc {
        pf.errors.push(
            "Missing 'current_page_description' — summarize what you see on the CURRENT page \
before leaving it; its raw content will be discarded and this note is all you keep."
                .to_owned(),
        );
    }
    let Some(panel_id) = tool.input.get("panel_id").and_then(serde_json::Value::as_str) else { return pf };
    let Some(ctx) = state.context.iter().find(|c| c.id == panel_id) else {
        pf.errors.push(format!("Panel '{panel_id}' not found"));
        return pf;
    };
    if let Some(page) = tool.input.get("page").and_then(serde_json::Value::as_i64) {
        let total = ctx.total_pages.to_i64();
        if total > 0 && (page < 1 || page > total) {
            pf.errors.push(format!("Page {page} out of range (1-{total})"));
        }
    }
    pf
}
