//! Context Radar — automatic log recall from Think task signals.
//!
//! Queries the Meilisearch logs index using the AI's recent task context
//! signals as semantic search queries.  Results are scored with adaptive
//! exponential decay and presented as YAML in a fixed panel.
//!
//! See `docs/design-logs-panel.md` for the full design.
//!
//!

use std::collections::HashMap;
use std::fmt::Write as _;

use chrono::TimeZone as _;
use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::runtime::State;

use crate::meili::client::{MeiliClient, SearchParams};
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
const MAX_FINAL_RESULTS: usize = 30;

/// Floor for the adaptive half-life (5 minutes in ms).
const HALF_LIFE_FLOOR_MS: f64 = 5.0 * 60.0 * 1000.0;

// ─── Scoring ────────────────────────────────────────────────────────────────

/// A scored log result before dedup.
struct ScoredResult {
    /// Unique log entry identifier (e.g. `"L42"`).
    log_id: String,
    /// ISO 8601 datetime when the log was created.
    datetime: String,
    /// Importance level (`"low"`, `"medium"`, `"high"`, `"critical"`).
    importance: String,
    /// Freeform categorization tags.
    tags: Vec<String>,
    /// Log entry text.
    content: String,
    /// Combined score: `relevance × query_decay × result_decay`.
    score: f64,
}

/// Compute the adaptive half-life from a set of task signals.
///
/// `half_life = max(span / 2, 5 minutes)`
/// where `span = newest_signal.timestamp - oldest_signal.timestamp`.
fn adaptive_half_life_ms(signals: &[crate::types::TaskSignal]) -> f64 {
    if signals.len() < 2 {
        return HALF_LIFE_FLOOR_MS;
    }
    let oldest = signals.first().map_or(0, |s| s.timestamp_ms);
    let newest = signals.last().map_or(0, |s| s.timestamp_ms);
    #[expect(clippy::cast_precision_loss, reason = "timestamp-diff ms fits in f64 mantissa for centuries")]
    let span = newest.saturating_sub(oldest) as f64;
    f64::max(span / 2.0, HALF_LIFE_FLOOR_MS)
}

/// Exponential decay factor: `exp(-ln(2) × age / half_life)`.
fn decay(age_ms: f64, half_life_ms: f64) -> f64 {
    if half_life_ms <= 0.0 {
        return 1.0;
    }
    (-f64::ln(2.0) * age_ms / half_life_ms).exp()
}

/// Append YAML lines for a single radar entry.
fn write_entry(yaml: &mut String, entry: &ScoredResult) {
    // Truncate long content to keep within token budget
    let content = if entry.content.len() > 200 {
        format!("{}...", entry.content.get(..entry.content.floor_char_boundary(197)).unwrap_or(""))
    } else {
        entry.content.clone()
    };
    let _w0 = writeln!(yaml, "  - content: \"{}\"", content.replace('"', "\\\""));
    let _w1 = writeln!(yaml, "    datetime: \"{}\"", entry.datetime);
    let _w2 = writeln!(yaml, "    importance: {}", entry.importance);
    if !entry.tags.is_empty() {
        let _w3 = writeln!(yaml, "    tags: [{}]", entry.tags.join(", "));
    }
    let _w4 = writeln!(yaml, "    score: {:.3}", entry.score);
}

/// Format milliseconds as a human-readable duration (e.g. "42m", "1h24m", "3d2h").
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::integer_division_remainder_used,
    reason = "display-only duration formatting — negative/huge values are impossible from timestamps"
)]
fn format_duration_ms(ms: f64) -> String {
    let secs = ms as u64 / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{days}d{}h", hours % 24)
    } else if hours > 0 {
        format!("{hours}h{}m", mins % 60)
    } else {
        format!("{mins}m")
    }
}

/// Format a millisecond timestamp as an ISO 8601 datetime string.
///
/// Falls back to `"unknown"` if the timestamp is zero or out of range.
fn format_timestamp_ms(ms: u64) -> String {
    if ms == 0 {
        return "unknown".to_string();
    }
    let dur = std::time::Duration::from_millis(ms);
    let secs = i64::try_from(dur.as_secs()).unwrap_or(i64::MAX);
    let nanos = dur.subsec_nanos();
    chrono::Utc.timestamp_opt(secs, nanos).single().map_or_else(|| "unknown".to_string(), |dt| dt.to_rfc3339())
}

/// Read the cached radar YAML from state, with fallback messages.
fn get_radar_yaml(state: &State) -> String {
    state.get_ext::<SearchState>().map_or_else(
        || "# Context Radar — search module not initialized\n".to_string(),
        |ss| {
            if ss.radar_cache.yaml.is_empty() {
                "# Context Radar — no task signals yet\n".to_string()
            } else {
                ss.radar_cache.yaml.clone()
            }
        },
    )
}

// ─── Signal ingestion ───────────────────────────────────────────────────────

