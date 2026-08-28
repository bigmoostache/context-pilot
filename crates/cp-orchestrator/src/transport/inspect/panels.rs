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

    let wid = match resolve_worker_id(folder_path, query) {
        Ok(id) => id,
        Err(reply) => return reply,
    };

    backend.inspect_mut().read_worker(folder_path, &wid).map_or_else(
        |_| HttpReply::error(404, "worker state unavailable"),
        |ws| {
            let cost = ws.get("cost").cloned().unwrap_or(serde_json::Value::Null);
            HttpReply::ok(&cost)
        },
    )
}

/// Resolve the worker id for [`usage`]: the explicit `?worker=` query param if
/// present, otherwise the first worker listed on disk. Returns an error
/// [`HttpReply`] when no worker can be determined.
fn resolve_worker_id(folder_path: &Path, query: &str) -> Result<String, HttpReply> {
    if let Some(id) = extract_worker_param(query) {
        return Ok(id);
    }
    let Ok(workers) = crate::inspect::StateReader::list_workers(folder_path) else {
        return Err(HttpReply::error(404, "cannot list workers"));
    };
    let Some(first) = workers.first() else {
        return Err(HttpReply::error(404, "no workers found"));
    };
    Ok(first.clone())
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

    // Behaviour `.md` files are fleet-shared (T651): the scan reads the
    // HOME-derived shared dir, NOT the per-agent realm. The active-agent id
    // above stays folder-relative — that's per-agent config.json state, not a
    // behaviour.
    let cp_dir = cp_base::config::constants::home_behaviours_dir();
    let mut items: Vec<serde_json::Value> = Vec::new();

    for (kind, subdir) in [("agent", "agents"), ("skill", "skills"), ("command", "commands")] {
        collect_kind_items(kind, &cp_dir.join(subdir), active_agent_id.as_deref(), &mut items);
    }

    HttpReply::ok(&items)
}

/// Scan one library `kind`'s shared `.md` directory and append its items to
/// `items`: disk files first (each an override candidate for a compiled-in
/// seed), then any seed with no on-disk file so the dropdown lists it too.
///
/// Mirrors the tui-side `cp_mod_prompt::storage::load_prompts_for` merge — one
/// seed source (`yamls/library.yaml` via `cp_base`), disk wins on id.
fn collect_kind_items(kind: &str, dir: &Path, active_agent_id: Option<&str>, items: &mut Vec<serde_json::Value>) {
    let seeds = seed_entries(kind);

    let mut disk_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for raw in entries {
            let Ok(entry) = raw else { continue };
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let (name, description) = parse_frontmatter(&content);
            let id = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("").to_owned();
            if id.is_empty() {
                continue;
            }
            let is_builtin = seeds.iter().any(|s| s.id == id);
            let _new = disk_ids.insert(id.clone());
            items.push(library_item(&LibraryItemParts {
                kind,
                id: &id,
                name: &name,
                description: &description,
                content: &content,
                active_agent_id,
                is_builtin,
            }));
        }
    }

    // Append seed built-ins that have no on-disk override, so the dropdown
    // lists every agent even before the user has created a local copy.
    for seed in seeds {
        if disk_ids.contains(&seed.id) {
            continue;
        }
        let active = (kind == "agent" && active_agent_id == Some(seed.id.as_str())).then_some(true);
        let body = (kind == "command").then(|| seed.content.clone());
        items.push(serde_json::json!({
            "id": seed.id,
            "name": seed.name,
            "kind": kind,
            "description": seed.description,
            "body": body,
            "active": active,
            "builtin": true,
        }));
    }
}

/// `GET /api/agent/{id}/identity` — the agent's durable self-identity.
///
/// Reads the ten identity fields from the agent's tier-② `config.json` at
/// `modules.agora.identity` (the Agora module is `is_global()`, so its
/// `save_module_data` — `{"identity": …}` — is written under
/// `Shared.modules["agora"]`). Uses the same mtime-cached inspector read as
/// [`library`]/[`usage`], so the web agent-settings form fetches the live
/// values on invalidation. Missing (never introduced / older state) yields the
/// empty ten-field object, so the form always has a stable shape to render.
pub fn identity(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };

    let live: serde_json::Value = {
        match state.lock() {
            Ok(mut b) => b
                .inspect_mut()
                .read_config(Path::new(&folder))
                .ok()
                .and_then(|cfg| {
                    cfg.get("modules").and_then(|m| m.get("agora")).and_then(|a| a.get("identity")).cloned()
                })
                .unwrap_or(serde_json::Value::Null),
            Err(_) => return HttpReply::error(500, "backend lock poisoned"),
        }
    };

    // Normalise to the full ten-field shape so the form renders a stable set of
    // inputs even before the identity has ever been set.
    let source = live.as_object();
    let mut obj = serde_json::Map::new();
    for key in [
        "identity",
        "values",
        "principles",
        "character",
        "expertise",
        "role",
        "operational_responsibilities",
        "knowledge_responsibilities",
        "organic_responsibilities",
        "direct_management",
    ] {
        let val = source.and_then(|m| m.get(key)).and_then(serde_json::Value::as_str).unwrap_or("");
        let _prev = obj.insert(key.to_owned(), serde_json::Value::String(val.to_owned()));
    }

    HttpReply::ok(&serde_json::Value::Object(obj))
}

