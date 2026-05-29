use cp_base::state::runtime::State;
use cp_base::state::watchers::{DYN_PANEL_ID_PLACEHOLDER, DynPanel};
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::api::{FirecrawlClient, MapParams, ScrapeParams, SearchParams};
use cp_base::cast::Safe as _;
use std::fmt::Write as _;

/// Dispatch firecrawl tool calls.
pub fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "firecrawl_scrape" => Some(exec_scrape(tool, state)),
        "firecrawl_search" => Some(exec_search(tool, state)),
        "firecrawl_map" => Some(exec_map(tool, state)),
        _ => None,
    }
}

/// Build a `FirecrawlClient` from the `FIRECRAWL_API_KEY` env var.
fn get_client() -> Result<FirecrawlClient, String> {
    let key = std::env::var("FIRECRAWL_API_KEY").map_err(|_e| "FIRECRAWL_API_KEY not set".to_string())?;
    FirecrawlClient::new(key)
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

/// Warning appended to every panel-creating tool result.
///
/// Prevents the LLM from closing result panels before acting on their content.
/// Closing a panel causes instant, irreversible context loss.
const PANEL_WARNING: &str = "\n\nIMPORTANT: Results live in this panel. Act on the information FIRST (write \
    files, answer questions, store in scratchpad, etc.), THEN close the panel. Closing it IMMEDIATELY and \
    IRREVERSIBLY erases all content from your context — you cannot recall it from memory afterward. \
    Never close-then-act; always act-then-close.";

/// Async timeout for Firecrawl scrape/search calls (seconds).
/// JS rendering + multi-page scraping can be slow.
const ASYNC_TIMEOUT_SCRAPE_SECS: u64 = 60;

/// Async timeout for Firecrawl map calls (seconds).
/// Map is faster — just sitemap/URL discovery.
const ASYNC_TIMEOUT_MAP_SECS: u64 = 30;

/// Execute the `firecrawl_scrape` tool: scrape a single URL for content.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_scrape(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_scrape");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_string());
    };

    // Extract all params to owned types for the closure
    let url = url.to_string();
    let formats_val: Vec<String> = tool.input.get("formats").and_then(|v| v.as_array()).map_or_else(
        || vec!["markdown".to_string(), "links".to_string()],
        |arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
    );
    let country_val = tool
        .input
        .get("location")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("country"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let languages_val: Option<Vec<String>> = tool
        .input
        .get("location")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("languages"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SCRAPE_SECS, move || {
        let formats: Vec<&str> = formats_val.iter().map(String::as_str).collect();
        let languages_refs: Option<Vec<&str>> = languages_val.as_ref().map(|v| v.iter().map(String::as_str).collect());

        let params = ScrapeParams { url: &url, formats, country: country_val.as_deref(), languages: languages_refs };

        match client.scrape(&params) {
            Ok(resp) => {
                if !resp.success {
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                    return ToolOutput {
                        content: format!("Firecrawl scrape failed: {msg}"),
                        is_error: true,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let Some(data) = resp.data else {
                    return ToolOutput {
                        content: "Scrape returned no data".to_string(),
                        is_error: true,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                };

                let title = data.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or("untitled");

                // Build panel content
                let mut content = String::new();
                if let Some(ref meta) = data.metadata {
                    content.push_str("## Metadata\n\n");
                    if let Some(ref t) = meta.title {
                        let _r = writeln!(content, "**Title:** {t}");
                    }
                    if let Some(ref d) = meta.description {
                        let _r = writeln!(content, "**Description:** {d}");
                    }
                    if let Some(ref u) = meta.source_url {
                        let _r = writeln!(content, "**URL:** {u}");
                    }
                    content.push('\n');
                }
                if let Some(ref md) = data.markdown {
                    content.push_str("## Content\n\n");
                    content.push_str(md);
                    content.push_str("\n\n");
                }
                if let Some(ref links) = data.links
                    && !links.is_empty()
                {
                    content.push_str("## Links\n\n");
                    for link in links {
                        let _r = writeln!(content, "- {link}");
                    }
                }

                let dyn_panel = DynPanel {
                    context_type: crate::panel::FIRECRAWL_PANEL_TYPE.to_string(),
                    display_name: format!("firecrawl_scrape: {url}"),
                    metadata: vec![("result_content".to_string(), content.clone())],
                    content: Some(content),
                };

                ToolOutput {
                    content: format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: scraped {url} ({title}){PANEL_WARNING}",
                    ),
                    is_error: false,
                    create_panel: Some(dyn_panel),
                    preserves_tempo: false,
                }
            }
            Err(e) => ToolOutput { content: e, is_error: true, create_panel: None, preserves_tempo: false },
        }
    })
}
/// Execute the `firecrawl_search` tool: search and scrape in one call.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_search");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    // Extract all params to owned types for the closure
    let query = query.to_string();
    let limit = tool.input.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(3).to_u32();
    let sources_val: Vec<String> = tool.input.get("sources").and_then(|v| v.as_array()).map_or_else(
        || vec!["web".to_string()],
        |arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
    );
    let cats_val: Option<Vec<String>> = tool
        .input
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let tbs_val = tool.input.get("tbs").and_then(|v| v.as_str()).map(String::from);
    let loc_val = tool.input.get("location").and_then(|v| v.as_str()).map(String::from);

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SCRAPE_SECS, move || {
        let sources: Vec<&str> = sources_val.iter().map(String::as_str).collect();
        let cats_refs: Option<Vec<&str>> = cats_val.as_ref().map(|v| v.iter().map(String::as_str).collect());

        let params = SearchParams {
            query: &query,
            limit,
            sources,
            categories: cats_refs,
            tbs: tbs_val.as_deref(),
            location: loc_val.as_deref(),
        };

        match client.search(&params) {
            Ok(resp) => {
                if !resp.success {
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                    return ToolOutput {
                        content: format!("Firecrawl search failed: {msg}"),
                        is_error: true,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let Some(data) = resp.data else {
                    return ToolOutput {
                        content: format!("No results found for '{query}'"),
                        is_error: false,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                };

                // Parse data — can be array (scraped results) or object (web/news/images dict)
                let results: Vec<crate::types::SearchResult> = if data.is_array() {
                    serde_json::from_value(data).unwrap_or_default()
                } else if let Some(web_arr) = data.get("web").and_then(|v| v.as_array()) {
                    web_arr.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
                } else {
                    // Fallback: dump as YAML
                    let panel_content = serde_yaml::to_string(&data).unwrap_or_else(|_| format!("{data:#}"));
                    let dyn_panel = DynPanel {
                        context_type: crate::panel::FIRECRAWL_PANEL_TYPE.to_string(),
                        display_name: format!("firecrawl_search: {query}"),
                        metadata: vec![("result_content".to_string(), panel_content.clone())],
                        content: Some(panel_content),
                    };
                    return ToolOutput {
                        content: format!(
                            "Created panel {DYN_PANEL_ID_PLACEHOLDER}: results for '{query}'{PANEL_WARNING}",
                        ),
                        is_error: false,
                        create_panel: Some(dyn_panel),
                        preserves_tempo: false,
                    };
                };

                let count = results.len();
                if count == 0 {
                    return ToolOutput {
                        content: format!("No results found for '{query}'"),
                        is_error: false,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                // Build panel: concatenated markdown per page
                let mut content = String::new();
                for (i, result) in results.iter().enumerate() {
                    let page_title = result.title.as_deref().unwrap_or("untitled");
                    let page_url = result.url.as_deref().unwrap_or("unknown");
                    let _r1 = write!(content, "## Result {} — {} ({})\n\n", i.saturating_add(1), page_title, page_url);
                    if let Some(ref md) = result.markdown {
                        content.push_str(md);
                        content.push_str("\n\n");
                    } else if let Some(ref desc) = result.description {
                        content.push_str(desc);
                        content.push_str("\n\n");
                    }
                    if let Some(ref links) = result.links
                        && !links.is_empty()
                    {
                        content.push_str("**Links:**\n");
                        for link in links.iter().take(10) {
                            let _r2 = writeln!(content, "- {link}");
                        }
                        content.push('\n');
                    }
                    content.push_str("---\n\n");
                }

                let dyn_panel = DynPanel {
                    context_type: crate::panel::FIRECRAWL_PANEL_TYPE.to_string(),
                    display_name: format!("firecrawl_search: {query}"),
                    metadata: vec![("result_content".to_string(), content.clone())],
                    content: Some(content),
                };

                ToolOutput {
                    content: format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {count} results for '{query}'{PANEL_WARNING}",
                    ),
                    is_error: false,
                    create_panel: Some(dyn_panel),
                    preserves_tempo: false,
                }
            }
            Err(e) => ToolOutput { content: e, is_error: true, create_panel: None, preserves_tempo: false },
        }
    })
}
/// Execute the `firecrawl_map` tool: discover all URLs on a domain.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_map(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_map");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_string());
    };

    // Extract all params to owned types for the closure
    let url = url.to_string();
    let limit = tool.input.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(50).to_u32();
    let search_val = tool.input.get("search").and_then(|v| v.as_str()).map(String::from);
    let include_subdomains = tool.input.get("include_subdomains").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let country_val = tool
        .input
        .get("location")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("country"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let languages_val: Option<Vec<String>> = tool
        .input
        .get("location")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("languages"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_MAP_SECS, move || {
        let langs_refs: Option<Vec<&str>> = languages_val.as_ref().map(|v| v.iter().map(String::as_str).collect());

        let params = MapParams {
            url: &url,
            limit,
            search: search_val.as_deref(),
            include_subdomains,
            country: country_val.as_deref(),
            languages: langs_refs,
        };

        match client.map(&params) {
            Ok(resp) => {
                if !resp.success {
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                    return ToolOutput {
                        content: format!("Firecrawl map failed: {msg}"),
                        is_error: true,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let links = resp.links.unwrap_or_default();
                let count = links.len();

                if count == 0 {
                    return ToolOutput {
                        content: format!("No URLs discovered on '{url}'"),
                        is_error: false,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let panel_content = match serde_yaml::to_string(&links) {
                    Ok(yaml) => yaml,
                    Err(e) => {
                        return ToolOutput {
                            content: format!("Failed to serialize response: {e}"),
                            is_error: true,
                            create_panel: None,
                            preserves_tempo: false,
                        };
                    }
                };

                let domain =
                    url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(&url);

                let dyn_panel = DynPanel {
                    context_type: crate::panel::FIRECRAWL_PANEL_TYPE.to_string(),
                    display_name: format!("firecrawl_map: {domain}"),
                    metadata: vec![("result_content".to_string(), panel_content.clone())],
                    content: Some(panel_content),
                };

                ToolOutput {
                    content: format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {count} URLs discovered on '{domain}'{PANEL_WARNING}",
                    ),
                    is_error: false,
                    create_panel: Some(dyn_panel),
                    preserves_tempo: false,
                }
            }
            Err(e) => ToolOutput { content: e, is_error: true, create_panel: None, preserves_tempo: false },
        }
    })
}
