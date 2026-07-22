//! Non-panel agent inspection endpoints that survived the cockpit removal.
//!
//! The cockpit's live context/module panels (memory, todos, tree, spine, queue,
//! scratchpad, callbacks, tools, radar, entities, and the panel list itself)
//! were removed with the cockpit view. What remains here are the two endpoints
//! that back **non-cockpit** surfaces:
//!
//! * [`usage`]   — the Usage/Costs page's per-worker cost snapshot.
//! * [`library`] — the fleet dashboard's prompt-library listing (agents /
//!   skills / commands).
//!
//! Both read the agent's tier-② persistence (`states/<worker>.json`,
//! `.context-pilot/{agents,skills,commands}/`) and reshape it to JSON. They
//! reach the shared [`Backend`](crate::transport::Backend) and
//! [`HttpReply`](crate::transport::rest::HttpReply) via absolute `crate::` paths.

use std::path::Path;
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::helpers::{agent_folder, extract_worker_param};

/// `GET /api/agent/{id}/usage` — current session cost data from worker state.
///
/// Returns the cumulative token counts and cost from the agent's active
/// worker. The web client can poll this to build a time series.
pub fn usage(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let folder_path = Path::new(&folder);

    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    let worker_id = extract_worker_param(query);
    let wid = match worker_id {
        Some(id) => id,
        None => {
            let workers = match backend.inspect_mut().list_workers(folder_path) {
                Ok(w) => w,
                Err(_) => return HttpReply::error(404, "cannot list workers"),
            };
            match workers.first() {
                Some(w) => w.clone(),
                None => return HttpReply::error(404, "no workers found"),
            }
        }
    };

    match backend.inspect_mut().read_worker(folder_path, &wid) {
        Ok(ws) => {
            let cost = ws.get("cost").cloned().unwrap_or(serde_json::Value::Null);
            HttpReply::ok(&cost)
        }
        Err(_) => HttpReply::error(404, "worker state unavailable"),
    }
}

/// `GET /api/agent/{id}/library` — prompt library items.
///
/// Scans the agent's `.context-pilot/{agents,skills,commands}/` directories
/// for `.md` files with YAML frontmatter and returns them as `LibraryItem[]`.
pub fn library(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };

    // The active behaviour agent is persisted at config.json
    // `modules.system.active_agent_id` (the PromptModule's global module data —
    // it is `is_global()`, so `build_module_data_maps` writes it under
    // `Shared.modules["system"]`). Read it via the mtime-cached inspector so
    // the footer selector can mark which agent is loaded, the same disk-read
    // mechanism `usage()` uses. Absent (older state / never switched) → None.
    let active_agent_id: Option<String> = {
        match state.lock() {
            Ok(mut b) => b.inspect_mut().read_config(Path::new(&folder)).ok().and_then(|cfg| {
                cfg.get("modules")
                    .and_then(|m| m.get("system"))
                    .and_then(|s| s.get("active_agent_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            }),
            Err(_) => return HttpReply::error(500, "backend lock poisoned"),
        }
    };

    let cp_dir = Path::new(&folder).join(".context-pilot");
    let mut items: Vec<serde_json::Value> = Vec::new();

    for (kind, subdir) in [("agent", "agents"), ("skill", "skills"), ("command", "commands")] {
        let dir = cp_dir.join(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let (name, description) = parse_frontmatter(&content);
            let id = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("").to_owned();
            // For commands, surface the prompt BODY (text after frontmatter) so
            // the web thread composer can seed the actual prompt when a `/cmd`
            // suggestion bubble is clicked (T350) — not the bare `/cmd` token.
            // Skipped for agents/skills (their bodies are large system prompts /
            // reference docs that nothing in the library list consumes).
            let body = (kind == "command").then(|| parse_command_body(&content));
            // Mark the active behaviour agent so the footer selector can show
            // and highlight it. Only meaningful for agents (skills/commands are
            // not "active"); left absent otherwise.
            let active = (kind == "agent" && active_agent_id.as_deref() == Some(id.as_str())).then_some(true);
            items.push(serde_json::json!({
                "id": id,
                "name": name,
                "kind": kind,
                "description": description,
                "body": body,
                "active": active,
            }));
        }
    }

    HttpReply::ok(&items)
}

/// Extract the markdown **body** of a command file — everything after the
/// YAML frontmatter block.
///
/// This is the text a `/command` expands to (the prompt that replaces the
/// `/cmd` literal). The web thread composer seeds it into the textarea when a
/// suggestion bubble is clicked (T350), so a `/boss-hunt` bubble fills with the
/// command's actual prompt rather than the bare `/boss-hunt` token.
///
/// Mirrors [`parse_frontmatter`]'s fence detection:
/// * no opening `---` → the whole (trimmed) file is the body;
/// * opening `---` but no closing `\n---` → no recoverable body (empty);
/// * otherwise → the trimmed text after the closing fence line.
fn parse_command_body(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return trimmed.trim().to_owned();
    }
    let after_first = trimmed[3..].trim_start_matches(['\r', '\n']);
    let Some(end) = after_first.find("\n---") else {
        return String::new();
    };
    // `after_first[end..]` begins with "\n---"; skip the newline so we sit on
    // the closing fence line, then take everything after that line ends.
    let after_fence = &after_first[end + 1..];
    match after_fence.find('\n') {
        Some(nl) => after_fence[nl + 1..].trim().to_owned(),
        None => String::new(),
    }
}

/// Extract `name` and `description` from YAML frontmatter in a markdown file.
///
/// Frontmatter is delimited by `---` lines at the top. Returns empty strings
/// if the file has no valid frontmatter.
fn parse_frontmatter(content: &str) -> (String, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), String::new());
    }
    let after_first = &trimmed[3..].trim_start_matches(['\r', '\n']);
    let Some(end) = after_first.find("\n---") else {
        return (String::new(), String::new());
    };
    let front = &after_first[..end];

    let mut name = String::new();
    let mut description = String::new();
    for line in front.lines() {
        if let Some(rest) = line.strip_prefix("name:") {
            name = rest.trim().trim_matches('"').trim_matches('\'').to_owned();
        } else if let Some(rest) = line.strip_prefix("description:") {
            description = rest.trim().trim_matches('"').trim_matches('\'').to_owned();
        }
    }
    (name, description)
}
