//! Message persistence module
//! Handles loading and saving individual message files
//!
//! UID format: UID_{number}_{letter}
//! - U = User message
//! - A = Assistant message
//! - T = Tool call
//! - R = Tool result
use std::fs;
use std::path::PathBuf;

use crate::constants::{MESSAGES_DIR, STORE_DIR};
use crate::state::Message;

fn messages_dir() -> PathBuf {
    PathBuf::from(STORE_DIR).join(MESSAGES_DIR)
}

fn message_path(uid: &str) -> PathBuf {
    messages_dir().join(format!("{}.yaml", uid))
}

/// Load a message by its UID from the messages directory
pub fn load_message(uid: &str) -> Option<Message> {
    let path = message_path(uid);
    let yaml = fs::read_to_string(&path).ok()?;
    serde_yaml::from_str(&yaml).ok()
}

/// Save a message to the messages directory using its UID
pub fn save_message(msg: &Message) {
    let dir = messages_dir();
    fs::create_dir_all(&dir).ok();
    // Use UID if available, otherwise fall back to id
    let file_id = msg.uid.as_ref().unwrap_or(&msg.id);
    let path = message_path(file_id);
    if let Ok(yaml) = serde_yaml::to_string(msg) {
        fs::write(path, yaml).ok();
    }
}

/// Delete a message file by its UID
pub fn delete_message(uid: &str) {
    let path = message_path(uid);
    fs::remove_file(path).ok();
}
