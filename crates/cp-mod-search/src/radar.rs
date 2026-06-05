//! Context Radar — automatic log recall from Think task signals.
//!
//! Queries the Meilisearch logs index using the AI's recent task context
//! signals as semantic search queries.  Results are scored with adaptive
//! exponential decay and presented as YAML in a fixed panel.
//!
//! See `docs/design-logs-panel.md` for the full design.

use std::collections::HashMap;
use std::fmt::Write as _;

use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::runtime::State;

use crate::meili::api::{MeiliClient, SearchParams};
use crate::types::SearchState;

/// Context type string for the radar panel (used by the search module registry).
pub(crate) const RADAR_PANEL_TYPE: &str = "context_radar";

/// Display name for the radar panel.
const RADAR_PANEL_NAME: &str = "Context Radar";

/// Number of search results per signal query.
const RESULTS_PER_QUERY: u32 = 10;

/// Semantic search ratio for log queries (0.0 = keyword, 1.0 = semantic).
const SEMANTIC_RATIO: f64 = 0.7;

/// Maximum number of results after dedup + ranking.
const MAX_FINAL_RESULTS: usize = 40;

/// Number of most-recent logs to always include deterministically.
///
/// These are fetched directly from [`LogsState`] — no Meilisearch
/// query needed.  Guarantees the AI always sees the latest context
/// regardless of signal-based relevance scoring.
const RECENT_LOGS_COUNT: usize = 10;

/// Half-life for log-count-based exponential decay (in number of logs).
///
/// A log that is 100 entries behind the current log count has half the
/// weight of the newest log.  This makes decay independent of wall-clock
/// time — vacations, breaks, and coding intensity don't affect scoring.
const HALF_LIFE_LOGS: f64 = 100.0;

/// A scored log result before dedup.
struct ScoredResult {
    /// Unique log entry identifier (e.g. `"L42"`).
    log_id: String,
    /// ISO 8601 datetime when the log was created.
    datetime: String,
    /// Importance level (`"low"`, `"medium"`, `"high"`, `"critical"`).
    importance: String,
    /// Log entry text.
    content: String,
    /// Combined score: `relevance × query_decay × result_decay`.
    score: f64,
}

/// Exponential decay with floor: `0.5 + 0.5 × exp(-ln(2) × age / half_life)`.
///
/// Decays from 1.0 (age = 0) down to a floor of 0.5 (age → ∞).
/// Old logs always retain at least half their relevance weight,
/// preventing semantically relevant older entries from vanishing.
fn decay(age_ms: f64, half_life_ms: f64) -> f64 {
    if half_life_ms <= 0.0 {
        return 1.0;
    }
    0.5f64.mul_add((-f64::ln(2.0) * age_ms / half_life_ms).exp(), 0.5)
}

/// Append YAML lines for a single radar entry.
fn write_entry(yaml: &mut String, entry: &ScoredResult) {
    let _w0 = writeln!(yaml, "  - content: \"{}\"", entry.content.replace('"', "\\\""));
    let _w1 = writeln!(yaml, "    datetime: \"{}\"", entry.datetime);
    let _w2 = writeln!(yaml, "    importance: {}", entry.importance);
    let _w4 = writeln!(yaml, "    score: {:.3}", entry.score);
}

