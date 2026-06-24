//! `POST /api/agent/{id}/library/command` — create a new `/command` in an
//! agent's prompt library.
//!
//! Split out of [`rest`](super) for the 500-line file budget. A command is a
//! markdown file with YAML frontmatter under the agent's
//! `.context-pilot/commands/` directory:
//!
//! ```text
//! ---
//! name: <name>
//! description: <description>
//! ---
//! <body>
//! ```
//!
//! The agent's running prompt module watches `.context-pilot/` and picks up the
//! new file automatically, so the command becomes invocable (and surfaces as a
//! `/command` suggestion bubble in the web composer) without a restart — the
//! same mechanism the agent's own `Behaviour_create` tool relies on. This is
//! purely an additive file write confined to the agent realm; it never touches
//! the live agent process directly.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::create::slugify;
use super::{Backend, HttpReply, resolve_entry};

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
pub fn create_command(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
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

    let entry = match resolve_entry(state, id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };

    let slug = slugify(name);
    let commands_dir = std::path::Path::new(&entry.folder).join(".context-pilot").join("commands");
    let file_path = commands_dir.join(format!("{slug}.md"));

    // Never clobber an existing command — the agent (or a prior create) may own
    // this slug already.
    if file_path.exists() {
        return HttpReply::error(409, "a command with this name already exists");
    }

    if let Err(e) = std::fs::create_dir_all(&commands_dir) {
        return HttpReply::error(502, &format!("could not create commands directory: {e}"));
    }

    // Build the markdown: YAML frontmatter (name + optional description) then
    // the prompt body. `yaml_scalar` quotes + escapes the single-line fields so
    // a colon, quote, or stray newline in the name/description can't corrupt the
    // frontmatter block.
    let description = req.description.trim();
    let mut markdown = String::new();
    markdown.push_str("---\n");
    markdown.push_str(&format!("name: {}\n", yaml_scalar(name)));
    markdown.push_str(&format!("description: {}\n", yaml_scalar(description)));
    markdown.push_str("---\n");
    markdown.push_str(body);
    markdown.push('\n');

    if let Err(e) = std::fs::write(&file_path, markdown) {
        return HttpReply::error(502, &format!("could not write command file: {e}"));
    }

    HttpReply::json(201, &CreateCommandReceipt { id: slug, status: "created" })
}

/// Encode a single-line string as a double-quoted YAML scalar.
///
/// Backslashes and double quotes are escaped, and any CR/LF is collapsed to a
/// space so the value stays on one frontmatter line (the line-oriented
/// `parse_frontmatter` reader splits on `name:` / `description:` prefixes and
/// would otherwise be derailed by an embedded newline).
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
    /// Display name — the command slug (its `/invocation`) is derived from it.
    name: String,
    /// Optional one-line description shown on the suggestion bubble.
    #[serde(default)]
    description: String,
    /// The prompt body the `/command` expands to.
    body: String,
}

/// The receipt returned when a command file has been created.
#[derive(Serialize)]
struct CreateCommandReceipt {
    /// The created command's slug (its `/invocation` id).
    id: String,
    /// Always `"created"`.
    status: &'static str,
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
