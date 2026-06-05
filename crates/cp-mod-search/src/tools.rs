//! Search tool execution — dispatches `search` tool calls.
//!
//! Queries Meilisearch indexes (files and/or logs) and creates
//! dynamic result panels.

use cp_base::state::runtime::State;
use cp_base::state::watchers::{DYN_PANEL_ID_PLACEHOLDER, DynPanel};
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::meili::api::MeiliClient;
use crate::panel::format_results;
use crate::types::{SearchResult, SearchState};

/// Dispatch search tool calls.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "search" => Some(exec_search(tool, state)),
        _ => None,
    }
}

/// Get a `MeiliClient` from the persisted state.
fn get_client(state: &State) -> Result<MeiliClient, String> {
    let ss = state.get_ext::<SearchState>().ok_or_else(|| "Search module not initialized".to_string())?;
    if ss.persist.port == 0 {
        return Err("Meilisearch server not running".to_string());
    }
    MeiliClient::new(ss.persist.port, &ss.persist.master_key)
}

/// Build an error `ToolResult` for sync validation failures.
fn err_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult {
        tool_use_id: tool.id.clone(),
        content,
        display: None,
        tldr: None,
        is_error: true,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}

/// Build a Meilisearch filter string for file queries.
///
/// `path_prefix` is added to the query string (not filter) since
/// Meilisearch STARTS WITH is version-dependent.  Extension and
/// date range use native Meilisearch filter syntax.
fn build_file_filter(extension: Option<&str>, from_date: Option<&str>, to_date: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ext) = extension {
        parts.push(format!("extension = '{ext}'"));
    }

    if let Some(from) = from_date
        && let Some(from_ms) = iso_to_ms(from)
    {
        parts.push(format!("last_modified_ms >= {from_ms}"));
    }
    if let Some(to) = to_date
        && let Some(to_ms) = iso_to_ms(to)
    {
        parts.push(format!("last_modified_ms <= {to_ms}"));
    }

    if parts.is_empty() { None } else { Some(parts.join(" AND ")) }
}

/// Build a Meilisearch filter string for log queries.
fn build_log_filter(from_date: Option<&str>, to_date: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(from) = from_date
        && let Some(from_ms) = iso_to_ms(from)
    {
        parts.push(format!("timestamp_ms >= {from_ms}"));
    }
    if let Some(to) = to_date
        && let Some(to_ms) = iso_to_ms(to)
    {
        parts.push(format!("timestamp_ms <= {to_ms}"));
    }

    if parts.is_empty() { None } else { Some(parts.join(" AND ")) }
}

/// Convert an ISO 8601 date string to milliseconds since Unix epoch.
///
/// Accepts date-only (`YYYY-MM-DD`) or full RFC 3339 datetime.
/// Returns `None` if parsing fails.
fn iso_to_ms(s: &str) -> Option<u64> {
    let s_full = if s.len() == 10 { format!("{s}T00:00:00Z") } else { s.to_string() };
    let ms = cp_mod_utilities::time::parse_rfc3339_to_epoch_ms(&s_full)?;
    u64::try_from(ms).ok()
}

/// Warning appended to every panel-creating tool result.
///
/// Prevents the LLM from closing result panels before acting on their content.
/// Closing a panel causes instant, irreversible context loss.
const PANEL_WARNING: &str = "\n\nIMPORTANT: Results live in this panel. Act on the information FIRST (write \
    files, answer questions, store in scratchpad, etc.), THEN close the panel. Closing it IMMEDIATELY and \
    IRREVERSIBLY erases all content from your context — you cannot recall it from memory afterward. \
    Never close-then-act; always act-then-close.";

/// Build Meilisearch sort parameter from tool sort string.
fn file_sort_string(sort: &str) -> Option<&'static str> {
    match sort {
        "date_asc" => Some("last_modified_ms:asc"),
        "date_desc" => Some("last_modified_ms:desc"),
        _ => None, // "relevance" — Meilisearch default
    }
}

/// Build Meilisearch sort parameter for logs.
fn log_sort_string(sort: &str) -> Option<&'static str> {
    match sort {
        "date_asc" => Some("timestamp_ms:asc"),
        "date_desc" => Some("timestamp_ms:desc"),
        _ => None,
    }
}

