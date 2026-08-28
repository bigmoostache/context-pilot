//! Prompt-library CRUD — `/command` files and behaviour-`agent` `.md`s.
//!
//! Split out of [`create`](super) for the 500-line file budget. Owns the
//! create/upsert-command and read/upsert/delete-agent handlers, the shared
//! markdown compose/parse helpers, and their request/receipt JSON shapes.
//! Agent creation + slug derivation stay in the parent module.

use std::sync::Mutex;

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::super::{Backend, HttpReply, resolve_entry};
use super::slugify;

// ── Create command (prompt library) ─────────────────────────────────

/// `POST /api/agent/{id}/library/command` — write a new command markdown file.
///
/// Body: `{ "name": "...", "description": "...?", "body": "..." }`. `name` and
/// `body` are required (the slug is derived from `name`, the body is the prompt
/// the `/command` expands to); `description` is optional (the one-line label
/// shown on the suggestion bubble).
///
/// Returns `201` with `{ "id": <slug>, "status": "created" }` on success,
/// `400` for a missing/blank name or body or malformed JSON, `404` for an
/// unknown agent, `409` when a command with that slug already exists (never
/// clobbers), and `502` if the file cannot be written.
pub(crate) fn create_command(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<CreateCommandReq>(body_bytes) else {
        return HttpReply::error(400, "malformed create-command request");
    };
    let name = req.name.trim();
    if name.is_empty() {
        return HttpReply::error(400, "command name is required");
    }
    let body = req.body.trim();
    if body.is_empty() {
        return HttpReply::error(400, "command body is required");
    }

    // The realm is resolved only to enforce the per-agent ACL (resolve_entry);
    // behaviour files themselves are fleet-shared, so the path is HOME-derived,
    // not folder-relative (T651).
    if let Err(reply) = resolve_entry(state, id) {
        return reply;
    }

    let slug = slugify(name);
    let commands_dir = cp_base::config::constants::home_behaviours_dir().join("commands");
    let file_path = commands_dir.join(format!("{slug}.md"));

    if file_path.exists() {
        return HttpReply::error(409, "a command with this name already exists");
    }

    if let Err(e) = std::fs::create_dir_all(&commands_dir) {
        return HttpReply::error(502, &format!("could not create commands directory: {e}"));
    }

    let description = req.description.trim();
    let mut markdown = String::new();
    markdown.push_str("---\n");
    let _w1 = writeln!(markdown, "name: {}", yaml_scalar(name));
    let _w2 = writeln!(markdown, "description: {}", yaml_scalar(description));
    markdown.push_str("---\n");
    markdown.push_str(body);
    markdown.push('\n');

    if let Err(e) = std::fs::write(&file_path, markdown) {
        return HttpReply::error(502, &format!("could not write command file: {e}"));
    }

    HttpReply::json(201, &CreateCommandReceipt { id: slug, status: "created" })
}

/// `PUT /api/agent/{id}/library/command/{item}` — create or overwrite a
/// `/command` markdown file (the command bubble row's per-command Edit button).
///
/// The command twin of [`upsert_library_agent`]: unlike [`create_command`]
/// (which 409s on an existing slug to guard accidental clobber), this
/// DELIBERATELY overwrites — editing an existing command is exactly an
/// overwrite of `commands/<item>.md`. The file id is the URL's `item` (stable
/// across edits — only the frontmatter `name` changes, so a rename never
/// orphans the `.md`).
///
/// Body: `{ "name": "...", "description": "...?", "body": "..." }` — `name` and
/// `body` are required. Returns `200` `{ id, status }`, `400` for a blank
/// name/body or malformed JSON, `404` for an unknown agent, `502` if the file
/// cannot be written.
pub(crate) fn upsert_library_command(state: &Mutex<Backend>, id: &str, item_id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<CreateCommandReq>(body_bytes) else {
        return HttpReply::error(400, "malformed upsert-command request");
    };
    let name = req.name.trim();
    if name.is_empty() {
        return HttpReply::error(400, "command name is required");
    }
    let body = req.body.trim();
    if body.is_empty() {
        return HttpReply::error(400, "command body is required");
    }

    // ACL gate only; behaviour files are fleet-shared (HOME-derived), not
    // folder-relative (T651) — same rule as create_command / upsert_library_agent.
    if let Err(reply) = resolve_entry(state, id) {
        return reply;
    }
    let commands_dir = cp_base::config::constants::home_behaviours_dir().join("commands");
    let file_path = commands_dir.join(format!("{item_id}.md"));

    if let Err(e) = std::fs::create_dir_all(&commands_dir) {
        return HttpReply::error(502, &format!("could not create commands directory: {e}"));
    }
    let markdown = compose_md(name, req.description.trim(), body);
    if let Err(e) = std::fs::write(&file_path, markdown) {
        return HttpReply::error(502, &format!("could not write command file: {e}"));
    }
    HttpReply::json(200, &CreateCommandReceipt { id: item_id.to_owned(), status: "saved" })
}

