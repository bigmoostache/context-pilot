//! Log → Meilisearch sync.
//!
//! Pushes every entry from the logs module into the per-project logs index
//! (upsert). Split out of `lib.rs` to keep that file within the 500-line cap.

use cp_base::state::runtime::State;

use crate::meili::api::MeiliClient;
use crate::types::SearchState;

/// Push all log entries from the logs module into the Meilisearch logs index.
///
/// Uses upsert semantics — existing documents with the same ID are updated,
/// new ones are inserted. Cheap for the typical log volume (~hundreds).
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

    let docs: Vec<serde_json::Value> = ls
        .logs
        .iter()
        .map(|l| {
            serde_json::json!({
                "id": l.id,
                "content": l.content,
                "importance": l.importance,
                "timestamp_ms": l.timestamp_ms,
                "datetime": l.datetime,
            })
        })
        .collect();

    let Ok(client) = MeiliClient::new(port, &master_key) else { return };
    // Fire-and-forget: Meilisearch processes the task asynchronously (including
    // remote Voyage AI embedding calls). No need to wait — the documents will
    // appear in search results within seconds, and blocking here freezes the UI.
    let _r = client.add_documents(&logs_uid, &serde_json::Value::Array(docs));
}
