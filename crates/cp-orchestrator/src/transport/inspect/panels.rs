//! Cockpit **panel inspection** endpoints — read agent state files and reshape
//! to maquette-compatible JSON.
//!
//! Each handler reads from the agent's tier-② persistence files (as specified
//! in the design doc v7 §3.1) and returns structured JSON. Global module data
//! comes from `config.json`, per-worker data from `states/<worker>.json`, and
//! shared files from the `shared/` directory.
//!
//! Worker-scoped endpoints accept an optional `?worker=<id>` query parameter;
//! when absent they fall back to the first worker found.

use std::path::Path;
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::helpers::{agent_folder, extract_worker_param, unwrap_module_array, yaml_map_to_keyed_array};

/// `GET /api/agent/{id}/panels` — live context panel list read from the
/// agent's `panels/` directory.
///
/// Each panel file (`panels/<uid>.json`) is parsed and reshaped to the
/// maquette [`ContextPanel`] format with real `tokens`, `misses`, and
/// `kind`. Returns an empty array when the panels directory is absent.
pub fn panel_list(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let panels_dir = Path::new(&folder).join(".context-pilot").join("panels");
    let entries = match std::fs::read_dir(&panels_dir) {
        Ok(rd) => rd,
        Err(_) => return HttpReply::ok(&serde_json::json!([])),
    };

    let mut panels: Vec<serde_json::Value> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else { continue };
        let Ok(val): Result<serde_json::Value, _> = serde_json::from_slice(&raw) else {
            continue;
        };
        let uid = val.get("uid").and_then(serde_json::Value::as_str).unwrap_or("");
        let name = val.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        let tokens = val.get("token_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let misses = val.get("total_cache_misses").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let panel_type = val.get("panel_type").and_then(serde_json::Value::as_str).unwrap_or("");
        let kind = map_panel_kind(panel_type);

        panels.push(serde_json::json!({
            "id": uid,
            "kind": kind,
            "name": name,
            "tokens": tokens,
            "costUsd": 0,
            "cached": misses == 0 && tokens > 0,
            "frozen": null,
            "misses": misses,
            "fixed": false,
        }));
    }

    // Sort by tokens descending for a meaningful default order.
    panels.sort_by(|a, b| {
        let at = a.get("tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let bt = b.get("tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
        bt.cmp(&at)
    });

    HttpReply::ok(&panels)
}

/// Map an agent panel_type string to the maquette PanelKind.
fn map_panel_kind(panel_type: &str) -> &'static str {
    match panel_type {
        "file" => "file",
        "console" => "console",
        "git_result" => "git",
        "conversation" | "conversation_history" => "threads",
        "search_result" => "search",
        "entity_result" => "entities",
        "memory" => "memory",
        "todo" => "todo",
        "spine" => "spine",
        "queue" => "queue",
        "scratchpad" => "scratchpad",
        "tree" => "tree",
        "callback" => "callback",
        "tools" => "tools",
        "context_radar" => "radar",
        "stats" => "stats",
        _ => "file",
    }
}

/// `GET /api/agent/{id}/memory` — memory items from `shared/memories.yaml`.
///
/// Transforms the on-disk YAML map (`{M1: {tl_dr, importance, labels}, …}`)
/// into the spec-compliant `MemoryCard[]` array.
pub fn memory(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    let folder_path = Path::new(&folder);
    let Ok(mut backend) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    let bytes = backend.inspect_mut().read_shared(folder_path, "memories.yaml");
    match bytes {
        Ok(raw) => {
            let yaml_val: Result<serde_json::Value, _> = serde_yaml::from_slice(&raw);
            match yaml_val {
                Ok(serde_json::Value::Object(map)) => {
                    let cards: Vec<serde_json::Value> = map
                        .into_iter()
                        .map(|(id, mut v)| {
                            v["id"] = serde_json::json!(id);
                            // Rename tl_dr → tldr for spec compliance.
                            if let Some(tl) = v.get("tl_dr").cloned() {
                                v["tldr"] = tl;
                            }
                            v
                        })
                        .collect();
                    HttpReply::ok(&cards)
                }
                Ok(_) => HttpReply::ok(&serde_json::json!([])),
                Err(_) => HttpReply::error(502, "YAML parse failed"),
            }
        }
        Err(_) => HttpReply::ok(&serde_json::json!([])),
    }
}

/// `GET /api/agent/{id}/todos` — todo items from the worker's module state.
///
/// Unwraps `{todos: [...]}` to return a flat `TodoItem[]`.
pub fn todos(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    unwrap_module_array(state, agent_id, query, "todo", "todos")
}