/// Encode a single-line string as a double-quoted YAML scalar.
///
/// Backslashes and double quotes are escaped, and any CR/LF is collapsed to a
/// space so the value stays on one frontmatter line.
fn yaml_scalar(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' | '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The `POST /api/agent/{id}/library/command` request body.
#[derive(Deserialize)]
struct CreateCommandReq {
    /// Command display name — the `commands/<slug>.md` file id is derived from it.
    name: String,
    /// One-line label shown on the `/command` suggestion bubble.
    #[serde(default)]
    description: String,
    /// The prompt body the `/command` expands to.
    body: String,
}

/// The receipt returned when a command file has been created.
#[derive(Serialize)]
struct CreateCommandReceipt {
    /// The command slug (its stable file id).
    id: String,
    /// Result marker (`"created"` / `"saved"` / `"deleted"`).
    status: &'static str,
}

// ── Agent library CRUD (T581 footer editor) ─────────────────────────

/// `GET /api/agent/{id}/library/agent/{itemId}` — one behaviour agent's raw
/// authoring fields for the footer selector's Export + Edit-prefill.
///
/// Returns `{ name, description, body, builtin }`. Reads the on-disk
/// `agents/<itemId>.md` when present (a user agent or a local override of a
/// built-in); otherwise falls back to the compiled-in seed of that id (a pure
/// built-in with no local copy — still exportable + editable, editing writes
/// the first override). `404` only when neither a disk file nor a seed exists.
pub(crate) fn read_library_agent(state: &Mutex<Backend>, id: &str, item_id: &str) -> HttpReply {
    if let Err(reply) = resolve_entry(state, id) {
        return reply;
    }
    let file_path = cp_base::config::constants::home_behaviours_dir().join("agents").join(format!("{item_id}.md"));

    // Disk copy wins (user agent or local override); it also carries the
    // `builtin` flag when its id shadows a compiled-in seed.
    if let Ok(content) = std::fs::read_to_string(&file_path) {
        let (name, description, body) = split_frontmatter(&content);
        let builtin = seed_agent(item_id).is_some();
        return HttpReply::json(200, &LibraryAgentRaw { name, description, body, builtin });
    }

    // No disk file — a pure built-in. Serve its seed so Export/Edit still work.
    seed_agent(item_id).map_or_else(
        || HttpReply::error(404, "no such agent"),
        |seed| {
            HttpReply::json(
                200,
                &LibraryAgentRaw {
                    name: seed.name.clone(),
                    description: seed.description.clone(),
                    body: seed.content.clone(),
                    builtin: true,
                },
            )
        },
    )
}

/// `PUT /api/agent/{id}/library/agent/{itemId}` — create or overwrite a
/// behaviour agent's `.md` (create a user agent, or write a local override of a
/// built-in).
///
/// Body: `{ "name": "...", "description": "...?", "body": "..." }`. `name` and
/// `body` are required. The file id is the URL's `itemId` (stable across edits —
/// only the frontmatter `name` changes, so a rename never orphans the file);
/// unlike [`create_command`] this DELIBERATELY overwrites, since overwriting a
/// built-in's id is exactly how an override is authored.
///
/// Returns `200` `{ id, status }`, `400` for a blank name/body or malformed
/// JSON, `404` for an unknown agent, `502` if the file cannot be written.
pub(crate) fn upsert_library_agent(state: &Mutex<Backend>, id: &str, item_id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(req) = serde_json::from_slice::<UpsertAgentReq>(body_bytes) else {
        return HttpReply::error(400, "malformed upsert-agent request");
    };
    let name = req.name.trim();
    if name.is_empty() {
        return HttpReply::error(400, "agent name is required");
    }
    let body = req.body.trim();
    if body.is_empty() {
        return HttpReply::error(400, "agent body is required");
    }

    if let Err(reply) = resolve_entry(state, id) {
        return reply;
    }
    let agents_dir = cp_base::config::constants::home_behaviours_dir().join("agents");
    let file_path = agents_dir.join(format!("{item_id}.md"));

    if let Err(e) = std::fs::create_dir_all(&agents_dir) {
        return HttpReply::error(502, &format!("could not create agents directory: {e}"));
    }
    let markdown = compose_md(name, req.description.trim(), body);
    if let Err(e) = std::fs::write(&file_path, markdown) {
        return HttpReply::error(502, &format!("could not write agent file: {e}"));
    }
    HttpReply::json(200, &CreateCommandReceipt { id: item_id.to_owned(), status: "saved" })
}

/// `DELETE /api/agent/{id}/library/agent/{itemId}` — remove a behaviour agent's
/// on-disk `.md`.
///
/// If the file was a local override of a built-in, the compiled-in seed
/// reappears on the next list; if it was a pure user agent, it is gone. A pure
/// built-in has NO file to delete, so this returns `404` — the frontend hides
/// Delete on such rows, this is the authoritative backstop.
pub(crate) fn delete_library_agent(state: &Mutex<Backend>, id: &str, item_id: &str) -> HttpReply {
    if let Err(reply) = resolve_entry(state, id) {
        return reply;
    }
    let file_path = cp_base::config::constants::home_behaviours_dir().join("agents").join(format!("{item_id}.md"));
    if !file_path.exists() {
        return HttpReply::error(404, "no local agent file to delete (pure built-in)");
    }
    match std::fs::remove_file(&file_path) {
        Ok(()) => HttpReply::json(200, &CreateCommandReceipt { id: item_id.to_owned(), status: "deleted" }),
        Err(e) => HttpReply::error(502, &format!("could not delete agent file: {e}")),
    }
}

/// Look up a compiled-in seed agent by id (for the built-in Export/Edit
/// fallback + the `builtin` flag).
fn seed_agent(item_id: &str) -> Option<&'static cp_base::config::SeedEntry> {
    cp_base::config::accessors::library::agents().iter().find(|s| s.id == item_id)
}

