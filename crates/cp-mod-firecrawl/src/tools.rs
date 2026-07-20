use cp_base::state::runtime::State;
use cp_base::state::watchers::DYN_PANEL_ID_PLACEHOLDER;
use cp_base::state::watchers::carriers::DynPanel;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::api::{FirecrawlClient, MapParams, ScrapeParams, SearchParams};
use cp_base::cast::Safe as _;
use std::fmt::Write as _;
use std::time::Duration;

/// Dispatch firecrawl tool calls.
pub fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "firecrawl_scrape" => Some(exec_scrape(tool, state)),
        "firecrawl_search" => Some(exec_search(tool, state)),
        "firecrawl_map" => Some(exec_map(tool, state)),
        "firecrawl_crawl" => Some(crate::crawl::exec_crawl(tool, state)),
        _ => None,
    }
}

/// Build a `FirecrawlClient` using the credential vault.
pub(crate) fn get_client() -> Result<FirecrawlClient, String> {
    let secret = cp_vault::vault().require("firecrawl").map_err(|e| e.to_string())?;
    FirecrawlClient::new(secret.expose().to_owned())
}

/// Build an error `ToolResult` for sync validation failures.
pub(crate) fn err_result(tool: &ToolUse, content: String) -> ToolResult {
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
    IRREVERSIBLY erases all content from your context \u{2014} you cannot recall it from memory afterward. \
    Never close-then-act; always act-then-close.";

/// Async timeout for Firecrawl scrape/search calls (seconds).
/// JS rendering + multi-page scraping can be slow.
const ASYNC_TIMEOUT_SCRAPE_SECS: u64 = 60;

/// Async timeout for Firecrawl map calls (seconds).
/// Map is faster — just sitemap/URL discovery.
const ASYNC_TIMEOUT_MAP_SECS: u64 = 30;

/// Async timeout for Firecrawl crawl jobs (seconds).
/// Crawls are long-running — up to 5 minutes.
pub(crate) const ASYNC_TIMEOUT_CRAWL_SECS: u64 = 310;

/// Polling interval between crawl status checks.
pub(crate) const CRAWL_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Maximum number of poll iterations before giving up internally.
pub(crate) const CRAWL_MAX_POLLS: u32 = 60;

/// Append the `## Metadata` section (title / description / URL) when present.
fn push_scrape_metadata(content: &mut String, data: &crate::types::ScrapeData) {
    let Some(meta) = data.metadata.as_ref() else {
        return;
    };
    content.push_str("## Metadata\n\n");
    if let Some(t) = meta.title.as_ref() {
        let _r = writeln!(content, "**Title:** {t}");
    }
    if let Some(d) = meta.description.as_ref() {
        let _r = writeln!(content, "**Description:** {d}");
    }
    if let Some(u) = meta.source_url.as_ref() {
        let _r = writeln!(content, "**URL:** {u}");
    }
    content.push('\n');
}

/// Build the markdown panel body for a scrape result (metadata + content + links).
fn build_scrape_panel_content(data: &crate::types::ScrapeData) -> String {
    let mut content = String::new();
    push_scrape_metadata(&mut content, data);
    if let Some(md) = data.markdown.as_ref() {
        content.push_str("## Content\n\n");
        content.push_str(md);
        content.push_str("\n\n");
    }
    if let Some(links) = data.links.as_ref()
        && !links.is_empty()
    {
        content.push_str("## Links\n\n");
        for link in links {
            let _r = writeln!(content, "- {link}");
        }
    }
    content
}

/// Execute the `firecrawl_scrape` tool: scrape a single URL for content.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_scrape(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_scrape");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url_ref) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_owned());
    };

    // Extract all params to owned types for the closure
    let url = url_ref.to_owned();
    let formats_val: Vec<String> = tool.input.get("formats").and_then(|v| v.as_array()).map_or_else(
        || vec!["markdown".to_owned(), "links".to_owned()],
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
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_owned());
                    return ToolOutput::error(format!("Firecrawl scrape failed: {msg}"));
                }

                let Some(data) = resp.data else {
                    return ToolOutput::error("Scrape returned no data".to_owned());
                };

                let title = data.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or("untitled").to_owned();
                let content = build_scrape_panel_content(&data);

                let dyn_panel =
                    DynPanel::new(crate::panel::FIRECRAWL_PANEL_TYPE.to_owned(), format!("firecrawl_scrape: {url}"))
                        .metadata(vec![("result_content".to_owned(), content.clone())])
                        .content(content);

                ToolOutput::ok(format!(
                    "Created panel {DYN_PANEL_ID_PLACEHOLDER}: scraped {url} ({title}){PANEL_WARNING}",
                ))
                .with_panel(dyn_panel)
            }
            Err(e) => ToolOutput::error(e),
        }
    })
}
/// Build the markdown panel body from scraped search results (one section each).
fn build_search_results_content(results: &[crate::types::SearchResult]) -> String {
    let mut content = String::new();
    for (i, result) in results.iter().enumerate() {
        let page_title = result.title.as_deref().unwrap_or("untitled");
        let page_url = result.url.as_deref().unwrap_or("unknown");
        let _r1 = write!(content, "## Result {} — {} ({})\n\n", i.saturating_add(1), page_title, page_url);
        if let Some(md) = result.markdown.as_ref() {
            content.push_str(md);
            content.push_str("\n\n");
        } else if let Some(desc) = result.description.as_ref() {
            content.push_str(desc);
            content.push_str("\n\n");
        } else {
            // Neither markdown nor description present — nothing to append.
        }
        if let Some(links) = result.links.as_ref()
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
    content
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

    let Some(query_ref) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_owned());
    };

    // Extract all params to owned types for the closure
    let query = query_ref.to_owned();
    let limit = tool.input.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(3).to_u32();
    let sources_val: Vec<String> = tool.input.get("sources").and_then(|v| v.as_array()).map_or_else(
        || vec!["web".to_owned()],
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
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_owned());
                    return ToolOutput::error(format!("Firecrawl search failed: {msg}"));
                }

                let Some(data) = resp.data else {
                    return ToolOutput::ok(format!("No results found for '{query}'"));
                };

                // Parse data — can be array (scraped results) or object (web/news/images dict)
                let results: Vec<crate::types::SearchResult> = if data.is_array() {
                    serde_json::from_value(data).unwrap_or_default()
                } else if let Some(web_arr) = data.get("web").and_then(|v| v.as_array()) {
                    web_arr.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
                } else {
                    // Fallback: dump as YAML
                    let panel_content = serde_yaml::to_string(&data).unwrap_or_else(|_| format!("{data:#}"));
                    let dyn_panel = DynPanel::new(
                        crate::panel::FIRECRAWL_PANEL_TYPE.to_owned(),
                        format!("firecrawl_search: {query}"),
                    )
                    .metadata(vec![("result_content".to_owned(), panel_content.clone())])
                    .content(panel_content);
                    return ToolOutput::ok(format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: results for '{query}'{PANEL_WARNING}",
                    ))
                    .with_panel(dyn_panel);
                };

                let count = results.len();
                if count == 0 {
                    return ToolOutput::ok(format!("No results found for '{query}'"));
                }

                // Build panel: concatenated markdown per page
                let content = build_search_results_content(&results);

                let dyn_panel =
                    DynPanel::new(crate::panel::FIRECRAWL_PANEL_TYPE.to_owned(), format!("firecrawl_search: {query}"))
                        .metadata(vec![("result_content".to_owned(), content.clone())])
                        .content(content);

                ToolOutput::ok(format!(
                    "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {count} results for '{query}'{PANEL_WARNING}",
                ))
                .with_panel(dyn_panel)
            }
            Err(e) => ToolOutput::error(e),
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

    let Some(url_ref) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_owned());
    };

    // Extract all params to owned types for the closure
    let url = url_ref.to_owned();
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
                    let msg = resp.error.unwrap_or_else(|| "Unknown error".to_owned());
                    return ToolOutput::error(format!("Firecrawl map failed: {msg}"));
                }

                let links = resp.links.unwrap_or_default();
                let count = links.len();

                if count == 0 {
                    return ToolOutput::ok(format!("No URLs discovered on '{url}'"));
                }

                let panel_content = match serde_yaml::to_string(&links) {
                    Ok(yaml) => yaml,
                    Err(e) => {
                        return ToolOutput::error(format!("Failed to serialize response: {e}"));
                    }
                };

                let domain =
                    url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(&url);

                let dyn_panel =
                    DynPanel::new(crate::panel::FIRECRAWL_PANEL_TYPE.to_owned(), format!("firecrawl_map: {domain}"))
                        .metadata(vec![("result_content".to_owned(), panel_content.clone())])
                        .content(panel_content);

                ToolOutput::ok(format!(
                    "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {count} URLs discovered on '{domain}'{PANEL_WARNING}",
                ))
                .with_panel(dyn_panel)
            }
            Err(e) => ToolOutput::error(e),
        }
    })
}