/// The compiled-in seed entries for a library `kind` (`"agent"`/`"skill"`/
/// `"command"`) — the same `yamls/library.yaml` source the tui loader uses, so
/// the orchestrator's list mirrors the agent's own built-in set exactly.
fn seed_entries(kind: &str) -> &'static [cp_base::config::SeedEntry] {
    use cp_base::config::accessors::library;
    match kind {
        "agent" => library::agents(),
        "skill" => library::skills(),
        "command" => library::commands(),
        _ => &[],
    }
}

/// Borrowed inputs for [`library_item`] — bundled so the builder stays under
/// the argument-count cap. Built by struct literal at the single call site in
/// [`collect_kind_items`].
struct LibraryItemParts<'parts> {
    /// Library kind (`"agent"` / `"skill"` / `"command"`).
    kind: &'parts str,
    /// The item's id (file stem).
    id: &'parts str,
    /// Display name from frontmatter.
    name: &'parts str,
    /// Description from frontmatter.
    description: &'parts str,
    /// Raw `.md` file contents (command body is parsed from this).
    content: &'parts str,
    /// The currently loaded behaviour agent id, if any.
    active_agent_id: Option<&'parts str>,
    /// Whether this id also exists as a compiled-in seed.
    is_builtin: bool,
}

/// Build one `LibraryItem` JSON object from an on-disk `.md`.
///
/// `body` is surfaced only for commands (the `/cmd` composer seed — T350);
/// agent/skill bodies are large and nothing in the list consumes them (Export /
/// Edit fetch them on demand). `active` marks the loaded behaviour agent;
/// `builtin` flags an id that also exists as a compiled-in seed (i.e. this disk
/// file is a local OVERRIDE of a built-in).
fn library_item(parts: &LibraryItemParts<'_>) -> serde_json::Value {
    let body = (parts.kind == "command").then(|| parse_command_body(parts.content));
    let active = (parts.kind == "agent" && parts.active_agent_id == Some(parts.id)).then_some(true);
    serde_json::json!({
        "id": parts.id,
        "name": parts.name,
        "kind": parts.kind,
        "description": parts.description,
        "body": body,
        "active": active,
        "builtin": parts.is_builtin.then_some(true),
    })
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
    let Some(after_first) = trimmed.strip_prefix("---") else {
        return trimmed.trim().to_owned();
    };
    let after_fm = after_first.trim_start_matches(['\r', '\n']);
    // `rest` begins immediately after the closing `\n---`; the remainder of the
    // fence line runs up to the next newline, so the body is everything past it.
    let Some((_front, rest)) = after_fm.split_once("\n---") else {
        return String::new();
    };
    rest.split_once('\n').map_or_else(String::new, |(_fence_tail, body)| body.trim().to_owned())
}

/// Extract `name` and `description` from YAML frontmatter in a markdown file.
///
/// Frontmatter is delimited by `---` lines at the top. Returns empty strings
/// if the file has no valid frontmatter.
fn parse_frontmatter(content: &str) -> (String, String) {
    let trimmed = content.trim_start();
    let Some(after_first) = trimmed.strip_prefix("---") else {
        return (String::new(), String::new());
    };
    let after_fm = after_first.trim_start_matches(['\r', '\n']);
    let Some((front, _rest)) = after_fm.split_once("\n---") else {
        return (String::new(), String::new());
    };

    let mut name = String::new();
    let mut description = String::new();
    for line in front.lines() {
        if let Some(rest) = line.strip_prefix("name:") {
            rest.trim().trim_matches('"').trim_matches('\'').clone_into(&mut name);
            continue;
        }
        if let Some(rest) = line.strip_prefix("description:") {
            rest.trim().trim_matches('"').trim_matches('\'').clone_into(&mut description);
        }
    }
    (name, description)
}
