//! Per-thread message files: `.context-pilot/threads/<id>.json`.
//!
//! Thread *metadata* stays in `config.json` (slim, see [`ThreadMeta`]); each
//! thread's `messages` live in their own file, rewritten only when that
//! thread's [`fingerprint`] changed since the last write. This keeps the save
//! path from re-serializing every message of every thread on each save.
//!
//! On-disk shape: `{"id": "<id>", "messages": [...]}`. The orchestrator reads
//! the same files to rebuild full thread logs for the web UI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use cp_base::config::constants::STORE_DIR;
use serde::Serialize;

use super::{Thread, ThreadMessage, ThreadOrigin, ThreadStatus};

/// Directory (under the store dir) holding one message file per thread.
pub const THREADS_DIR: &str = "threads";

/// Path of a thread's message file.
#[must_use]
pub fn thread_file(id: &str) -> PathBuf {
    PathBuf::from(STORE_DIR).join(THREADS_DIR).join(format!("{id}.json"))
}

/// Borrowing view of a [`Thread`] without its messages — what `config.json` stores.
///
/// Field names match [`Thread`] so a slim entry deserializes back into
/// a `Thread` with empty `messages` (filled by [`load_thread_messages`]).
#[derive(Debug, Serialize)]
pub struct ThreadMeta<'thread> {
    /// See [`Thread::id`].
    id: &'thread str,
    /// See [`Thread::name`].
    name: &'thread str,
    /// See [`Thread::status`].
    status: ThreadStatus,
    /// See [`Thread::created_at`].
    created_at: u64,
    /// See [`Thread::archived`].
    archived: bool,
    /// See [`Thread::paused`].
    paused: bool,
    /// See [`Thread::origin`].
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<&'thread ThreadOrigin>,
}

impl<'thread> From<&'thread Thread> for ThreadMeta<'thread> {
    fn from(t: &'thread Thread) -> Self {
        Self {
            id: &t.id,
            name: &t.name,
            status: t.status,
            created_at: t.created_at,
            archived: t.archived,
            paused: t.paused,
            origin: t.origin.as_ref(),
        }
    }
}

/// On-disk body of a thread message file.
#[derive(Serialize)]
struct ThreadFile<'thread> {
    /// Owning thread id.
    id: &'thread str,
    /// Full ordered message log.
    messages: &'thread [ThreadMessage],
}

/// Cheap change detector over a thread's messages — no serialization.
///
/// Folds every field a mutation can touch (count, timestamps, content and
/// path lengths, flags) so any push/retain/edit/ack changes the value.
fn fingerprint(messages: &[ThreadMessage]) -> u64 {
    const K: u64 = 0x0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::try_from(messages.len()).unwrap_or(u64::MAX);
    for m in messages {
        let len = |s: Option<&String>| s.map_or(u64::MAX, |v| u64::try_from(v.len()).unwrap_or(u64::MAX));
        let flags = u64::from(m.acknowledged)
            | (u64::from(m.auto) << 1u8)
            | (u64::from(m.has_been_pushed) << 2u8)
            | (u64::from(m.author == super::ThreadAuthor::Assistant) << 3u8);
        for v in [m.timestamp, len(m.content.as_ref()), len(m.file_path.as_ref()), flags] {
            h = (h ^ v).wrapping_mul(K);
        }
    }
    h
}

/// Fingerprint of each thread's file as last written / loaded.
fn clean_map() -> &'static Mutex<HashMap<String, u64>> {
    static CLEAN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    CLEAN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serialize the message files of threads changed since their last write.
///
/// Marks them clean on return: callers must hand every op to a writer that
/// never drops it (the persistence writer's durable lane).
#[must_use]
pub fn dirty_thread_file_ops(threads: &[Thread]) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(mut clean) = clean_map().lock() else { return Vec::new() };
    let mut ops = Vec::new();
    for t in threads {
        let fp = fingerprint(&t.messages);
        if clean.get(&t.id) == Some(&fp) {
            continue;
        }
        let Ok(bytes) = serde_json::to_vec(&ThreadFile { id: &t.id, messages: &t.messages }) else { continue };
        let _prev = clean.insert(t.id.clone(), fp);
        ops.push((thread_file(&t.id), bytes));
    }
    ops
}

/// Fill `messages` of threads loaded slim from `config.json` from their files.
///
/// Threads that already carry inline messages (legacy `config.json`) are left
/// as-is and stay unmarked, so the first save migrates them to files.
pub fn load_thread_messages(threads: &mut [Thread]) {
    let Ok(mut clean) = clean_map().lock() else { return };
    for t in threads.iter_mut().filter(|t| t.messages.is_empty()) {
        let Ok(bytes) = std::fs::read(thread_file(&t.id)) else { continue };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        let Some(msgs) = v.get("messages").and_then(|m| serde_json::from_value(m.clone()).ok()) else { continue };
        t.messages = msgs;
        let _prev = clean.insert(t.id.clone(), fingerprint(&t.messages));
    }
}