/// Maximum character length for a task context signal.
///
/// Signals should be 1–2 sentences (the `task_context` param).  Anything
/// longer almost certainly contains a leaked `thought_body`.
const MAX_SIGNAL_LEN: usize = 300;

/// Truncate and sanitize a signal string.
///
/// - Caps at [`MAX_SIGNAL_LEN`] characters (on a char boundary).
/// - Strips XML/tool-call artifacts that indicate a leaked `thought_body`.
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
    let Some(ss) = state.get_ext_mut::<SearchState>() else { return };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let safe = sanitize_signal(content);
    ss.persist.task_signals.push(crate::types::TaskSignal { timestamp_ms: now_ms, content: safe });
    // Ring buffer: drop oldest signals when over capacity
    let len = ss.persist.task_signals.len();
    if len > crate::types::MAX_TASK_SIGNALS {
        let excess = len.saturating_sub(crate::types::MAX_TASK_SIGNALS);
        drop(ss.persist.task_signals.drain(..excess));
    }
}

// ─── Refresh ────────────────────────────────────────────────────────────────

/// Recompute the Context Radar panel content.
///
/// Queries the Meilisearch logs index for each task signal, scores results
/// with adaptive decay, deduplicates, and stores the YAML in
/// [`SearchState::radar_cache`].
///
/// Called from the main binary's pipeline after:
/// - Think (with `task_context`)
/// - `log_create` / `Close_conversation_history`
/// - Boot pre-population
#[expect(clippy::cast_precision_loss, reason = "timestamp ms as f64 — decay math requires floating point")]
pub(crate) fn refresh(state: &mut State) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));

    // Read connection info + signals (snapshot to release borrow)
    let (port, master_key, project_hash, signals) = {
        let Some(ss) = state.get_ext::<SearchState>() else {
            return;
        };
        if ss.persist.port == 0 || ss.persist.task_signals.is_empty() {
            // No server or no signals — clear the cache
            if let Some(ss_mut) = state.get_ext_mut::<SearchState>() {
                ss_mut.radar_cache.yaml = String::from("# Context Radar — no task signals yet\n");
                ss_mut.radar_cache.last_refresh_ms = now_ms;
            }
            return;
        }
        (
            ss.persist.port,
            ss.persist.master_key.clone(),
            ss.persist.project_hash.clone(),
            ss.persist.task_signals.clone(),
        )
    };

    let logs_uid = format!("cp_{project_hash}_logs");

    let Ok(client) = MeiliClient::new(port, &master_key) else {
        return;
    };

    // Compute adaptive half-life
    let half_life_ms = adaptive_half_life_ms(&signals);
    let now_f64 = now_ms as f64;

    // Query logs index for each signal
    let mut all_results: Vec<ScoredResult> = Vec::new();

    for signal in &signals {
        let query_age_ms = now_f64 - signal.timestamp_ms as f64;
        let q_decay = decay(query_age_ms, half_life_ms);

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
            let log_ts = hit.get("timestamp_ms").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let result_age_ms = now_f64 - log_ts as f64;
            let r_decay = decay(result_age_ms, half_life_ms);

            let score = relevance * q_decay * r_decay;

            let log_id = hit.get("id").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            let datetime = hit.get("datetime").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            let importance = hit.get("importance").and_then(serde_json::Value::as_str).unwrap_or("medium").to_string();
            let tags: Vec<String> = hit
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let content = hit.get("content").and_then(serde_json::Value::as_str).unwrap_or("").to_string();

            all_results.push(ScoredResult { log_id, datetime, importance, tags, content, score });
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

    // Sort descending by score, take top K
    let mut ranked: Vec<ScoredResult> = best_by_id.into_values().collect();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(MAX_FINAL_RESULTS);

    // Compute span for header
    let span_ms = if signals.len() >= 2 {
        signals.last().map_or(0, |s| s.timestamp_ms).saturating_sub(signals.first().map_or(0, |s| s.timestamp_ms))
    } else {
        0
    };

    // Build YAML output
    let mut yaml = String::with_capacity(4096);
    let _h0 = writeln!(yaml, "# Context Radar — {} results from {} signals", ranked.len(), signals.len());
    let _h1 = writeln!(
        yaml,
        "# Half-life: {} (span: {})",
        format_duration_ms(half_life_ms),
        format_duration_ms(span_ms as f64)
    );

    // Show all task signals as anchors with timestamps (most recent first)
    if !signals.is_empty() {
        let _h2 = writeln!(yaml, "anchors:");
        for sig in signals.iter().rev() {
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

    // Store in cache
    if let Some(ss) = state.get_ext_mut::<SearchState>() {
        ss.radar_cache.yaml = yaml;
        ss.radar_cache.last_refresh_ms = now_ms;
    }
}

// ─── Panel ──────────────────────────────────────────────────────────────────

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
