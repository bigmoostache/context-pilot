//! Message persistence module
//! Handles loading and saving individual message files
//!
//! UID format: UID_{number}_{letter}
//! - U = User message
//! - A = Assistant message
//! - T = Tool call
//! - R = Tool result
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use crate::infra::constants::{MESSAGES_DIR, STORE_DIR};
use crate::state::Message;

/// Build the path to the messages directory.
fn messages_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(MESSAGES_DIR)
}

/// Build the filesystem path for a message with the given UID.
fn message_path(uid: &str) -> PathBuf {
    messages_dir().join(format!("{uid}.yaml"))
}

/// Load a message by its UID from the messages directory
pub(crate) fn load_message(uid: &str) -> Option<Message> {
    let path = message_path(uid);
    let yaml = fs::read_to_string(&path).ok()?;
    serde_yaml::from_str(&yaml).ok()
}

/// Save a message to the messages directory using its UID
pub(crate) fn save_message(msg: &Message) {
    let dir = messages_dir();
    let _mkdir = fs::create_dir_all(&dir).ok();
    // Use UID if available, otherwise fall back to id
    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
    let path = message_path(file_id);
    if let Ok(yaml) = serde_yaml::to_string(msg) {
        let _r = fs::write(path, yaml).ok();
    }
}

/// Delete a message file by its UID
pub(crate) fn delete_message(uid: &str) {
    let path = message_path(uid);
    let _r = fs::remove_file(path).ok();
}

/// Append a user prompt to the persistent prompt history file.
///
/// Format: one JSON object per line (JSONL) at `.context-pilot/prompt-history.jsonl`.
/// This is an append-only audit log that survives conversation clears and context switches.
pub(crate) fn record_prompt_history(content: &str) {
    let path = PathBuf::from(STORE_DIR).join("prompt-history.jsonl");
    let timestamp = cp_mod_utilities::time::now_utc_rfc3339_secs();
    // Escape content properly for JSON embedding (trim trailing whitespace)
    let escaped = serde_json::to_string(content.trim_end()).unwrap_or_default();
    let line = format!(r#"{{"ts":"{timestamp}","content":{escaped}}}"#);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _r = writeln!(file, "{line}");
    }
}

/// Load all prompt history entries from the JSONL file.
///
/// Returns a `Vec<String>` of past user prompts, oldest first.
pub(crate) fn load_prompt_history() -> Vec<String> {
    let path = PathBuf::from(STORE_DIR).join("prompt-history.jsonl");
    let Ok(content) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let v = serde_json::from_str::<serde_json::Value>(line).ok()?;
            v.get("content")?.as_str().map(String::from)
        })
        .collect()
}