/// `GET /api/agent/{id}/spine` — spine notifications + config.
///
/// Unwraps `{notifications: [...]}` to return a flat `SpineNotif[]`.
pub fn spine(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    unwrap_module_array(state, agent_id, query, "spine", "notifications")
}

/// `GET /api/agent/{id}/queue` — queue state (active, queued calls).
///
/// Unwraps `{queued_calls: [...]}` to return a flat `QueueAction[]`.
pub fn queue(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    unwrap_module_array(state, agent_id, query, "queue", "queued_calls")
}

/// `GET /api/agent/{id}/scratchpad` — scratchpad cells.
///
/// Unwraps `{scratchpad_cells: [...]}` to return a flat `ScratchCell[]`.
pub fn scratchpad(state: &Mutex<Backend>, agent_id: &str, query: &str) -> HttpReply {
    unwrap_module_array(state, agent_id, query, "scratchpad", "scratchpad_cells")
}

/// `GET /api/agent/{id}/tree` — tree descriptions from shared YAML.
///
/// Transforms the on-disk YAML map (`{path: description, …}`) into a
/// `TreeRow[]` array with minimal fields.
pub fn tree(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    yaml_map_to_keyed_array(state, &folder, "tree-descriptions.yaml", |path, desc| {
        let kind = if path.ends_with('/') { "dir" } else { "file" };
        serde_json::json!({ "depth": 0, "name": path, "kind": kind, "desc": desc })
    })
}

/// `GET /api/agent/{id}/callbacks` — callback definitions from shared YAML.
///
/// Transforms the on-disk YAML map (`{CB1: {name, pattern, …}, …}`) into
/// a `CallbackRow[]` array with the map key injected as `id`.
pub fn callbacks(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let folder = match agent_folder(state, agent_id) {
        Ok(f) => f,
        Err(reply) => return reply,
    };
    yaml_map_to_keyed_array(state, &folder, "callbacks.yaml", |id, mut val| {
        val["id"] = serde_json::json!(id);
        val
    })
}

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
            items.push(serde_json::json!({
                "id": id,
                "name": name,
                "kind": kind,
                "description": description,
                "body": body,
            }));
        }
    }

    HttpReply::ok(&items)
}

// ── Derived-state panels (NOT reconstructable from tier-② files) ────────
//
// `tools`, `radar`, and `entities` read state the read-only inspection plane
// structurally cannot rebuild from an agent's on-disk tier-② files:
//
//   * **tools**  — the enabled-tool *catalog* (category + description) lives in
//     the agent binary's compiled module YAMLs, not in any per-agent file. The
//     orchestrator is a separate binary with no module registry, so it cannot
//     name or describe the tools.
//   * **radar**  — the Context Radar is a half-life ranking the *running* agent
//     computes over its logs; it is not persisted as a consumable artifact.
//   * **entities** — the entity DB is live SQLite (the on-disk `entities.db` is
//     a zero-byte handle; the truth lives in the agent's open connection/WAL).
//     Faithful row-counts/samples need a live connection the inspection plane
//     does not (and must not concurrently) open.
//
// Each endpoint therefore returns its NORMAL EMPTY shape (a deliberate 200, not
// a 404 that reads as a bug) — `[]` for the list panels, `{anchors,results}`
// empty for radar. The frontend cockpit panels render an explicit
// "unavailable over the web inspection plane" notice on the empty shape rather
// than a misleading blank list. Serving real data here is a follow-up that
// requires the AGENT to emit these into a readable artifact (e.g. a periodic
// `shared/tools.json` / `radar.json` / `entities-summary.json`) — tracked
// separately; this is the honest boundary, not fabricated data.

/// `GET /api/agent/{id}/tools` — empty by design (see module note above).
pub fn tools(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    // Resolve the agent so an unknown id still 404s (a known agent simply has
    // no inspection-plane tool catalog).
    match agent_folder(state, agent_id) {
        Ok(_) => HttpReply::ok(&serde_json::json!([])),
        Err(reply) => reply,
    }
}

/// `GET /api/agent/{id}/radar` — empty by design (see module note above).
pub fn radar(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    match agent_folder(state, agent_id) {
        Ok(_) => HttpReply::ok(&serde_json::json!({ "anchors": [], "results": [] })),
        Err(reply) => reply,
    }
}

/// `GET /api/agent/{id}/entities` — empty by design (see module note above).
pub fn entities(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    match agent_folder(state, agent_id) {
        Ok(_) => HttpReply::ok(&serde_json::json!([])),
        Err(reply) => reply,
    }
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

// Helpers in super::helpers — agent_folder, worker_module, unwrap_module_array,
// yaml_map_to_keyed_array, extract_worker_param.
