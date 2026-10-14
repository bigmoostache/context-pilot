//! Log → Meilisearch sync.
//!
//! Pushes every entry from the logs module into the per-project logs index
//! (upsert). Split out of `lib.rs` to keep that file within the 500-line cap.

use cp_base::state::runtime::State;

use crate::meili::api::MeiliClient;
use crate::types::SearchState;

/// Push new or changed log entries from the logs module into the Meilisearch
/// logs index (upsert), skipping entries already pushed unchanged (see [`SYNCED`]).
///
/// Called from:
/// - `load_module_data()` to backfill existing logs on boot/reload.
/// - `handle_tool_execution()` in the main binary after `log_create` /
///   `Close_conversation_history` finish executing (the `on_tool_complete`
///   hook fires too early — during streaming, before execution).
pub fn sync_logs_to_meilisearch(state: &State) {
    let Some(ss) = state.get_ext::<SearchState>() else { return };
    if ss.persist.port == 0 {
        return;
    }
    let port = ss.persist.port;
    let master_key = ss.persist.master_key.clone();
    let logs_uid = format!("cp_{}_logs", ss.persist.project_hash);

    let ls = cp_mod_logs::types::LogsState::get(state);
    if ls.logs.is_empty() {
        return;
    }

    // Only entries whose hash differs from the last successful push are sent:
    // typically 1-3 new logs instead of the whole history (~8k).
    let (docs, hashes): (Vec<serde_json::Value>, Vec<IdHash>) = {
        let _p = cp_base::perf_span!("logsync_build_docs");
        let synced = SYNCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        ls.logs
            .iter()
            .filter_map(|l| {
                let h = entry_hash(l);
                if synced.get(&l.id) == Some(&h) {
                    return None;
                }
                let doc = serde_json::json!({
                    "id": l.id,
                    "content": l.content,
                    "importance": l.importance,
                    "timestamp_ms": l.timestamp_ms,
                    "datetime": l.datetime,
                });
                Some((doc, (l.id.clone(), h)))
            })
            .unzip()
    };
    if docs.is_empty() {
        return;
    }

    let client_res = {
        let _p = cp_base::perf_span!("logsync_client");
        MeiliClient::new(port, &master_key)
    };
    let Ok(client) = client_res else { return };
    // Fire-and-forget: Meilisearch processes the task asynchronously (including
    // remote Voyage AI embedding calls). No need to wait — the documents will
    // appear in search results within seconds, and blocking here freezes the UI.
    let sent = {
        let _p = cp_base::perf_span!("logsync_http");
        client.add_documents(&logs_uid, &serde_json::Value::Array(docs)).is_ok()
    };
    // Record the watermark only once Meili accepted the batch, so a failed push
    // is retried in full on the next sync.
    if sent {
        SYNCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(hashes);
    }
}

/// `(log id, entry hash)` pair recorded in [`SYNCED`] after a successful push.
type IdHash = (String, u64);

/// Per-log-id hash of the fields last pushed to Meilisearch. Process-local:
/// empty after a restart, so the first sync of a session re-pushes everything
/// (Meili upserts, so that is idempotent).
static SYNCED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Hash of every field sent for a log entry; a change re-pushes the entry.
fn entry_hash(l: &cp_mod_logs::types::LogEntry) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    l.content.hash(&mut h);
    l.importance.hash(&mut h);
    l.timestamp_ms.hash(&mut h);
    l.datetime.hash(&mut h);
    h.finish()
}
