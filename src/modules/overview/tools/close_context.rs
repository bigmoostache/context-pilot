use crate::infra::tools::{ToolResult, ToolUse};
use crate::modules::all_modules;
use crate::state::{Kind, State};
use std::fmt::Write as _;

/// Execute the `Close_panel` tool to remove context panels.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing 'ids' array parameter".to_owned(), true);
    };

    if ids.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'ids' array".to_owned(), true);
    }

    let mut closed: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let modules = all_modules();

    for id_value in ids {
        let Some(id) = id_value.as_str() else {
            errors.push("Invalid ID (not a string)".to_owned());
            continue;
        };

        // Find the context element
        let ctx_idx = state.context.iter().position(|c| c.id == id);

        let Some(idx) = ctx_idx else {
            not_found.push(id.to_owned());
            continue;
        };

        // Fixed panels are always protected
        let Some(ctx_elem) = state.context.get(idx) else {
            not_found.push(id.to_owned());
            continue;
        };
        if ctx_elem.context_type.is_fixed() {
            skipped.push(format!("{id} (protected)"));
            continue;
        }

        // Guard: file panels need a current tree description before closing.
        // EXCEPTION: a file that no longer exists on disk is exempt (T355) — a
        // deleted / branch-switched-away file can't be tree_described (the tool
        // rejects "path not found"), so demanding a description would block the
        // close forever. Such panels must be freely closable.
        if state.active_modules.contains("tree")
            && ctx_elem.context_type.as_str() == Kind::FILE
            && let Some(file_path) = ctx_elem.get_meta_str("file_path")
            && std::path::Path::new(file_path).exists()
            && let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize())
            && let Ok(rel) = std::path::Path::new(file_path).strip_prefix(&cwd)
        {
            let rel_str = rel.to_string_lossy();
            let ts = cp_mod_tree::types::TreeState::get(state);
            let needs_skip = ts.descriptions.iter().find(|d| d.path == rel_str.as_ref()).map_or_else(
                || !has_pending_tree_describe(state, rel_str.as_ref()),
                |desc| {
                    let current_hash =
                        cp_mod_tree::tools::compute_file_hash(std::path::Path::new(file_path)).unwrap_or_default();
                    !desc.file_hash.is_empty()
                        && desc.file_hash != current_hash
                        && !has_pending_tree_describe(state, rel_str.as_ref())
                },
            );
            if needs_skip {
                skipped.push(format!("{id} ({rel_str}) — needs tree_describe before closing"));
                continue;
            }
        }

        // Take the context element out so modules can mutate state without borrow conflicts
        let ctx = state.context.remove(idx);

        // Ask modules for special close handling
        let mut close_result: Option<Result<String, String>> = None;
        for module in &modules {
            if let Some(result) = module.on_close_context(&ctx, state) {
                close_result = Some(result);
                break;
            }
        }

        match close_result {
            Some(Ok(desc)) => {
                // Context already removed
                closed.push(format!("{id} ({desc})"));
            }
            Some(Err(msg)) => {
                // Put it back — close was rejected
                state.context.insert(idx, ctx);
                skipped.push(msg);
            }
            None => {
                // Default: use context_detail for description
                let detail = modules.iter().find_map(|m| m.context_detail(&ctx)).unwrap_or_else(|| ctx.name.clone());
                // Context already removed
                closed.push(format!("{id} ({detail})"));
            }
        }
    }

    // Build response
    let mut output = String::new();

    if !closed.is_empty() {
        let _r = write!(output, "Closed {}:\n{}", closed.len(), closed.join("\n"));
    }

    if !skipped.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Skipped {}:\n{}", skipped.len(), skipped.join("\n"));
    }

    if !not_found.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Not found: {}", not_found.join(", "));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors:\n{}", errors.join("\n"));
    }

    let mut result = ToolResult::new(tool.id.clone(), output, closed.is_empty() && skipped.is_empty());
    result.preserves_tempo = true;
    result
}

/// Check if there's a pending `tree_describe` in the queue for the given path.
///
/// The `Close_panel` stale-/missing-description guard skips a file panel whose
/// tree description is absent or out of date — UNLESS the agent has already
/// queued a `tree_describe` for that path (the fix is in flight), in which case
/// closing is allowed. Lives beside the guard that consumes it (the `Close_panel`
/// tool), and is re-exported to the overview module's `pre_flight` which applies
/// the same rule before the tool runs.
pub(crate) fn has_pending_tree_describe(state: &State, rel_path: &str) -> bool {
    state.active_modules.contains("queue")
        && cp_mod_queue::types::QueueState::get(state).queued_calls.iter().any(|call| {
            call.tool_name == "tree_describe"
                && call.input.get("descriptions").and_then(|v| v.as_array()).is_some_and(|descs| {
                    descs.iter().any(|d| {
                        d.get("path").and_then(|p| p.as_str()) == Some(rel_path)
                            && d.get("delete").and_then(serde_json::Value::as_bool) != Some(true)
                    })
                })
        })
}
