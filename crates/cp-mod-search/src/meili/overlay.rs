//! Ctrl+I overlay data provider.
//!
//! Fetches live Meilisearch stats and builds [`SearchOverlayInfo`]
//! for the main binary's overlay renderer.  Stats are cached for
//! 2 seconds to avoid hammering the local server from the render loop.

use cp_base::state::runtime::State;

use cp_base::cast::Safe as _;

use super::api;
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

    // Sort recompute counts descending, take top 8
    let mut top_recomputed: Vec<(String, u64)> =
        metrics.recompute_counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
    top_recomputed.sort_by_key(|e| std::cmp::Reverse(e.1));
    top_recomputed.truncate(8);

    // Sort last_sent_ms descending (most recent first), take top 8
    let mut recently_sent: Vec<(String, u64)> = metrics.last_sent_ms.iter().map(|(k, v)| (k.clone(), *v)).collect();
    recently_sent.sort_by_key(|e| std::cmp::Reverse(e.1));
    recently_sent.truncate(8);

    // Extract live stats (or defaults)
    let live = metrics.live_stats.clone().unwrap_or_default();

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
        database_size_bytes: live.database_size_bytes,
        used_database_size_bytes: live.used_database_size_bytes,
        files_embedding_count: live.files_embedding_count,
        files_is_indexing: live.files_is_indexing,
        logs_doc_count: live.logs_doc_count,
        embedding_model: live.embedding_model.clone(),
        meili_version: live.version,
        avg_document_size: live.avg_document_size,
        raw_document_db_size: live.raw_document_db_size,
        files_embedded_doc_count: live.files_embedded_doc_count,
        files_total_doc_count: live.files_total_doc_count,
        last_update: live.last_update,
        recent_tasks: live
            .recent_tasks
            .iter()
            .map(|t| crate::types::MeiliTaskInfo {
                uid: t.uid,
                task_type: shorten_task_type(&t.task_type),
                status: t.status.clone(),
                duration: humanize_duration(&t.duration),
            })
            .collect(),
        top_recomputed,
        recently_sent,
        meili_cpu_pct: live.meili_cpu_pct,
        meili_memory_bytes: live.meili_memory_bytes,
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

/// Compute Meilisearch CPU% from the tick delta vs the previous refresh.
///
/// `prev` is `(previous cpu ticks, previous fetched-at ms)`. Ticks are
/// centiseconds (100/sec). Returns 0.0 on the first sample or a zero interval.
///
/// Float math routes through the `float_math` chokepoint (CPU% is a ratio of
/// two time deltas — inherently fractional).
fn compute_cpu_pct(prev: Option<(u64, u64)>, cur_ticks: u64) -> f32 {
    use cp_base::cast::float_math;
    let Some((prev_t, prev_ms)) = prev else {
        return 0.0;
    };
    let tick_delta = cur_ticks.saturating_sub(prev_t);
    let ms_delta = current_ms().saturating_sub(prev_ms);
    if ms_delta == 0 || prev_t == 0 {
        return 0.0;
    }
    let cpu_secs = float_math::div_u64(tick_delta, 100.0);
    let wall_secs = float_math::div_u64(ms_delta, 1000.0);
    float_math::percent(cpu_secs, wall_secs).to_f32()
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
    let Ok(meili) = api::MeiliClient::new(ss.persist.port, &ss.persist.master_key) else {
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

    // Fetch server version (cheap GET, never changes but simpler to always fetch)
    let version = meili.version().unwrap_or_default();

    // Fetch recent tasks filtered to this project's indexes
    let tasks_json =
        meili.recent_tasks(5, &[&files_uid, &logs_uid]).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));

    let parsed = parse_stats_json(&stats, &files_uid, &logs_uid);
    let recent_tasks = parse_recent_tasks(&tasks_json);

    // -- Read Meilisearch process stats (CPU ticks + RSS) --
    let meili_pid = super::server::read_pid();
    let (meili_cpu_ticks, meili_memory_bytes) = meili_pid.and_then(read_process_stats).unwrap_or((0, 0));

    // Compute CPU% from tick delta vs previous refresh
    let prev_ticks =
        ss.metrics.lock().ok().and_then(|m| m.live_stats.as_ref().map(|s| (s.meili_cpu_ticks, s.fetched_at_ms)));
    let meili_cpu_pct = compute_cpu_pct(prev_ticks, meili_cpu_ticks);

    let live = MeiliLiveStats {
        database_size_bytes: parsed.db_size,
        used_database_size_bytes: parsed.db_used,
        files_embedding_count: parsed.emb_count,
        files_is_indexing: parsed.is_indexing,
        logs_doc_count: parsed.logs_count,
        embedding_model: model,
        fetched_at_ms: current_ms(),
        version,
        avg_document_size: parsed.avg_doc_size,
        raw_document_db_size: parsed.raw_doc_db,
        files_embedded_doc_count: parsed.embedded_count,
        files_total_doc_count: parsed.total_count,
        last_update: parsed.last_update,
        recent_tasks,
        meili_cpu_ticks,
        meili_cpu_pct,
        meili_memory_bytes,
    };

    // Write to cache (lock held briefly)
    if let Ok(mut m) = ss.metrics.lock() {
        m.live_stats = Some(live);
    }
}

