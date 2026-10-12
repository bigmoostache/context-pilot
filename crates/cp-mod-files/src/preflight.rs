//! Pre-flight checks for `Open`, `Edit` and `Write`, run before a call is queued.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_base::tools::pre_flight::Verdict;
use cp_mod_queue::types::QueueState;

/// Build virtual file content by replaying pending queue edits on top of `disk_content`.
///
/// Walks queued tool calls in order. A queued `Write` replaces the virtual content entirely;
/// a queued `Edit` applies `old_string → new_string` via normalized matching (same logic as
/// `execute_edit`). Returns `None` when no pending changes touch `canonical_path`.
fn build_virtual_content(disk_content: &str, canonical_path: &str, state: &State) -> Option<String> {
    let qs = state.get_ext::<QueueState>()?;
    let mut virtual_content: Option<String> = None;

    for call in &qs.queued_calls {
        let path_val = call.input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let call_canonical = state
            .resolve_path(path_val)
            .canonicalize()
            .map_or_else(|_| path_val.to_owned(), |p| p.to_string_lossy().to_string());
        if call_canonical != canonical_path {
            continue;
        }

        match call.tool_name.as_str() {
            "Write" => {
                if let Some(contents) = call.input.get("contents").and_then(|v| v.as_str()) {
                    virtual_content = Some(contents.to_owned());
                }
            }
            "Edit" => {
                let base = virtual_content.as_deref().unwrap_or(disk_content);
                let old = call.input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                let new = call.input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");

                if let Some(actual) = crate::tools::edit_file::find_normalized_match(base, old) {
                    let actual_owned = actual.to_owned();
                    let mut buf = base.to_owned();
                    buf = buf.replacen(&actual_owned, new, 1);
                    virtual_content = Some(buf);
                }
                // If the queued edit doesn't match, skip it — it may fail at flush time
            }
            _ => {}
        }
    }

    virtual_content
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
        if state.thread().context.iter().any(|c| c.get_meta_str("file_path") == Some(&canonical)) {
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

/// Verify `old_string` against both disk and post-queue virtual content,
/// pushing an error onto `pf` when the edit cannot apply. See the truth matrix
/// inline for the four current/virtual outcomes.
fn preflight_edit_oldstring(tool: &ToolUse, state: &State, target: &EditTarget<'_>, pf: &mut Verdict) {
    let path_str = target.path.to_string_lossy();
    let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str()) else {
        return;
    };
    let Ok(content) = std::fs::read_to_string(target.path) else {
        return;
    };
    let current_ok = crate::tools::edit_file::find_normalized_match(&content, old_string).is_some();
    let virtual_content = build_virtual_content(&content, target.canonical, state);
    let virtual_ok = virtual_content
        .as_ref()
        .is_some_and(|vc| crate::tools::edit_file::find_normalized_match(vc, old_string).is_some());

    if current_ok && !virtual_ok && virtual_content.is_some() {
        pf.errors.push(format!(
            "old_string found in '{path_str}' on disk but a pending Queue edit conflicts — the queued changes remove this text"
        ));
    } else if !current_ok && !virtual_ok {
        pf.errors.push(format!("old_string not found in '{path_str}' — open the file to see current content"));
    } else {
        // current ✓ virtual ✓, or current ✗ virtual ✓ — the edit applies cleanly.
    }
    // current ✗ virtual ✓ → fine (model edits post-queue state)
    // current ✓ virtual ✓ → fine (no conflict)
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
        .thread()
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
