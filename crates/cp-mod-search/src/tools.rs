//! Search tool execution — dispatches `search` tool calls.
//!
//! Queries Meilisearch indexes (files and/or logs) and creates
//! dynamic result panels.

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::client::MeiliClient;
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

/// Build a successful `ToolResult`.
fn ok_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult {
        tool_use_id: tool.id.clone(),
        content,
        display: None,
        tldr: None,
        is_error: false,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}

/// Build an error `ToolResult`.
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

    let dt = chrono::DateTime::parse_from_rfc3339(&s_full).ok()?;
    u64::try_from(dt.timestamp_millis()).ok()
}

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

/// Parse a single Meilisearch hit from the files index.
fn parse_file_hit(hit: &serde_json::Value) -> SearchResult {
    let content = hit
        .get("_formatted")
        .and_then(|f| f.get("content"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| hit.get("content").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_string();

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
        tags: None,
    }
}

/// Parse a single Meilisearch hit from the logs index.
fn parse_log_hit(hit: &serde_json::Value) -> SearchResult {
    let content = hit
        .get("_formatted")
        .and_then(|f| f.get("content"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| hit.get("content").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .to_string();

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
        tags: hit
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()),
    }
}

/// Format search results as human-readable text for panel and tool output.
fn format_results(query: &str, file_results: &[SearchResult], log_results: &[SearchResult]) -> String {
    use std::fmt::Write as _;

    let total = file_results.len().saturating_add(log_results.len());
    let mut out = String::new();

    _ = writeln!(out, "# Search: \"{query}\" ({total} results)\n");

    if !file_results.is_empty() {
        _ = writeln!(out, "## Files ({})\n", file_results.len());
        for r in file_results {
            let path = r.file_path.as_deref().unwrap_or("?");
            let chunk = r.chunk_type.as_deref().unwrap_or("raw");
            let name = r.chunk_name.as_deref().unwrap_or("");
            let lines = match (r.line_start, r.line_end) {
                (Some(s), Some(e)) => format!(" lines {s}-{e}"),
                _ => String::new(),
            };
            let ext = r.extension.as_deref().unwrap_or("");

            if name.is_empty() {
                _ = writeln!(out, "- `{path}` [{chunk}]{lines} ({ext})");
            } else {
                _ = writeln!(out, "- `{path}` [{chunk}] {name}{lines} ({ext})");
            }

            // Snippet: first 3 lines of content
            let snippet: String = r.content.lines().take(3).collect::<Vec<_>>().join("\n  ");
            if !snippet.is_empty() {
                _ = writeln!(out, "  {snippet}");
            }
        }
        out.push('\n');
    }

    if !log_results.is_empty() {
        _ = writeln!(out, "## Logs ({})\n", log_results.len());
        for r in log_results {
            let id = r.log_id.as_deref().unwrap_or("?");
            let dt = r.datetime.as_deref().unwrap_or("?");
            let importance = r.importance.as_deref().unwrap_or("medium");
            let tags_str =
                r.tags.as_ref().filter(|t| !t.is_empty()).map(|t| format!(" #{}", t.join(" #"))).unwrap_or_default();

            let snippet: String = r.content.lines().take(3).collect::<Vec<_>>().join("\n  ");
            _ = writeln!(out, "- [{id}] {dt} · {importance}{tags_str}\n  {snippet}");
        }
        out.push('\n');
    }

    if file_results.is_empty() && log_results.is_empty() {
        out.push_str("No results found.\n");
    }

    out
}

/// Execute the `search` tool.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    // --- Extract parameters --------------------------------------------------

    let Some(query) = tool.input.get("query").and_then(serde_json::Value::as_str) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    let scope = tool.input.get("scope").and_then(serde_json::Value::as_str).unwrap_or("all");
    let path_prefix = tool.input.get("path_prefix").and_then(serde_json::Value::as_str);
    let extension = tool.input.get("extension").and_then(serde_json::Value::as_str);
    let sort = tool.input.get("sort").and_then(serde_json::Value::as_str).unwrap_or("relevance");
    let from_date = tool.input.get("from_date").and_then(serde_json::Value::as_str);
    let to_date = tool.input.get("to_date").and_then(serde_json::Value::as_str);
    let include_context = tool.input.get("include_context").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let limit = tool
        .input
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .map_or(20_u32, |n| u32::try_from(n.min(50)).unwrap_or(50));

    // --- Resolve index UIDs --------------------------------------------------

    let project_hash = state.get_ext::<SearchState>().map(|s| s.persist.project_hash.clone()).unwrap_or_default();

    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    // --- Search files --------------------------------------------------------

    let search_files = scope == "all" || scope == "project";
    let search_logs = scope == "all" || scope == "logs";

    let mut file_results: Vec<SearchResult> = Vec::new();
    let mut log_results: Vec<SearchResult> = Vec::new();

    if search_files {
        // Prepend path_prefix to query for relevance boost
        let effective_query = path_prefix.map_or_else(|| query.to_string(), |prefix| format!("{prefix} {query}"));

        let file_filter = build_file_filter(extension, from_date, to_date);
        let file_sort = file_sort_string(sort);

        match client.search(&crate::client::SearchParams {
            uid: &files_uid,
            query: &effective_query,
            filter: file_filter.as_deref(),
            sort: file_sort,
            limit,
        }) {
            Ok(resp) => {
                if let Some(hits) = resp.get("hits").and_then(|h| h.as_array()) {
                    for hit in hits {
                        file_results.push(parse_file_hit(hit));
                    }
                }
            }
            Err(e) => log::warn!("File search failed: {e}"),
        }
    }

    // --- Search logs ---------------------------------------------------------

    if search_logs {
        let log_filter = build_log_filter(from_date, to_date);
        let log_sort = log_sort_string(sort);

        match client.search(&crate::client::SearchParams {
            uid: &logs_uid,
            query,
            filter: log_filter.as_deref(),
            sort: log_sort,
            limit,
        }) {
            Ok(resp) => {
                if let Some(hits) = resp.get("hits").and_then(|h| h.as_array()) {
                    for hit in hits {
                        log_results.push(parse_log_hit(hit));
                    }
                }
            }
            Err(e) => log::warn!("Log search failed: {e}"),
        }
    }

    // --- Build output --------------------------------------------------------

    let file_count = file_results.len();
    let log_count = log_results.len();
    let panel_content = format_results(query, &file_results, &log_results);

    // Create dynamic panel
    let panel_id = crate::panel::create(state, &format!("search: {query}"), &panel_content);

    if include_context {
        ok_result(tool, format!("Created panel {panel_id}\n\n{panel_content}"))
    } else {
        ok_result(tool, format!("Created panel {panel_id}: {file_count} files, {log_count} logs for \"{query}\""))
    }
}