/// Numeric + string fields parsed out of the Meilisearch `/stats` payload.
struct ParsedStats {
    /// Total on-disk database size in bytes.
    db_size: u64,
    /// Used (non-free) database size in bytes.
    db_used: u64,
    /// Number of embeddings on the files index.
    emb_count: u64,
    /// Whether the files index is currently indexing.
    is_indexing: bool,
    /// Average document size on the files index (bytes).
    avg_doc_size: u64,
    /// Raw (pre-index) document store size on the files index (bytes).
    raw_doc_db: u64,
    /// Count of files-index documents that carry an embedding.
    embedded_count: u64,
    /// Total files-index document count.
    total_count: u64,
    /// Logs-index document count.
    logs_count: u64,
    /// ISO 8601 timestamp of the last index update.
    last_update: String,
}

/// Parse the `/stats` JSON into a flat [`ParsedStats`] (per-index lookups by uid).
fn parse_stats_json(stats: &serde_json::Value, files_uid: &str, logs_uid: &str) -> ParsedStats {
    let u64_at = |v: Option<&serde_json::Value>, key: &str| -> u64 {
        v.and_then(|x| x.get(key)).and_then(serde_json::Value::as_u64).unwrap_or(0)
    };

    let indexes = stats.get("indexes");
    let files_stats = indexes.and_then(|i| i.get(files_uid));
    let logs_stats = indexes.and_then(|i| i.get(logs_uid));

    ParsedStats {
        db_size: stats.get("databaseSize").and_then(serde_json::Value::as_u64).unwrap_or(0),
        db_used: stats.get("usedDatabaseSize").and_then(serde_json::Value::as_u64).unwrap_or(0),
        last_update: stats.get("lastUpdate").and_then(serde_json::Value::as_str).unwrap_or("").to_owned(),
        emb_count: u64_at(files_stats, "numberOfEmbeddings"),
        is_indexing: files_stats
            .and_then(|f| f.get("isIndexing"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        avg_doc_size: u64_at(files_stats, "avgDocumentSize"),
        raw_doc_db: u64_at(files_stats, "rawDocumentDbSize"),
        embedded_count: u64_at(files_stats, "numberOfEmbeddedDocuments"),
        total_count: u64_at(files_stats, "numberOfDocuments"),
        logs_count: u64_at(logs_stats, "numberOfDocuments"),
    }
}

/// Parse the recent-tasks JSON array into [`MeiliTask`] records.
fn parse_recent_tasks(tasks_json: &serde_json::Value) -> Vec<crate::types::MeiliTask> {
    tasks_json
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    Some(crate::types::MeiliTask {
                        uid: t.get("uid")?.as_u64()?,
                        task_type: t.get("type")?.as_str()?.to_owned(),
                        status: t.get("status")?.as_str()?.to_owned(),
                        duration: t.get("duration").and_then(serde_json::Value::as_str).unwrap_or("").to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read CPU ticks (centiseconds) and RSS (bytes) for a process by PID.
///
/// On Linux, reads `/proc/<pid>/stat` + `/proc/<pid>/statm`.
/// On macOS, shells out to `ps -o rss=,cputime= -p <pid>`.
#[cfg(target_os = "linux")]
fn read_process_stats(pid: u32) -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat.split_whitespace();
    let utime: u64 = fields.nth(13)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    let cpu_ticks = utime.saturating_add(stime);
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some((cpu_ticks, rss_pages.saturating_mul(4096)))
}

/// Read CPU ticks (centiseconds) and RSS (bytes) for a process by PID (macOS).
#[cfg(target_os = "macos")]
fn read_process_stats(pid: u32) -> Option<(u64, u64)> {
    let output =
        std::process::Command::new("ps").args(["-o", "rss=,cputime=", "-p", &pid.to_string()]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    let mut parts = text.split_whitespace();
    let rss_kb: u64 = parts.next()?.parse().ok()?;
    let cputime_str = parts.next()?;
    let cpu_ticks = parse_ps_cputime(cputime_str)?;
    Some((cpu_ticks, rss_kb.saturating_mul(1024)))
}

/// Parse the `H:MM:SS` / `MM:SS` / `SS` seconds portion of a `ps` cputime.
#[cfg(target_os = "macos")]
fn parse_ps_seconds(main_part: &str) -> Option<u64> {
    let segments: Vec<&str> = main_part.split(':').collect();
    let n = |i: usize| -> Option<u64> { segments.get(i)?.parse().ok() };
    match segments.len() {
        1 => n(0),
        2 => Some(n(0)?.saturating_mul(60).saturating_add(n(1)?)),
        3 => Some(n(0)?.saturating_mul(3600).saturating_add(n(1)?.saturating_mul(60)).saturating_add(n(2)?)),
        _ => None,
    }
}

/// Parse `ps` cputime format (`H:MM:SS.cc` / `MM:SS.cc`) into centiseconds.
#[cfg(target_os = "macos")]
fn parse_ps_cputime(raw: &str) -> Option<u64> {
    let (main_part, centis_str) = raw.rsplit_once('.')?;
    let centis: u64 = centis_str.parse().ok()?;
    let total_secs = parse_ps_seconds(main_part)?;
    Some(total_secs.saturating_mul(100).saturating_add(centis))
}

/// Fallback for unsupported platforms — no process stats available.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_process_stats(_pid: u32) -> Option<(u64, u64)> {
    None
}

/// Shorten Meilisearch task type names for compact display.
fn shorten_task_type(task_type: &str) -> String {
    match task_type {
        "documentAdditionOrUpdate" => "docAdd".to_owned(),
        "documentDeletion" => "docDel".to_owned(),
        "settingsUpdate" => "settings".to_owned(),
        "indexCreation" => "create".to_owned(),
        "indexUpdate" => "update".to_owned(),
        "indexDeletion" => "delete".to_owned(),
        "indexSwap" => "swap".to_owned(),
        "taskCancelation" => "cancel".to_owned(),
        "taskDeletion" => "taskDel".to_owned(),
        "dumpCreation" => "dump".to_owned(),
        "snapshotCreation" => "snapshot".to_owned(),
        other => other.to_owned(),
    }
}

/// Convert an ISO 8601 duration string (e.g. "PT0.254092S") to a short
/// human-readable form (e.g. "0.25s"). Returns "—" for empty strings.
fn humanize_duration(iso: &str) -> String {
    if iso.is_empty() {
        return "\u{2014}".to_owned(); // em-dash
    }

    // Strip "PT" prefix and "S" suffix: "PT0.254092S" → "0.254092"
    let stripped = iso.strip_prefix("PT").unwrap_or(iso);
    let stripped = stripped.strip_suffix('S').unwrap_or(stripped);

    // Truncate to 2 decimal places for display
    if let Some((whole, frac)) = stripped.split_once('.') {
        let short_frac = frac.get(..2).unwrap_or(frac);
        format!("{whole}.{short_frac}s")
    } else {
        format!("{stripped}s")
    }
}
