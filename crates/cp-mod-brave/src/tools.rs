use cp_base::state::runtime::State;
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

/// Build a `BraveClient` from the `BRAVE_API_KEY` env var.
fn get_client() -> Result<BraveClient, String> {
    let api_key = std::env::var("BRAVE_API_KEY").map_err(|_e| "BRAVE_API_KEY not set".to_string())?;
    BraveClient::new(api_key)
}

/// Build a successful `ToolResult`.
fn ok_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, display: None, is_error: false, tool_name: tool.name.clone() }
}

/// Build an error `ToolResult`.
fn err_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, display: None, is_error: true, tool_name: tool.name.clone() }
}
/// Execute the `brave_search` tool: web search with snippet results.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    let count = tool.input.get("count").and_then(serde_json::Value::as_u64).unwrap_or(5).to_u32();
    let freshness_val = tool.input.get("freshness").and_then(|v| v.as_str()).map(String::from);
    let country_val = tool.input.get("country").and_then(|v| v.as_str()).unwrap_or("US");
    let search_lang = tool.input.get("search_lang").and_then(|v| v.as_str()).unwrap_or("en");
    let safe_search = tool.input.get("safe_search").and_then(|v| v.as_str()).unwrap_or("moderate");
    let goggles_val = tool.input.get("goggles_id").and_then(|v| v.as_str()).map(String::from);

    let params = SearchParams {
        query,
        count,
        freshness: freshness_val.as_deref(),
        country: country_val,
        search_lang,
        safe_search,
        goggles_id: goggles_val.as_deref(),
    };

    match client.search(&params) {
        Ok((search_resp, rich_data)) => {
            let result_count = search_resp.web.as_ref().map_or(0, |w| w.results.len());

            if result_count == 0 && rich_data.is_none() {
                return ok_result(tool, format!("No results found for '{query}'"));
            }

            // Build panel content as YAML
            let mut panel_content = String::new();

            // Add rich results at top if present
            if let Some(ref rich) = rich_data {
                panel_content.push_str("# Rich Results\n\n");
                if let Ok(yaml) = serde_yaml::to_string(rich) {
                    panel_content.push_str(&yaml);
                }
                panel_content.push_str("\n---\n\n");
            }

            // Add web results
            panel_content.push_str("# Web Results\n\n");
            if let Ok(yaml) = serde_yaml::to_string(&search_resp) {
                panel_content.push_str(&yaml);
            }

            // Create dynamic panel
            let panel_id = crate::panel::create(state, &format!("brave_search: {query}"), &panel_content);

            ok_result(tool, format!("Created panel {panel_id}: {result_count} results for '{query}'"))
        }
        Err(e) => err_result(tool, e),
    }
}
/// Execute the `brave_llm_context` tool: LLM-optimized content extraction.
fn exec_llm_context(tool: &ToolUse, state: &mut State) -> ToolResult {
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(query) = tool.input.get("query").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'query'".to_string());
    };

    let max_tokens =
        tool.input.get("maximum_number_of_tokens").and_then(serde_json::Value::as_u64).unwrap_or(8192).to_u32();
    let count = tool.input.get("count").and_then(serde_json::Value::as_u64).unwrap_or(20).to_u32();
    let threshold_mode = tool.input.get("context_threshold_mode").and_then(|v| v.as_str()).unwrap_or("balanced");
    let freshness_val = tool.input.get("freshness").and_then(|v| v.as_str()).map(String::from);
    let country_val = tool.input.get("country").and_then(|v| v.as_str()).unwrap_or("US");
    let goggles_val = tool.input.get("goggles").and_then(|v| v.as_str()).map(String::from);

    let params = LLMContextParams {
        query,
        max_tokens,
        count,
        threshold_mode,
        freshness: freshness_val.as_deref(),
        country: country_val,
        goggles: goggles_val.as_deref(),
    };

    match client.llm_context(&params) {
        Ok(resp) => {
            let url_count = resp.grounding.as_ref().and_then(|g| g.generic.as_ref()).map_or(0, Vec::len);

            if url_count == 0 {
                return ok_result(tool, format!("No context found for '{query}'"));
            }

            // Build panel content as YAML
            let panel_content = match serde_yaml::to_string(&resp) {
                Ok(yaml) => yaml,
                Err(e) => return err_result(tool, format!("Failed to serialize response: {e}")),
            };

            let panel_id = crate::panel::create(state, &format!("brave_llm_context: {query}"), &panel_content);

            ok_result(tool, format!("Created panel {panel_id}: {url_count} URLs, ~{max_tokens} tokens for '{query}'"))
        }
        Err(e) => err_result(tool, e),
    }
}