/// Format a millisecond timestamp as ISO 8601, or `"unknown"` if zero/out-of-range.
fn format_timestamp_ms(ms: u64) -> String {
    if ms == 0 {
        return "unknown".to_string();
    }
    i64::try_from(ms)
        .ok()
        .and_then(cp_mod_utilities::time::epoch_ms_to_rfc3339)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Read the cached radar YAML from state, with fallback messages.
fn get_radar_yaml(state: &State) -> String {
    state.get_ext::<SearchState>().map_or_else(
        || "# Context Radar — search module not initialized\n".to_string(),
        |ss| {
            let cache = ss.radar_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.yaml.is_empty() {
                "# Context Radar — no task signals yet\n".to_string()
            } else {
                cache.yaml.clone()
            }
        },
    )
}

/// Maximum character length for a task context signal.
///
/// Signals should be 1–2 sentences (the `task_context` param).  Anything
/// longer almost certainly contains a leaked `thought_body`.
const MAX_SIGNAL_LEN: usize = 300;

/// Truncate and sanitize a signal string (cap at [`MAX_SIGNAL_LEN`], strip XML artifacts).
pub(crate) fn sanitize_signal(raw: &str) -> String {
    // If the signal contains tool XML, it's a leaked thought_body — take only
    // the text before the XML starts.
    let content = raw
        .find("<parameter")
        .or_else(|| raw.find("</"))
        .map_or(raw, |idx| raw.get(..idx).unwrap_or(raw))
        .trim()
        .trim_end_matches('"')
        .trim_end_matches('>')
        .trim();

    if content.len() <= MAX_SIGNAL_LEN {
        content.to_string()
    } else {
        let boundary = content.floor_char_boundary(MAX_SIGNAL_LEN);
        format!("{}…", content.get(..boundary).unwrap_or(content))
    }
}

/// Push a task context signal from the Think tool into the ring buffer.
///
/// Called from `pipeline.rs` after a Think tool executes with a
/// `task_context` parameter.  Caps the buffer at [`crate::types::MAX_TASK_SIGNALS`].
pub(crate) fn push_signal(state: &mut State, content: &str) {
    let _fg = cp_base::flame!("radar_push_signal");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let safe = sanitize_signal(content);

    // Read log count BEFORE borrowing SearchState (avoids double-borrow).
    let log_count = u64::try_from(cp_mod_logs::types::LogsState::get(state).logs.len()).unwrap_or(0);

    let Some(ss) = state.get_ext_mut::<SearchState>() else { return };
    ss.persist.task_signals.push(crate::types::TaskSignal { timestamp_ms: now_ms, log_count, content: safe });
    // Ring buffer: drop oldest signals when over capacity
    let len = ss.persist.task_signals.len();
    if len > crate::types::MAX_TASK_SIGNALS {
        let excess = len.saturating_sub(crate::types::MAX_TASK_SIGNALS);
        drop(ss.persist.task_signals.drain(..excess));
    }
}

/// Recompute the Context Radar panel content on a background thread.
///
/// Extracts all needed data from state synchronously (fast), then spawns
/// a detached thread that queries the Meilisearch logs index, scores
/// results with adaptive decay, and stores the YAML in the shared
/// [`RadarCache`].  Returns immediately — the panel picks up the new
/// content on the next render cycle.
///
/// Called from the main binary's pipeline after:
/// - Think (with `task_context`)
/// - `log_create` / `Close_conversation_history`
/// - Boot pre-population
pub(crate) fn refresh(state: &State) {
    let _fg = cp_base::flame!("radar_refresh");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));

    // Current log count for distance-based decay
    let logs_state = cp_mod_logs::types::LogsState::get(state);
    let current_log_count = u64::try_from(logs_state.logs.len()).unwrap_or(0);

    // Snapshot recent logs for the deterministic section (before releasing borrow)
    let recent_count = RECENT_LOGS_COUNT.min(logs_state.logs.len());
    let recent_logs: Vec<RecentLogSnapshot> = logs_state
        .logs
        .iter()
        .rev()
        .take(recent_count)
        .map(|l| RecentLogSnapshot {
            id: l.id.clone(),
            datetime: l.datetime.clone(),
            importance: l.importance.clone(),
            content: l.content.clone(),
        })
        .collect();

    // Read connection info + signals (snapshot to release borrow)
    let Some(ss) = state.get_ext::<SearchState>() else {
        return;
    };

    if ss.persist.port == 0 || ss.persist.task_signals.is_empty() {
        // No server or no signals — clear the cache via the shared Arc
        let mut cache = ss.radar_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.yaml = String::from("# Context Radar — no task signals yet\n");
        cache.last_refresh_ms = now_ms;
        drop(cache);
        return;
    }

    let job = RefreshJob {
        now_ms,
        current_log_count,
        recent_logs,
        port: ss.persist.port,
        master_key: ss.persist.master_key.clone(),
        project_hash: ss.persist.project_hash.clone(),
        signals: ss.persist.task_signals.clone(),
        radar_cache: std::sync::Arc::clone(&ss.radar_cache),
    };

    // Spawn background thread — fire-and-forget.
    // The thread does the expensive Meilisearch queries and writes the
    // result to the shared RadarCache when done.
    let _handle = std::thread::Builder::new().name("radar-refresh".into()).spawn(move || {
        refresh_inner(&job);
    });
}

