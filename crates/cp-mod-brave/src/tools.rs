use cp_base::state::runtime::State;
use cp_base::state::watchers::DYN_PANEL_ID_PLACEHOLDER;
use cp_base::state::watchers::carriers::DynPanel;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::api::{BraveClient, LLMContextParams, SearchParams};
use std::ops::RangeInclusive;

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
    IRREVERSIBLY erases all content from your context \u{2014} you cannot recall it from memory afterward. \
    Never close-then-act; always act-then-close.";

/// Async timeout for Brave API calls (seconds). Kept above [`crate::api::TOTAL_BUDGET`]
/// so the client's own budget always fires first with a specific error.
pub(crate) const ASYNC_TIMEOUT_SECS: u64 = 35;

/// Smallest `maximum_number_of_tokens` the LLM context API accepts.
const MIN_TOKENS: u32 = 1024;
/// Largest `maximum_number_of_tokens` the LLM context API accepts (32768).
const MAX_TOKENS: u32 = 0x8000;

/// Read an optional string parameter.
fn opt_str(tool: &ToolUse, key: &str) -> Option<String> {
    tool.input.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// Read a string parameter with a default.
fn str_or(tool: &ToolUse, key: &str, default: &str) -> String {
    opt_str(tool, key).unwrap_or_else(|| default.to_owned())
}

/// Read an integer parameter, clamped to the range the Brave API accepts.
/// Out-of-range values would otherwise come back as an opaque HTTP 422.
pub(crate) fn clamped(input: &serde_json::Value, key: &str, default: u32, range: RangeInclusive<u32>) -> u32 {
    input
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map_or(default, cp_base::cast::Safe::to_u32)
        .clamp(*range.start(), *range.end())
}

/// Wrap rendered content in a persisted dynamic panel.
fn result_panel(title: String, content: String) -> DynPanel {
    // `metadata` is the only copy that survives a reload (panel.rs restores from it).
    DynPanel::new(crate::panel::BRAVE_PANEL_TYPE.to_owned(), title)
        .metadata(vec![("result_content".to_owned(), content.clone())])
        .content(content)
}

/// Execute the `brave_search` tool: web search with snippet results, off the main loop.
fn exec_search(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("brave_search");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };
    let Some(query) = opt_str(tool, "query") else {
        return err_result(tool, "Missing required parameter 'query'".to_owned());
    };
    let count = clamped(&tool.input, "count", 5, 1..=20);
    let freshness = opt_str(tool, "freshness");
    let country = str_or(tool, "country", "US");
    let search_lang = str_or(tool, "search_lang", "en");
    let safe_search = str_or(tool, "safe_search", "moderate");
    let goggles_id = opt_str(tool, "goggles_id");

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
        let outcome = match client.search(&params) {
            Ok(o) => o,
            Err(e) => return ToolOutput::error(e),
        };
        let note = outcome.rich_error.map(|e| format!("\nRich results unavailable: {e}")).unwrap_or_default();
        let result_count = outcome.response.web.as_ref().map_or(0, |w| w.results.len());
        if result_count == 0 && outcome.rich.is_none() {
            return ToolOutput::ok(format!("No results found for '{query}'{note}"));
        }
        let content = crate::render::search(&outcome.response, outcome.rich.as_ref());
        ToolOutput::ok(format!(
            "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {result_count} results for '{query}'{note}{PANEL_WARNING}",
        ))
        .with_panel(result_panel(format!("brave_search: {query}"), content))
    })
}

/// Execute the `brave_llm_context` tool: LLM-optimized content extraction, off the main loop.
fn exec_llm_context(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("brave_llm_context");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };
    let Some(query) = opt_str(tool, "query") else {
        return err_result(tool, "Missing required parameter 'query'".to_owned());
    };
    let max_tokens = clamped(&tool.input, "maximum_number_of_tokens", 8192, MIN_TOKENS..=MAX_TOKENS);
    let count = clamped(&tool.input, "count", 20, 1..=50);
    let threshold_mode = str_or(tool, "context_threshold_mode", "balanced");
    let freshness = opt_str(tool, "freshness");
    let country = str_or(tool, "country", "US");
    let goggles = opt_str(tool, "goggles");

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
        let resp = match client.llm_context(&params) {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(e),
        };
        let url_count = resp.grounding.as_ref().and_then(|g| g.generic.as_ref()).map_or(0, Vec::len);
        if url_count == 0 {
            return ToolOutput::ok(format!("No context found for '{query}'"));
        }
        let content = crate::render::llm_context(&resp);
        ToolOutput::ok(format!(
            "Created panel {DYN_PANEL_ID_PLACEHOLDER}: {url_count} URLs, ~{max_tokens} tokens for '{query}'{PANEL_WARNING}",
        ))
        .with_panel(result_panel(format!("brave_llm_context: {query}"), content))
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_TOKENS, MIN_TOKENS, clamped};

    #[test]
    fn numeric_params_are_clamped() {
        let input = serde_json::json!({"count": 500i32, "low": 0i32, "neg": -3i32});
        assert_eq!(clamped(&input, "count", 5, 1..=20), 20);
        assert_eq!(clamped(&input, "low", 5, 1..=20), 1);
        assert_eq!(clamped(&input, "neg", 5, 1..=20), 5);
        assert_eq!(clamped(&input, "missing", 8192, MIN_TOKENS..=MAX_TOKENS), 8192);
    }
}