/// Deduplicate search results by content identity, keeping the highest score.
///
/// Used when multi-search returns overlapping results from keyword and semantic
/// queries.  After dedup, results are sorted by score descending and truncated
/// to `limit`.
fn dedup_by_score(results: &mut Vec<SearchResult>, limit: u32) {
    use std::collections::HashMap;

    // Collect best result per dedup key into a HashMap.
    let mut best: HashMap<String, SearchResult> = HashMap::new();

    for r in results.drain(..) {
        let key = r
            .log_id
            .as_deref()
            .map(String::from)
            .or_else(|| r.file_path.as_deref().map(|p| format!("{p}:{}", r.line_start.unwrap_or(0))))
            .unwrap_or_else(|| format!("__content_{}", best.len()));

        match best.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if r.ranking_score.unwrap_or(0.0) > e.get().ranking_score.unwrap_or(0.0) {
                    let _prev = e.insert(r);
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                let _ref = e.insert(r);
            }
        }
    }

    // Collect, sort by score descending, truncate
    *results = best.into_values().collect();
    results.sort_by(|a, b| {
        b.ranking_score.unwrap_or(0.0).partial_cmp(&a.ranking_score.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit as usize);
}

/// Parse a single Meilisearch hit from the files index.
fn parse_file_hit(hit: &serde_json::Value) -> SearchResult {
    let content = hit.get("content").and_then(serde_json::Value::as_str).unwrap_or("").to_string();

    SearchResult {
        content,
        file_path: hit.get("file_path").and_then(serde_json::Value::as_str).map(String::from),
        chunk_type: hit.get("chunk_type").and_then(serde_json::Value::as_str).map(String::from),
        chunk_name: hit.get("chunk_name").and_then(serde_json::Value::as_str).map(String::from),
        line_start: hit.get("line_start").and_then(serde_json::Value::as_u64).and_then(|n| u32::try_from(n).ok()),
        line_end: hit.get("line_end").and_then(serde_json::Value::as_u64).and_then(|n| u32::try_from(n).ok()),
        extension: hit.get("extension").and_then(serde_json::Value::as_str).map(String::from),
        log_id: None,
        datetime: None,
        importance: None,
        ranking_score: hit.get("_rankingScore").and_then(serde_json::Value::as_f64),
    }
}

/// Parse a single Meilisearch hit from the logs index.
fn parse_log_hit(hit: &serde_json::Value) -> SearchResult {
    let content = hit.get("content").and_then(serde_json::Value::as_str).unwrap_or("").to_string();

    SearchResult {
        content,
        file_path: None,
        chunk_type: None,
        chunk_name: None,
        line_start: None,
        line_end: None,
        extension: None,
        log_id: hit.get("id").and_then(serde_json::Value::as_str).map(String::from),
        datetime: hit.get("datetime").and_then(serde_json::Value::as_str).map(String::from),
        importance: hit.get("importance").and_then(serde_json::Value::as_str).map(String::from),
        ranking_score: hit.get("_rankingScore").and_then(serde_json::Value::as_f64),
    }
}

/// Async timeout for Meilisearch queries (seconds).
/// Local server, but semantic search involves remote Voyage AI embedder calls.
const ASYNC_TIMEOUT_SECS: u64 = 30;

/// Execute the `search` tool.
///
/// Runs Meilisearch HTTP queries on a worker thread to avoid blocking the main event loop.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("search_exec");
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    // --- Extract parameters (sync, needs State) ------------------------------

    let Some(query) = tool.input.get("query").and_then(serde_json::Value::as_str) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    let Some(semantic_query) =
        tool.input.get("semantic_query").and_then(serde_json::Value::as_str).filter(|s| !s.trim().is_empty())
    else {
        return err_result(
            tool,
            "Missing or empty 'semantic_query' parameter. You MUST provide a fabricated example of what the \
             target content looks like — NOT a description of what you're looking for, but an uneducated guess \
             at the actual text/code. Semantic embeddings find near-neighbors, so a fake snippet that resembles \
             the real content yields dramatically better results than a high-level description."
                .to_string(),
        );
    };

    let scope = tool.input.get("scope").and_then(serde_json::Value::as_str).unwrap_or("all");
    let path_prefix = tool.input.get("path_prefix").and_then(serde_json::Value::as_str);
    let extension = tool.input.get("extension").and_then(serde_json::Value::as_str);
    let sort = tool.input.get("sort").and_then(serde_json::Value::as_str).unwrap_or("relevance");
    let from_date = tool.input.get("from_date").and_then(serde_json::Value::as_str);
    let to_date = tool.input.get("to_date").and_then(serde_json::Value::as_str);
    let limit = tool
        .input
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .map_or(20_u32, |n| u32::try_from(n.min(50)).unwrap_or(50));
    let hide_contents = tool.input.get("hide_contents").and_then(serde_json::Value::as_bool).unwrap_or(false);

    // --- Resolve index UIDs (needs State) ------------------------------------

    let project_hash = state.get_ext::<SearchState>().map(|s| s.persist.project_hash.clone()).unwrap_or_default();
    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    // --- Extract owned values for the closure --------------------------------

    let query = query.to_string();
    let semantic_query = semantic_query.to_string();
    let effective_query = path_prefix.map_or_else(|| query.clone(), |prefix| format!("{prefix} {query}"));
    let search_files = scope == "all" || scope == "project";
    let search_logs = scope == "all" || scope == "logs";
    let search_entities = scope == "all" || scope == "entities";
    let file_filter = build_file_filter(extension, from_date, to_date);
    let log_filter = build_log_filter(from_date, to_date);
    let file_sort = file_sort_string(sort);
    let log_sort = log_sort_string(sort);
    let entities_uid = format!("cp_{project_hash}_entities");

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SECS, move || {
        let mut file_results: Vec<SearchResult> = Vec::new();
        let mut log_results: Vec<SearchResult> = Vec::new();

        // --- Search files ----------------------------------------------------

        if search_files {
            let keyword_params = crate::meili::api::SearchParams {
                uid: &files_uid,
                query: &effective_query,
                filter: file_filter.as_deref(),
                sort: file_sort,
                limit,
                semantic_ratio: Some(0.0),
            };
            let semantic_params = crate::meili::api::SearchParams {
                uid: &files_uid,
                query: &semantic_query,
                filter: file_filter.as_deref(),
                sort: file_sort,
                limit,
                semantic_ratio: Some(1.0),
            };
            match client.multi_search(&[keyword_params, semantic_params]) {
                Ok(results) => {
                    for result_set in &results {
                        if let Some(hits) = result_set.get("hits").and_then(|h| h.as_array()) {
                            for hit in hits {
                                file_results.push(parse_file_hit(hit));
                            }
                        }
                    }
                    dedup_by_score(&mut file_results, limit);
                }
                Err(e) => log::warn!("File multi-search failed: {e}"),
            }
        }

        // --- Search logs -----------------------------------------------------

        if search_logs {
            let keyword_params = crate::meili::api::SearchParams {
                uid: &logs_uid,
                query: &query,
                filter: log_filter.as_deref(),
                sort: log_sort,
                limit,
                semantic_ratio: Some(0.0),
            };
            let semantic_params = crate::meili::api::SearchParams {
                uid: &logs_uid,
                query: &semantic_query,
                filter: log_filter.as_deref(),
                sort: log_sort,
                limit,
                semantic_ratio: Some(1.0),
            };
            match client.multi_search(&[keyword_params, semantic_params]) {
                Ok(results) => {
                    for result_set in &results {
                        if let Some(hits) = result_set.get("hits").and_then(|h| h.as_array()) {
                            for hit in hits {
                                log_results.push(parse_log_hit(hit));
                            }
                        }
                    }
                    dedup_by_score(&mut log_results, limit);
                }
                Err(e) => log::warn!("Log multi-search failed: {e}"),
            }
        }

        // --- Search entities -------------------------------------------------

        let mut entity_results: Vec<crate::panel::EntityHit> = Vec::new();
        if search_entities {
            let entity_params = crate::meili::api::SearchParams {
                uid: &entities_uid,
                query: &query,
                filter: None,
                sort: None,
                limit,
                semantic_ratio: None, // keyword only — no embedder on entities index
            };
            match client.search(&entity_params) {
                Ok(result) => {
                    if let Some(hits) = result.get("hits").and_then(|h| h.as_array()) {
                        for hit in hits {
                            let table = hit
                                .get("entity_table")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown")
                                .to_string();
                            let all_text =
                                hit.get("_all_text").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                            let rank_score = hit.get("_rankingScore").and_then(serde_json::Value::as_f64);
                            entity_results.push(crate::panel::EntityHit { table, all_text, score: rank_score });
                        }
                    }
                }
                Err(e) => {
                    // Silently skip — index may not exist if entities module unused
                    log::debug!("Entity search skipped: {e}");
                }
            }
        }

        // --- Build output ----------------------------------------------------

        let file_count = file_results.len();
        let log_count = log_results.len();
        let entity_count = entity_results.len();
        let search_output =
            crate::panel::SearchOutput { files: &file_results, logs: &log_results, entities: &entity_results };
        let panel_content = format_results(&query, &search_output, hide_contents);

        if hide_contents {
            return ToolOutput { content: panel_content, is_error: false, create_panel: None, preserves_tempo: true };
        }

        let dyn_panel = DynPanel {
            context_type: crate::panel::SEARCH_PANEL_TYPE.to_string(),
            display_name: format!("search: {query}"),
            metadata: vec![("result_content".to_string(), panel_content.clone())],
            content: Some(panel_content),
        };

        ToolOutput {
            content: format!(
                "Created panel {DYN_PANEL_ID_PLACEHOLDER}: \
                 {file_count} file chunks, {log_count} logs, {entity_count} entities for \"{query}\"{PANEL_WARNING}",
            ),
            is_error: false,
            create_panel: Some(dyn_panel),
            preserves_tempo: false,
        }
    })
}
