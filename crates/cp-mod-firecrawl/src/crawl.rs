//! Recursive site crawling — `firecrawl_crawl` tool implementation.

use cp_base::cast::Safe;
use cp_base::state::runtime::State;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};
use std::fmt::Write as _;

use crate::api::CrawlParams;
use crate::tools::{ASYNC_TIMEOUT_CRAWL_SECS, CRAWL_MAX_POLLS, CRAWL_POLL_INTERVAL, err_result, get_client};

/// Execute the `firecrawl_crawl` tool: recursively crawl a site.
///
/// Starts an async crawl job, polls until complete, then writes combined
/// markdown output to the specified path. No panel is created — the file
/// is the deliverable.
pub(crate) fn exec_crawl(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_crawl");
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let Some(url) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_owned());
    };
    let Some(output) = tool.input.get("output").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'output'".to_owned());
    };

    let url = url.to_owned();
    let output = std::path::PathBuf::from(output);
    let limit = tool.input.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(10).min(100).to_u32();
    let max_depth = tool.input.get("max_depth").and_then(serde_json::Value::as_u64).map(Safe::to_u32);
    let include_paths: Option<Vec<String>> = tool
        .input
        .get("include_paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let exclude_paths: Option<Vec<String>> = tool
        .input
        .get("exclude_paths")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let allow_subdomains = tool.input.get("allow_subdomains").and_then(serde_json::Value::as_bool).unwrap_or(false);

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_CRAWL_SECS, move || {
        let inc_refs: Option<Vec<&str>> = include_paths.as_ref().map(|v| v.iter().map(String::as_str).collect());
        let exc_refs: Option<Vec<&str>> = exclude_paths.as_ref().map(|v| v.iter().map(String::as_str).collect());

        let params = CrawlParams {
            url: &url,
            limit,
            max_depth,
            include_paths: inc_refs,
            exclude_paths: exc_refs,
            allow_subdomains,
        };

        // 1. Start the crawl job
        let start = match client.start_crawl(&params) {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(e),
        };
        if !start.success {
            let msg = start.error.unwrap_or_else(|| "Unknown error".to_owned());
            return ToolOutput::error(format!("Crawl failed to start: {msg}"));
        }
        let Some(job_id) = start.id else {
            return ToolOutput::error("Crawl started but no job ID returned".to_owned());
        };

        // 2. Poll until complete
        let mut last_completed = 0u32;
        for _ in 0..CRAWL_MAX_POLLS {
            std::thread::sleep(CRAWL_POLL_INTERVAL);
            let status = match client.poll_crawl(&job_id) {
                Ok(s) => s,
                Err(e) => {
                    return ToolOutput::error(format!("Crawl poll failed: {e}"));
                }
            };
            last_completed = status.completed.unwrap_or(0);

            match status.status.as_str() {
                "completed" => {
                    return write_crawl_output(&url, &output, status);
                }
                "failed" => {
                    let msg = status.error.unwrap_or_else(|| "Unknown error".to_owned());
                    return ToolOutput::error(format!("Crawl failed: {msg}"));
                }
                _ => {} // still scraping, keep polling
            }
        }

        let timeout_secs = CRAWL_MAX_POLLS.saturating_mul(CRAWL_POLL_INTERVAL.as_secs().to_u32());
        ToolOutput::error(format!(
            "Crawl timed out after {timeout_secs}s ({last_completed} pages scraped). \
                 Job '{job_id}' may still be running on Firecrawl servers.",
        ))
    })
}

/// Write crawl results to a combined markdown file.
fn write_crawl_output(url: &str, output: &std::path::Path, status: crate::types::CrawlStatusResponse) -> ToolOutput {
    let pages = status.data.unwrap_or_default();
    let count = pages.len();
    let credits = status.credits_used.unwrap_or(0);

    if count == 0 {
        return ToolOutput::ok(format!("Crawl of '{url}' completed but returned 0 pages."));
    }

    let mut md = String::new();
    writeln!(md, "# Crawl: {url}\n").unwrap_or(());
    writeln!(md, "> {count} pages crawled, {credits} credits used\n").unwrap_or(());
    writeln!(md, "---\n").unwrap_or(());

    for (i, page) in pages.iter().enumerate() {
        let title = page.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or("untitled");
        let page_url = page.metadata.as_ref().and_then(|m| m.source_url.as_deref()).unwrap_or("unknown");
        writeln!(md, "## Page {} — {} ({})\n", i.saturating_add(1), title, page_url).unwrap_or(());
        if let Some(content) = &(page.markdown) {
            md.push_str(content);
            md.push_str("\n\n");
        }
        md.push_str("---\n\n");
    }

    // Write to disk
    if let Some(parent) = output.parent() {
        drop(std::fs::create_dir_all(parent));
    }
    if let Err(e) = std::fs::write(output, &md) {
        return ToolOutput::error(format!("Crawl completed ({count} pages) but failed to write output: {e}"));
    }

    ToolOutput::ok(format!(
        "Crawl of '{url}' completed: {count} pages, {credits} credits. \
             Results written to '{}'.",
        output.display(),
    ))
}
