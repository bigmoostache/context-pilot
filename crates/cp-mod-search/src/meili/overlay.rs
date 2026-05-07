//! Ctrl+I overlay data provider.
//!
//! Fetches live Meilisearch stats and builds [`SearchOverlayInfo`]
//! for the main binary's overlay renderer.  Stats are cached for
//! 2 seconds to avoid hammering the local server from the render loop.

use cp_base::state::runtime::State;

use super::client;
use crate::types::{MeiliLiveStats, SearchOverlayInfo, SearchState};

/// Read overlay information from the search module's state.
///
/// Returns `None` if the search module hasn't been initialized.
/// Used by the main binary's Ctrl+I overlay renderer.
///
/// Fetches live stats from Meilisearch at most once every 2 seconds
/// (cached in `SearchMetrics.live_stats`). The HTTP call is made
/// outside any lock to avoid blocking.
#[must_use]
pub(crate) fn overlay_info(state: &State) -> Option<SearchOverlayInfo> {
    let ss = state.get_ext::<SearchState>()?;

    // Refresh live stats from Meilisearch (max once per 2s, no lock held during HTTP)
    refresh_live_stats(ss);

    let metrics = ss.metrics.lock().ok()?;

    // Sort extensions by count descending, take top 8.
    let mut ext_vec: Vec<(String, u64)> = metrics.extension_counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
    ext_vec.sort_by_key(|e| std::cmp::Reverse(e.1));
    ext_vec.truncate(8);

    // Extract live stats (or defaults)
    let (db_size, db_used, emb_count, is_indexing, logs_count, model) = metrics.live_stats.as_ref().map_or_else(
        || (0, 0, 0, false, 0, String::new()),
        |ls| {
            (
                ls.database_size_bytes,
                ls.used_database_size_bytes,
                ls.files_embedding_count,
                ls.files_is_indexing,
                ls.logs_doc_count,
                ls.embedding_model.clone(),
            )
        },
    );

    Some(SearchOverlayInfo {
        port: ss.persist.port,
        chunks_indexed: metrics.chunks_indexed,
        files_indexed: metrics.files_indexed,
        queue_depth: metrics.queue_depth,
        error_count: metrics.error_count,
        last_activity_ms: metrics.last_activity_ms,
        index_ready: metrics.scan_complete,
        top_extensions: ext_vec,
        tree_sitter_chunks: metrics.tree_sitter_chunks,
        fallback_chunks: metrics.fallback_chunks,
        ocr_attempted: metrics.ocr_attempted,
        ocr_succeeded: metrics.ocr_succeeded,
        ocr_failed: metrics.ocr_failed,
        ocr_cached: metrics.ocr_cached,
        ocr_available: metrics.ocr_enabled,
        database_size_bytes: db_size,
        used_database_size_bytes: db_used,
        files_embedding_count: emb_count,
        files_is_indexing: is_indexing,
        logs_doc_count: logs_count,
        embedding_model: model,
    })
}

/// Current time in milliseconds since Unix epoch.
fn current_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Refresh cached live stats from Meilisearch if stale (>2s old).
///
/// Makes HTTP calls (`/stats` + `/settings/embedders`) outside any lock.
fn refresh_live_stats(ss: &SearchState) {
    if ss.persist.port == 0 {
        return;
    }

    let now_ms = current_ms();

    // Check if cached stats are fresh enough (lock held briefly)
    let is_stale = ss
        .metrics
        .lock()
        .ok()
        .is_none_or(|m| m.live_stats.as_ref().is_none_or(|s| now_ms.saturating_sub(s.fetched_at_ms) > 2000));

    if !is_stale {
        return;
    }

    // Fetch live stats — no lock held during network I/O
    let Ok(meili) = client::MeiliClient::new(ss.persist.port, &ss.persist.master_key) else {
        return;
    };
    let Ok(stats) = meili.global_stats() else {
        return;
    };

    let files_uid = format!("cp_{}_files", ss.persist.project_hash);
    let logs_uid = format!("cp_{}_logs", ss.persist.project_hash);

    // Read embedding model name from embedder settings (cached alongside stats)
    let model = meili
        .get_embedder_settings(&files_uid)
        .ok()
        .and_then(|v| v.get("default")?.get("model")?.as_str().map(String::from))
        .unwrap_or_default();

    let live = parse_live_stats(&stats, &files_uid, &logs_uid, &model);

    // Write to cache (lock held briefly)
    if let Ok(mut m) = ss.metrics.lock() {
        m.live_stats = Some(live);
    }
}

/// Parse the raw `/stats` JSON into a [`MeiliLiveStats`].
fn parse_live_stats(stats: &serde_json::Value, files_uid: &str, logs_uid: &str, model: &str) -> MeiliLiveStats {
    let db_size = stats.get("databaseSize").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let db_used = stats.get("usedDatabaseSize").and_then(serde_json::Value::as_u64).unwrap_or(0);

    let indexes = stats.get("indexes");

    let files_stats = indexes.and_then(|i| i.get(files_uid));
    let emb_count =
        files_stats.and_then(|f| f.get("numberOfEmbeddings")).and_then(serde_json::Value::as_u64).unwrap_or(0);
    let is_indexing =
        files_stats.and_then(|f| f.get("isIndexing")).and_then(serde_json::Value::as_bool).unwrap_or(false);

    let logs_stats = indexes.and_then(|i| i.get(logs_uid));
    let logs_count =
        logs_stats.and_then(|l| l.get("numberOfDocuments")).and_then(serde_json::Value::as_u64).unwrap_or(0);

    MeiliLiveStats {
        database_size_bytes: db_size,
        used_database_size_bytes: db_used,
        files_embedding_count: emb_count,
        files_is_indexing: is_indexing,
        logs_doc_count: logs_count,
        embedding_model: model.to_string(),
        fetched_at_ms: current_ms(),
    }
}
