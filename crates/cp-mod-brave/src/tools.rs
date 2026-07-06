use cp_base::state::runtime::State;
use cp_base::state::watchers::{DYN_PANEL_ID_PLACEHOLDER, DynPanel};
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::api::{BraveClient, LLMContextParams, SearchParams};
use cp_base::cast::Safe as _;

/// Dispatch brave tool calls.
pub fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    match tool.name.as_str() {
        "brave_search" => Some(exec_search(tool, state)),
        "brave_llm_context" => Some(exec_llm_context(tool, state)),
        _ => None,
    }
}

/// Build a `BraveClient` using the credential vault.
fn get_client() -> Result<BraveClient, String> {
    let secret = cp_vault::vault().require("brave").map_err(|e| e.to_string())?;
    BraveClient::new(secret.expose().to_owned())
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

/// Warning appended to every panel-creating tool result.
///
/// Prevents the LLM from closing result panels before acting on their content.
/// Closing a panel causes instant, irreversible context loss.
const PANEL_WARNING: &str = "\n\nIMPORTANT: Results live in this panel. Act on the information FIRST (write \
    files, answer questions, store in scratchpad, etc.), THEN close the panel. Closing it IMMEDIATELY and \
    IRREVERSIBLY erases all content from your context — you cannot recall it from memory afterward. \
    Never close-then-act; always act-then-close.";

/// Async timeout for Brave API calls (seconds).
/// The HTTP client has a 10s per-request timeout, but retries on 5xx (up to 3 attempts).
const ASYNC_TIMEOUT_SECS: u64 = 35;

/// Execute the `brave_search` tool: web search with snippet results.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("brave_search");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    // Extract all params to owned types for the closure
    let query = query.to_string();
    let count = tool.input.get("count").and_then(serde_json::Value::as_u64).unwrap_or(5).to_u32();
    let freshness = tool.input.get("freshness").and_then(|v| v.as_str()).map(String::from);
    let country = tool.input.get("country").and_then(|v| v.as_str()).unwrap_or("US").to_string();
    let search_lang = tool.input.get("search_lang").and_then(|v| v.as_str()).unwrap_or("en").to_string();
    let safe_search = tool.input.get("safe_search").and_then(|v| v.as_str()).unwrap_or("moderate").to_string();
    let goggles_id = tool.input.get("goggles_id").and_then(|v| v.as_str()).map(String::from);

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SECS, move || {
        let params = SearchParams {
            query: &query,
            count,
            freshness: freshness.as_deref(),
            country: &country,
            search_lang: &search_lang,
            safe_search: &safe_search,
            goggles_id: goggles_id.as_deref(),
        };

        match client.search(&params) {
            Ok((search_resp, rich_data)) => {
                let result_count = search_resp.web.as_ref().map_or(0, |w| w.results.len());

                if result_count == 0 && rich_data.is_none() {
                    return ToolOutput {
                        content: format!("No results found for '{query}'"),
                        is_error: false,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                // Build panel content as YAML
                let mut panel_content = String::new();
                if let Some(ref rich) = rich_data {
                    panel_content.push_str("# Rich Results\n\n");
                    if let Ok(yaml) = serde_yaml::to_string(rich) {
                        panel_content.push_str(&yaml);
                    }
                    panel_content.push_str("\n---\n\n");
                }
                panel_content.push_str("# Web Results\n\n");
                if let Ok(yaml) = serde_yaml::to_string(&search_resp) {
                    panel_content.push_str(&yaml);
                }

                let dyn_panel = DynPanel {
                    context_type: crate::panel::BRAVE_PANEL_TYPE.to_string(),
                    display_name: format!("brave_search: {query}"),
                    metadata: vec![("result_content".to_string(), panel_content.clone())],
                    content: Some(panel_content),
                };

                ToolOutput {
                    content: format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {result_count} results for '{query}'{PANEL_WARNING}",
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

/// Execute the `brave_llm_context` tool: LLM-optimized content extraction.
///
/// Runs the HTTP call on a worker thread to avoid blocking the main event loop.
fn exec_llm_context(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("brave_llm_context");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    // Extract all params to owned types for the closure
    let query = query.to_string();
    let max_tokens =
        tool.input.get("maximum_number_of_tokens").and_then(serde_json::Value::as_u64).unwrap_or(8192).to_u32();
    let count = tool.input.get("count").and_then(serde_json::Value::as_u64).unwrap_or(20).to_u32();
    let threshold_mode =
        tool.input.get("context_threshold_mode").and_then(|v| v.as_str()).unwrap_or("balanced").to_string();
    let freshness = tool.input.get("freshness").and_then(|v| v.as_str()).map(String::from);
    let country = tool.input.get("country").and_then(|v| v.as_str()).unwrap_or("US").to_string();
    let goggles = tool.input.get("goggles").and_then(|v| v.as_str()).map(String::from);

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SECS, move || {
        let params = LLMContextParams {
            query: &query,
            max_tokens,
            count,
            threshold_mode: &threshold_mode,
            freshness: freshness.as_deref(),
            country: &country,
            goggles: goggles.as_deref(),
        };

        match client.llm_context(&params) {
            Ok(resp) => {
                let url_count = resp.grounding.as_ref().and_then(|g| g.generic.as_ref()).map_or(0, Vec::len);

                if url_count == 0 {
                    return ToolOutput {
                        content: format!("No context found for '{query}'"),
                        is_error: false,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let panel_content = match serde_yaml::to_string(&resp) {
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

                let dyn_panel = DynPanel {
                    context_type: crate::panel::BRAVE_PANEL_TYPE.to_string(),
                    display_name: format!("brave_llm_context: {query}"),
                    metadata: vec![("result_content".to_string(), panel_content.clone())],
                    content: Some(panel_content),
                };

                ToolOutput {
                    content: format!(
                        "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {url_count} URLs, ~{max_tokens} tokens for '{query}'{PANEL_WARNING}",
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
