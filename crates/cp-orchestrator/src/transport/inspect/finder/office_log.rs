//! Live office updates: a per-file CRDT state kept next to the file on disk, so
//! an open `BetterOffice` editor can merge an agent's edit in place instead of
//! reloading the whole document.
//!
//! The orchestrator never parses the CRDT. Per file it keeps the bytes every
//! replica opened (`base`) and the latest full replica state a writer posted
//! (`state`, a Yrs `encodeStateAsUpdate`). Merging a full state is idempotent,
//! so a reader can apply it whatever it already holds.
//!
//! The log is a cache over the file: it is valid only while the file's version
//! equals the version its last writer produced. Any other write (python-pptx, a
//! whole-file save) resets it to the current file, and editors fall back to a
//! reload.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

use super::office::{confined_target, readable, reply_of};
use super::support::extract_param;

/// One file's live state.
struct Log {
    /// Version of the file `base` was read at.
    base_version: String,
    /// The bytes every replica opens; they share one CRDT base.
    base: Vec<u8>,
    /// Latest full replica state, `None` until a writer posts one.
    state: Option<Vec<u8>>,
    /// Bumped on every accepted update; a writer must name the one it built on.
    seq: u64,
    /// File version the last accepted update wrote.
    head_version: String,
}

/// Process-wide logs, keyed by resolved file path.
static LOGS: LazyLock<Mutex<HashMap<PathBuf, Log>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Run `f` on the file's log, first resetting it when the file moved under it.
fn with_log<T>(path: &Path, f: impl FnOnce(&mut Log) -> Result<T, HttpReply>) -> Result<T, HttpReply> {
    let current = cp_office::version(path).map_err(|e| reply_of(&e))?;
    let mut logs = LOGS.lock().map_err(|_poisoned| HttpReply::error(500, "office log lock poisoned"))?;
    let stale = logs.get(path).is_none_or(|log| log.head_version != current);
    if stale {
        let file = cp_office::read(path).map_err(|e| reply_of(&e))?;
        let fresh = Log {
            base_version: file.version.clone(),
            base: file.bytes,
            state: None,
            seq: 0,
            head_version: file.version,
        };
        drop(logs.insert(path.to_path_buf(), fresh));
    }
    let log = logs.get_mut(path).ok_or_else(|| HttpReply::error(500, "office log missing"))?;
    let out = f(log);
    drop(logs);
    out
}

/// `GET /api/fleet/{id}/fs/office/log?path=[&base=1]` — `{ baseVersion,
/// headVersion, seq, state? , base? }` (bytes base64). `base=1` adds the base
/// bytes, needed once when a replica opens.
pub(crate) fn fs_office_log(state: &Mutex<Backend>, fleet_id: &str, query: &str) -> HttpReply {
    let with_base = extract_param(query, "base").is_some_and(|b| b == "1");
    let outcome = readable(state, fleet_id, query).and_then(|path| {
        with_log(&path, |log| {
            Ok(serde_json::json!({
                "baseVersion": log.base_version,
                "headVersion": log.head_version,
                "seq": log.seq,
                "state": log.state.as_ref().map(|s| B64.encode(s)),
                "base": with_base.then(|| B64.encode(&log.base)),
            }))
        })
    });
    match outcome {
        Ok(json) => HttpReply::ok(&json),
        Err(reply) => reply,
    }
}

/// A decoded update body: `[state][file bytes]`, split at the `stateLen` param.
struct Update {
    /// Full replica state after the edit.
    state: Vec<u8>,
    /// The document saved from that replica.
    file: Vec<u8>,
}

/// Read the body and split it after `state_len` bytes.
fn parse_update(body: &mut dyn std::io::Read, state_len: Option<usize>) -> Result<Update, HttpReply> {
    let mut raw = Vec::new();
    let cap = cp_office::MAX_BYTES.saturating_mul(2);
    if body.take(cap).read_to_end(&mut raw).is_err() {
        return Err(HttpReply::error(400, "could not read request body"));
    }
    let Some(split) = state_len.filter(|&n| n > 0 && n < raw.len()) else {
        return Err(HttpReply::error(400, "malformed update body"));
    };
    let file = raw.split_off(split);
    Ok(Update { state: raw, file })
}

/// `POST /api/fleet/{id}/fs/office/update?path=&seq=&stateLen=` — body per
/// [`parse_update`]. Writes the file (atomic, conflict-checked against the log
/// head) and stores the state. `409` when `seq` is not the current one or the
/// file moved. Returns `{ seq, headVersion }`.
pub(crate) fn fs_office_update(
    state: &Mutex<Backend>,
    fleet_id: &str,
    query: &str,
    body: &mut dyn std::io::Read,
) -> HttpReply {
    let seq = extract_param(query, "seq").and_then(|s| s.parse::<u64>().ok());
    let state_len = extract_param(query, "stateLen").and_then(|s| s.parse::<usize>().ok());
    let result = confined_target(state, fleet_id, query).and_then(|path| {
        let update = parse_update(body, state_len)?;
        with_log(&path, |log| apply(&path, log, seq, update))
    });
    match result {
        Ok(json) => HttpReply::ok(&json),
        Err(reply) => reply,
    }
}

/// Accept `update` on top of `log` when the writer built on its current `seq`.
fn apply(path: &Path, log: &mut Log, seq: Option<u64>, update: Update) -> Result<serde_json::Value, HttpReply> {
    if seq != Some(log.seq) {
        return Err(HttpReply::error(409, "office log moved; replay and retry"));
    }
    let version = cp_office::write(path, &update.file, Some(&log.head_version)).map_err(|e| reply_of(&e))?;
    log.state = Some(update.state);
    log.seq = log.seq.saturating_add(1);
    log.head_version = version;
    Ok(serde_json::json!({ "seq": log.seq, "headVersion": log.head_version }))
}