/// Compose a prompt `.md` — YAML frontmatter (`name`/`description`) + body.
/// Shared by [`create_command`] and [`upsert_library_agent`] so both emit the
/// exact same on-disk shape the tui loader parses.
fn compose_md(name: &str, description: &str, body: &str) -> String {
    let mut markdown = String::new();
    markdown.push_str("---\n");
    let _w1 = writeln!(markdown, "name: {}", yaml_scalar(name));
    let _w2 = writeln!(markdown, "description: {}", yaml_scalar(description));
    markdown.push_str("---\n");
    markdown.push_str(body);
    markdown.push('\n');
    markdown
}

/// Split a prompt `.md` into `(name, description, body)` — the read twin of
/// [`compose_md`]. Tolerant of a missing frontmatter block (whole file = body).
fn split_frontmatter(content: &str) -> (String, String, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), String::new(), content.trim().to_owned());
    }
    let after_first = trimmed.get(3..).unwrap_or("").trim_start_matches(['\r', '\n']);
    let Some(end) = after_first.find("\n---") else {
        return (String::new(), String::new(), content.trim().to_owned());
    };
    let front = after_first.get(..end).unwrap_or("");
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
    // Body = everything after the closing fence line.
    let after_fence = after_first.get(end.saturating_add(1)..).unwrap_or("");
    let body = after_fence.find('\n').map_or("", |nl| after_fence.get(nl.saturating_add(1)..).unwrap_or("")).trim();
    (name, description, body.to_owned())
}

/// The raw authoring fields returned by [`read_library_agent`].
#[derive(Serialize)]
struct LibraryAgentRaw {
    /// The behaviour agent's display name (frontmatter `name`).
    name: String,
    /// The one-line description (frontmatter `description`).
    description: String,
    /// The agent's system-prompt body.
    body: String,
    /// Whether a compiled-in seed of this id exists (built-in or override).
    builtin: bool,
}

/// The `PUT /api/agent/{id}/library/agent/{itemId}` request body.
#[derive(Deserialize)]
struct UpsertAgentReq {
    /// The behaviour agent's display name (frontmatter `name`).
    name: String,
    /// Optional one-line description (frontmatter `description`).
    #[serde(default)]
    description: String,
    /// The agent's system-prompt body.
    body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_scalar_quotes_and_escapes() {
        assert_eq!(yaml_scalar("Hello"), "\"Hello\"");
        assert_eq!(yaml_scalar("a \"b\" c"), "\"a \\\"b\\\" c\"");
        assert_eq!(yaml_scalar("line1\nline2"), "\"line1 line2\"");
        assert_eq!(yaml_scalar("back\\slash"), "\"back\\\\slash\"");
    }
}
