//! Pre-flight checks for `Open`, `Edit` and `Write`, run before a call is queued.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_base::tools::pre_flight::Verdict;
use cp_mod_queue::types::QueueState;

use crate::tools::matcher::{MatchError, apply, locate};

/// Virtual post-queue content of one file, plus how many queued `Edit`s on it
/// no longer apply (they would fail at flush time).
struct VirtualFile {
    /// Content after replaying every applicable queued Write/Edit.
    content: String,
    /// Queued `Edit` calls whose `old_string` did not resolve to a unique match.
    stale_edits: usize,
}

/// Build virtual file content by replaying pending queue edits on top of `disk_content`.
///
/// Walks queued tool calls in order. A queued `Write` replaces the virtual content entirely;
/// a queued `Edit` is spliced with the same matcher as `execute_edit`. Returns `None` when
/// no pending changes touch `canonical_path`.
fn build_virtual_content(disk_content: &str, canonical_path: &str, state: &State) -> Option<VirtualFile> {
    let qs = state.get_ext::<QueueState>()?;
    let mut virtual_file: Option<VirtualFile> = None;

    for call in &qs.queued_calls {
        let path_val = call.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let call_canonical = state
            .resolve_path(path_val)
            .canonicalize()
            .map_or_else(|_| path_val.to_owned(), |p| p.to_string_lossy().to_string());
        if call_canonical != canonical_path {
            continue;
        }
        let vf = virtual_file.get_or_insert_with(|| VirtualFile { content: disk_content.to_owned(), stale_edits: 0 });

        match call.tool_name.as_str() {
            "Write" => {
                if let Some(contents) = call.input.get("contents").and_then(|v| v.as_str()) {
                    contents.clone_into(&mut vf.content);
                }
            }
            "Edit" => {
                let old = call.input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                let new = call.input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                match apply(&vf.content, old, new) {
                    Ok(updated) => vf.content = updated,
                    Err(_) => vf.stale_edits = vf.stale_edits.saturating_add(1),
                }
            }
            _ => {}
        }
    }

    virtual_file
}

/// Pre-flight for `Open`: validate each path exists, is a file, and warn if
/// already open in context.
pub(crate) fn preflight_open(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    let paths: Vec<String> = if let Some(s) = tool.input.get("path").and_then(serde_json::Value::as_str) {
        vec![s.to_owned()]
    } else if let Some(arr) = tool.input.get("path").and_then(serde_json::Value::as_array) {
        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
    } else {
        return pf;
    };
    for path in &paths {
        check_open_path(path, state, &mut pf);
    }
    pf
}

/// Validate one `Open` path: workdir present, exists, is a file, not already open.
fn check_open_path(path: &str, state: &State, pf: &mut Verdict) {
    let resolved = match state.try_resolve_path(path) {
        Ok(r) => r,
        Err(e) => return pf.errors.push(e),
    };
    let p = resolved.as_path();
    if !p.exists() {
        pf.errors.push(format!("File '{path}' not found"));
    } else if !p.is_file() {
        pf.errors.push(format!("'{path}' is not a file"));
    } else {
        let canonical = p.canonicalize().map_or_else(|_| path.to_owned(), |cp| cp.to_string_lossy().to_string());
        if state.agent().context.iter().any(|c| c.get_meta_str("file_path") == Some(&canonical)) {
            pf.warnings.push(format!("File '{path}' is already open in context"));
        }
    }
}

/// Resolved edit target: on-disk path plus its canonical string form.
struct EditTarget<'target> {
    /// The filesystem path being edited.
    path: &'target std::path::Path,
    /// Canonicalized path string, matched against open context entries.
    canonical: &'target str,
}

/// Verify `old_string` resolves to exactly one place in the post-queue
/// content (disk content when nothing is queued for this file). Ambiguous or
/// missing matches are errors; queued edits that no longer apply are warned.
fn preflight_edit_oldstring(tool: &ToolUse, state: &State, target: &EditTarget<'_>, pf: &mut Verdict) {
    let path_str = target.path.to_string_lossy();
    let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(target.path) else {
        return;
    };
    let virtual_file = build_virtual_content(&content, target.canonical, state);
    if let Some(stale) = virtual_file.as_ref().map(|vf| vf.stale_edits).filter(|&n| n > 0) {
        pf.warnings.push(format!(
            "{stale} queued Edit(s) on '{path_str}' no longer match and will fail at flush — fix or Queue_undo them"
        ));
    }
    let effective = virtual_file.as_ref().map_or(content.as_str(), |vf| vf.content.as_str());
    match locate(effective, old_string) {
        Ok(_) => {}
        Err(MatchError::NotFound) if virtual_file.is_some() && locate(&content, old_string).is_ok() => {
            pf.errors.push(format!(
                "old_string found in '{path_str}' on disk but a pending Queue edit conflicts — the queued changes remove this text"
            ));
        }
        Err(MatchError::NotFound) => {
            pf.errors.push(format!("old_string not found in '{path_str}' — open the file to see current content"));
        }
        Err(other) => pf.errors.push(format!("{} in '{path_str}'", other.describe())),
    }
}

/// Pre-flight for `Edit`: activate queue, validate path, warn if not open,
/// then verify `old_string` against disk + virtual content.
pub(crate) fn preflight_edit(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    // File edits are destructive — auto-activate queue for batching
    pf.activate_queue = true;
    let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) else {
        return pf;
    };
    let resolved = match state.try_resolve_path(path_str) {
        Ok(r) => r,
        Err(e) => {
            pf.errors.push(e);
            return pf;
        }
    };
    let p = resolved.as_path();
    if !p.exists() {
        pf.errors.push(format!("File '{path_str}' not found"));
        return pf;
    }
    if !p.is_file() {
        pf.errors.push(format!("'{path_str}' is not a file"));
        return pf;
    }
    let canonical = p.canonicalize().map_or_else(|_| path_str.to_owned(), |cp| cp.to_string_lossy().to_string());
    let is_open = state
        .agent()
        .context
        .iter()
        .any(|c| c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical));
    if !is_open {
        pf.warnings.push(format!("File '{path_str}' is not open in context. Edit will proceed if old_string has a unique match, but open the file to see current content."));
    }
    preflight_edit_oldstring(tool, state, &EditTarget { path: p, canonical: &canonical }, &mut pf);
    pf
}

/// Pre-flight for `Write`: activate queue, warn if parent dir is missing (it is
/// auto-created).
pub(crate) fn preflight_write(tool: &ToolUse, state: &State) -> Verdict {
    let mut pf = Verdict::new();
    // File writes are destructive — auto-activate queue for batching
    pf.activate_queue = true;
    if let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) {
        let p = match state.try_resolve_path(path_str) {
            Ok(r) => r,
            Err(e) => {
                pf.errors.push(e);
                return pf;
            }
        };
        if let Some(parent) = p.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            pf.warnings.push(format!(
                "Parent directory '{}' does not exist — it will be created automatically",
                parent.display()
            ));
        }
    }
    pf
}