/// Snapshot of a recent log entry for the deterministic radar section.
///
/// Avoids holding a borrow on `LogsState` across the async boundary.
struct RecentLogSnapshot {
    /// Unique log entry ID (e.g. `"L42"`).
    id: String,
    /// ISO 8601 datetime string.
    datetime: String,
    /// Importance level (e.g. `"high"`).
    importance: String,
    /// Log entry text.
    content: String,
}

/// All data needed by the background refresh thread.
///
/// Bundles parameters extracted from `State` on the main thread so
/// `refresh_inner` can run without any state reference.
struct RefreshJob {
    /// Unix timestamp (ms) when the refresh was requested.
    now_ms: u64,
    /// Total log count at refresh time (for distance-based decay).
    current_log_count: u64,
    /// Snapshots of the N most-recent logs for deterministic inclusion.
    recent_logs: Vec<RecentLogSnapshot>,
    /// Meilisearch server TCP port.
    port: u16,
    /// Meilisearch API master key.
    master_key: String,
    /// 8-char hash of the project path (for index naming).
    project_hash: String,
    /// Task context signals from the Think tool (ring buffer snapshot).
    signals: Vec<crate::types::TaskSignal>,
    /// Shared handle to the radar cache for writing results.
    radar_cache: crate::types::SharedRadarCache,
}

