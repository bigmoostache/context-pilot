use cp_base::state::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::api::{FirecrawlClient, MapParams, ScrapeParams, SearchParams};

/// Dispatch firecrawl tool calls.
pub fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "firecrawl_scrape" => Some(exec_scrape(tool, state)),
        "firecrawl_search" => Some(exec_search(tool, state)),
        "firecrawl_map" => Some(exec_map(tool, state)),
        _ => None,
    }
}

fn get_client() -> Result<FirecrawlClient, String> {
    let key = std::env::var("FIRECRAWL_API_KEY").map_err(|_| "FIRECRAWL_API_KEY not set".to_string())?;
    Ok(FirecrawlClient::new(key))
}

fn ok_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, is_error: false, tool_name: tool.name.clone() }
}

fn err_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, is_error: true, tool_name: tool.name.clone() }
}

fn exec_scrape(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_string());
    };

    // Parse formats: default ["markdown", "links"]
    let formats_val: Vec<String> = tool
        .input
        .get("formats")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| vec!["markdown".to_string(), "links".to_string()]);
    let formats: Vec<&str> = formats_val.iter().map(|s| s.as_str()).collect();

    // Parse location
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
    let languages_refs: Option<Vec<&str>> = languages_val.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());

    let params = ScrapeParams { url, formats, country: country_val.as_deref(), languages: languages_refs };

    match client.scrape(&params) {
        Ok(resp) => {
            if !resp.success {
                let msg = resp.error.unwrap_or_else(|| "Unknown error".to_string());
                return err_result(tool, format!("Firecrawl scrape failed: {}", msg));
            }

            let Some(data) = resp.data else { return err_result(tool, "Scrape returned no data".to_string()) };

            let title = data.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or("untitled");

            // Build panel content
            let mut content = String::new();

            // Metadata header
            if let Some(ref meta) = data.metadata {
                content.push_str("## Metadata\n\n");
                if let Some(ref t) = meta.title {
                    content.push_str(&format!("**Title:** {}\n", t));
                }
                if let Some(ref d) = meta.description {
                    content.push_str(&format!("**Description:** {}\n", d));
                }
                if let Some(ref u) = meta.source_url {
                    content.push_str(&format!("**URL:** {}\n", u));
                }
                content.push('\n');
            }

            // Markdown content
            if let Some(ref md) = data.markdown {
                content.push_str("## Content\n\n");
                content.push_str(md);
                content.push_str("\n\n");
            }

            // Links
            if let Some(ref links) = data.links
                && !links.is_empty()
            {
                content.push_str("## Links\n\n");
                for link in links {
                    content.push_str(&format!("- {}\n", link));
                }
            }

            let panel_id = crate::panel::create_panel(state, &format!("firecrawl_scrape: {}", url), &content);

            ok_result(tool, format!("Created panel {}: scraped {} ({})", panel_id, url, title))
        }
        Err(e) => err_result(tool, e),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    let limit = tool.input.get("limit").and_then(|v| v.as_u64()).unwrap_or(3) as u32;

    let sources_val: Vec<String> = tool
        .input
        .get("sources")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| vec!["web".to_string()]);
    let sources: Vec<&str> = sources_val.iter().map(|s| s.as_str()).collect();

    let cats_val: Option<Vec<String>> = tool
        .input
        .get("categories")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let cats_refs: Option<Vec<&str>> = cats_val.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());

    let tbs_val = tool.input.get("tbs").and_then(|v| v.as_str()).map(String::from);
    let loc_val = tool.input.get("location").and_then(|v| v.as_str()).map(String::from);

    let params = SearchParams {
        query,
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
                return err_result(tool, format!("Firecrawl search failed: {}", msg));
            }

            let Some(data) = resp.data else { return ok_result(tool, format!("No results found for '{}'", query)) };

            // Parse data — can be array (scraped results) or object (web/news/images dict)
            let results: Vec<crate::types::SearchResult> = if data.is_array() {
                serde_json::from_value(data).unwrap_or_default()
            } else if let Some(web_arr) = data.get("web").and_then(|v| v.as_array()) {
                web_arr.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
            } else {
                // Fallback: dump as YAML
                let panel_content = serde_yaml::to_string(&data).unwrap_or_else(|_| format!("{:#}", data));
                let panel_id =
                    crate::panel::create_panel(state, &format!("firecrawl_search: {}", query), &panel_content);
                return ok_result(tool, format!("Created panel {}: results for '{}'", panel_id, query));
            };

            let count = results.len();

            if count == 0 {
                return ok_result(tool, format!("No results found for '{}'", query));
            }

            // Build panel: concatenated markdown per page
            let mut content = String::new();
            for (i, result) in results.iter().enumerate() {
                let page_title = result.title.as_deref().unwrap_or("untitled");
                let page_url = result.url.as_deref().unwrap_or("unknown");

                content.push_str(&format!("## Result {} — {} ({})\n\n", i + 1, page_title, page_url));

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
                        content.push_str(&format!("- {}\n", link));
                    }
                    content.push('\n');
                }

                content.push_str("---\n\n");
            }

            let panel_id = crate::panel::create_panel(state, &format!("firecrawl_search: {}", query), &content);

            ok_result(tool, format!("Created panel {}: {} results for '{}'", panel_id, count, query))
        }
        Err(e) => err_result(tool, e),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn exec_map(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_string());
    };

    let limit = tool.input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
    let search_val = tool.input.get("search").and_then(|v| v.as_str()).map(String::from);
    let include_subdomains = tool.input.get("include_subdomains").and_then(|v| v.as_bool()).unwrap_or(false);

    // Parse location
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
    let langs_refs: Option<Vec<&str>> = languages_val.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());

    let params = MapParams {
        url,
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
                return err_result(tool, format!("Firecrawl map failed: {}", msg));
            }

            let links = resp.links.unwrap_or_default();
            let count = links.len();

            if count == 0 {
                return ok_result(tool, format!("No URLs discovered on '{}'", url));
            }

            // YAML panel for URL list
            let panel_content = match serde_yaml::to_string(&links) {
                Ok(yaml) => yaml,
                Err(e) => return err_result(tool, format!("Failed to serialize response: {}", e)),
            };

            // Extract domain for title
            let domain =
                url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(url);

            let panel_id = crate::panel::create_panel(state, &format!("firecrawl_map: {}", domain), &panel_content);

            ok_result(tool, format!("Created panel {}: {} URLs discovered on '{}'", panel_id, count, domain))
        }
        Err(e) => err_result(tool, e),
    }
}
