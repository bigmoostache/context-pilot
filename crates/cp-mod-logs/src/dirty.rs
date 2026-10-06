//! Per-chunk change tracking for log persistence: a cheap fingerprint per
//! chunk, memoised at write time, so a save re-serializes only the chunks
//! (and the `next_id` counter) that changed since their last write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::types::LogEntry;

/// Cheap change detector over one chunk's entries — no serialization.
pub(crate) fn chunk_fingerprint(entries: &[&LogEntry]) -> u64 {
    const K: u64 = 0x0100_0000_01b3;
    let len = |s: &str| u64::try_from(s.len()).unwrap_or(u64::MAX);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::try_from(entries.len()).unwrap_or(u64::MAX);
    for e in entries {
        for v in [len(&e.id), e.timestamp_ms, len(&e.content), len(&e.importance), len(&e.datetime)] {
            h = (h ^ v).wrapping_mul(K);
        }
    }
    h
}

/// Key under which the last written `next_log_id` is memoised in [`clean_map`].
const NEXT_ID_KEY: usize = usize::MAX;

/// Fingerprint of each chunk file (and the `next_id` value) as last written.
pub(crate) fn clean_map() -> &'static Mutex<HashMap<usize, u64>> {
    static CLEAN: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
    CLEAN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `next_id.json` write op, only when the counter moved since the last write.
pub(crate) fn next_id_op(
    clean: &mut HashMap<usize, u64>,
    dir: &Path,
    next_log_id: usize,
) -> Option<(PathBuf, Vec<u8>)> {
    let next_fp = u64::try_from(next_log_id).unwrap_or(u64::MAX);
    if clean.get(&NEXT_ID_KEY) == Some(&next_fp) {
        return None;
    }
    let s = serde_json::to_string_pretty(&serde_json::json!({ "next_log_id": next_log_id })).ok()?;
    let _prev = clean.insert(NEXT_ID_KEY, next_fp);
    Some((dir.join("next_id.json"), s.into_bytes()))
}