/// Inner refresh logic — runs on a background thread.
///
/// Queries the Meilisearch logs index for each task signal, scores results
/// with adaptive decay, deduplicates, and writes the YAML to the shared
/// [`RadarCache`].
#[expect(clippy::cast_precision_loss, reason = "timestamp ms as f64 — decay math requires floating point")]
fn refresh_inner(job: &RefreshJob) {
    let current_log_count_f = job.current_log_count as f64;
    let logs_uid = format!("cp_{}_logs", job.project_hash);

    let Ok(client) = MeiliClient::new(job.port, &job.master_key) else {
        return;
    };

    // Query logs index for each signal
    let mut all_results: Vec<ScoredResult> = Vec::new();

    for signal in &job.signals {
        // Log-count-based decay: distance = how many logs created since this signal
        let q_distance = current_log_count_f - signal.log_count as f64;
        let q_decay = decay(q_distance, HALF_LIFE_LOGS);

        let Ok(json) = client.search(&SearchParams {
            uid: &logs_uid,
            query: &signal.content,
            filter: None,
            sort: None,
            limit: RESULTS_PER_QUERY,
            semantic_ratio: Some(SEMANTIC_RATIO),
        }) else {
            continue;
        };

        let Some(hits) = json.get("hits").and_then(serde_json::Value::as_array) else {
            continue;
        };

        for hit in hits {
            let relevance = hit.get("_rankingScore").and_then(serde_json::Value::as_f64).unwrap_or(0.0);

            // Parse log number from ID (e.g. "L42" → 42) for count-based decay
            let log_number = hit
                .get("id")
                .and_then(serde_json::Value::as_str)
                .and_then(|id| id.strip_prefix('L'))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);
            let r_distance = current_log_count_f - log_number as f64;
            let r_decay = decay(r_distance, HALF_LIFE_LOGS);

            let score = relevance * q_decay * r_decay;

            let log_id = hit.get("id").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            let datetime = hit.get("datetime").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            let importance = hit.get("importance").and_then(serde_json::Value::as_str).unwrap_or("medium").to_string();
            let content = hit.get("content").and_then(serde_json::Value::as_str).unwrap_or("").to_string();

            all_results.push(ScoredResult { log_id, datetime, importance, content, score });
        }
    }

    // Dedup by log ID — keep max score per unique ID
    let mut best_by_id: HashMap<String, ScoredResult> = HashMap::new();
    for result in all_results {
        match best_by_id.entry(result.log_id.clone()) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if result.score > e.get().score {
                    let _prev = e.insert(result);
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let _prev = e.insert(result);
            }
        }
    }

    // Collect the recent log IDs so we can exclude them from the semantic pool
    let recent_ids: std::collections::HashSet<String> = job.recent_logs.iter().map(|l| l.id.clone()).collect();

    // Sort descending by score, exclude recent-log IDs, take top K
    let mut ranked: Vec<ScoredResult> = best_by_id.into_values().filter(|r| !recent_ids.contains(&r.log_id)).collect();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(MAX_FINAL_RESULTS);

    // Inject the N most recent logs deterministically (from snapshot)
    for log in &job.recent_logs {
        ranked.push(ScoredResult {
            log_id: log.id.clone(),
            datetime: log.datetime.clone(),
            importance: log.importance.clone(),
            content: log.content.clone(),
            score: 0.0,
        });
    }

    // Re-sort by datetime for display (newest first).
    // Selection was score-based; display is chronological.
    ranked.sort_by(|a, b| b.datetime.cmp(&a.datetime));

    // Build YAML output
    let mut yaml = String::with_capacity(4096);
    let _h0 = writeln!(yaml, "# Context Radar — {} results from {} signals", ranked.len(), job.signals.len());
    let _h1 = writeln!(yaml, "# Half-life: {HALF_LIFE_LOGS:.0} logs");

    // Show all task signals as anchors with timestamps (most recent first)
    if !job.signals.is_empty() {
        let _h2 = writeln!(yaml, "anchors:");
        for sig in job.signals.iter().rev() {
            let datetime = format_timestamp_ms(sig.timestamp_ms);
            let _h3 = writeln!(yaml, "  - time: \"{datetime}\"");
            let _h4 = writeln!(yaml, "    signal: \"{}\"", sig.content.replace('"', "\\\""));
        }
    }

    if ranked.is_empty() {
        let _h4 = writeln!(yaml, "# No matching logs found");
    } else {
        let _h5 = writeln!(yaml, "results:");
        for entry in &ranked {
            write_entry(&mut yaml, entry);
        }
    }

    // Store in shared cache — the panel picks this up on next render
    if let Ok(mut cache) = job.radar_cache.lock() {
        cache.yaml = yaml;
        cache.last_refresh_ms = job.now_ms;
    }
}

/// Fixed panel that shows Context Radar results.
pub(crate) struct ContextRadarPanel;

impl Panel for ContextRadarPanel {
    fn title(&self, _state: &State) -> String {
        RADAR_PANEL_NAME.to_string()
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span};

        let content = get_radar_yaml(state);

        content
            .lines()
            .map(|line| {
                let semantic = if line.starts_with('#') {
                    Semantic::Muted
                } else if line == "anchors:" || line == "results:" {
                    Semantic::Header
                } else if line.trim_start().starts_with("- content:") {
                    Semantic::Info
                } else if line.trim_start().starts_with("- time:") || line.trim_start().starts_with("signal:") {
                    // Anchor signal items
                    Semantic::Success
                } else if line.contains("importance: critical") || line.contains("importance: high") {
                    Semantic::Warning
                } else if line.contains("score:") {
                    Semantic::Muted
                } else {
                    Semantic::Default
                };

                Block::Line(vec![Span::styled(line.to_string(), semantic)])
            })
            .collect()
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh(&self, state: &mut State) {
        let yaml = get_radar_yaml(state);
        let token_count = cp_base::state::context::estimate_tokens(&yaml);
        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type.as_str() == RADAR_PANEL_TYPE) {
            ctx.token_count = token_count;
            ctx.full_token_count = token_count;
        }
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let yaml = get_radar_yaml(state);

        let (id, last_ms) = state
            .context
            .iter()
            .find(|e| e.context_type.as_str() == RADAR_PANEL_TYPE)
            .map(|e| (e.id.clone(), e.last_refresh_ms))
            .unwrap_or_default();

        vec![ContextItem::new(id, RADAR_PANEL_NAME, yaml, last_ms)]
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
