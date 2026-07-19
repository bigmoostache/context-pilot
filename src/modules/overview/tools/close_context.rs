use crate::infra::tools::{ToolResult, ToolUse};
use crate::modules::{Module, all_modules};
use crate::state::{Entry, Kind, State};
use std::fmt::Write as _;

/// Outcome accumulators for a `Close_panel` batch.
#[derive(Default)]
struct CloseTally {
    /// Panels successfully closed (with their display descriptions).
    closed: Vec<String>,
    /// Panels skipped (protected, describe-gated, or module-rejected).
    skipped: Vec<String>,
    /// Requested ids that matched no live panel.
    not_found: Vec<String>,
    /// Malformed id entries (non-string JSON values).
    errors: Vec<String>,
}

/// Whether a file panel must be skipped because its tree description is missing
/// or stale (and no `tree_describe` is queued). Only applies when the tree
/// module is active and the file still exists on disk (T355 exemption).
fn file_panel_needs_describe(state: &State, ctx: &Entry) -> Option<String> {
    if !state.active_modules.contains("tree") || ctx.context_type.as_str() != Kind::FILE {
        return None;
    }
    let file_path = ctx.get_meta_str("file_path")?;
    if !std::path::Path::new(file_path).exists() {
        return None;
    }
    let cwd = std::env::current_dir().and_then(|d| d.canonicalize()).ok()?;
    let rel = std::path::Path::new(file_path).strip_prefix(&cwd).ok()?;
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
    needs_skip.then(|| format!("{} ({rel_str}) — needs tree_describe before closing", ctx.id))
}

/// Remove one context panel (already validated) and ask modules for special
/// close handling, recording the outcome into `tally`. `at` is the panel's
/// `(index, id)` in `state.context`.
fn close_one_panel(state: &mut State, at: (usize, &str), modules: &[Box<dyn Module>], tally: &mut CloseTally) {
    let (idx, id) = at;
    let ctx = state.context.remove(idx);
    let mut close_result: Option<Result<String, String>> = None;
    for module in modules {
        if let Some(result) = module.on_close_context(&ctx, state) {
            close_result = Some(result);
            break;
        }
    }
    match close_result {
        Some(Ok(desc)) => tally.closed.push(format!("{id} ({desc})")),
        Some(Err(msg)) => {
            state.context.insert(idx, ctx);
            tally.skipped.push(msg);
        }
        None => {
            let detail = modules.iter().find_map(|m| m.context_detail(&ctx)).unwrap_or_else(|| ctx.name.clone());
            tally.closed.push(format!("{id} ({detail})"));
        }
    }
}

/// Route one requested id to its outcome: not-found, protected, describe-skip,
/// or an actual close.
fn process_close_id(state: &mut State, id: &str, modules: &[Box<dyn Module>], tally: &mut CloseTally) {
    let Some(idx) = state.context.iter().position(|c| c.id == id) else {
        tally.not_found.push(id.to_owned());
        return;
    };
    let Some(ctx_elem) = state.context.get(idx) else {
        tally.not_found.push(id.to_owned());
        return;
    };
    if ctx_elem.context_type.is_fixed() {
        tally.skipped.push(format!("{id} (protected)"));
        return;
    }
    if let Some(skip_msg) = file_panel_needs_describe(state, ctx_elem) {
        tally.skipped.push(skip_msg);
        return;
    }
    close_one_panel(state, (idx, id), modules, tally);
}

/// Assemble the tool-result text from a completed close batch.
fn build_close_output(tally: &CloseTally) -> String {
    let mut output = String::new();
    if !tally.closed.is_empty() {
        let _r = write!(output, "Closed {}:\n{}", tally.closed.len(), tally.closed.join("\n"));
    }
    for (label, items) in [("Skipped", &tally.skipped), ("Not found", &tally.not_found), ("Errors", &tally.errors)] {
        if items.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        if label == "Not found" {
            let _r = write!(output, "Not found: {}", items.join(", "));
        } else {
            let _r = write!(output, "{label} {}:\n{}", items.len(), items.join("\n"));
        }
    }
    output
}

/// Execute the `Close_panel` tool to remove context panels.
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(ids) = tool.input.get("ids").and_then(serde_json::Value::as_array) else {
        return ToolResult::new(tool.id.clone(), "Missing 'ids' array parameter".to_owned(), true);
    };
    if ids.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'ids' array".to_owned(), true);
    }

    let modules = all_modules();
    let mut tally = CloseTally::default();

    for id_value in ids {
        match id_value.as_str() {
            Some(id) => process_close_id(state, id, &modules, &mut tally),
            None => tally.errors.push("Invalid ID (not a string)".to_owned()),
        }
    }

    let output = build_close_output(&tally);
    let mut result = ToolResult::new(tool.id.clone(), output, tally.closed.is_empty() && tally.skipped.is_empty());
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
