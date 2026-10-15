//! Recursive site crawling — `firecrawl_crawl` tool implementation.
//!
//! The whole crawl (start, polls, result pagination) runs under one deadline that
//! ends before the async tool timeout, so the worker always reports its own
//! outcome. A job abandoned on timeout or error is cancelled server-side so it
//! stops consuming credits.

use cp_base::cast::Safe;
use cp_base::state::runtime::State;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use crate::api::{CrawlParams, FirecrawlClient};
use crate::tools::{err_result, get_client};
use crate::types::{CrawlPageData, CrawlStatusResponse};

/// Async timeout for the crawl tool (seconds): start + polling + pagination + write.
const ASYNC_TIMEOUT_CRAWL_SECS: u64 = 310;
/// Kept free at the end of the async timeout for the cancel request and the file write.
const RESERVE_SECS: u64 = 25;
/// Polling interval between crawl status checks.
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Upper bound on followed `next` pages (v2 splits results above ~10 MB).
const MAX_RESULT_PAGES: usize = 1000;

/// Pace and wall-clock budget of one crawl.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// Sleep between status polls.
    pub interval: Duration,
    /// Everything up to the last result page must finish within this.
    pub budget: Duration,
}

impl Timing {
    /// Production timing: fits inside [`ASYNC_TIMEOUT_CRAWL_SECS`] with [`RESERVE_SECS`] spare.
    const PRODUCTION: Self = Self {
        interval: POLL_INTERVAL,
        budget: Duration::from_secs(ASYNC_TIMEOUT_CRAWL_SECS.saturating_sub(RESERVE_SECS)),
    };
}

/// Owned crawl parameters handed to the worker thread by [`exec_crawl`].
#[derive(Debug)]
pub(crate) struct CrawlJob {
    /// Starting URL.
    pub url: String,
    /// Destination file for combined markdown output (already validated).
    pub output: PathBuf,
    /// Max pages to crawl.
    pub limit: u32,
    /// Optional max link-following depth.
    pub max_depth: Option<u32>,
    /// Optional include-path regex patterns.
    pub include_paths: Option<Vec<String>>,
    /// Optional exclude-path regex patterns.
    pub exclude_paths: Option<Vec<String>>,
    /// Whether to follow subdomain links.
    pub allow_subdomains: bool,
}

/// Run one crawl off the main loop: start the job, poll to completion, gather
/// every result page, then write combined markdown to disk.
pub(crate) fn run_crawl(mut client: FirecrawlClient, job: &CrawlJob, timing: Timing) -> ToolOutput {
    let deadline = Instant::now().checked_add(timing.budget).unwrap_or_else(Instant::now);
    client.set_deadline(deadline);

    let inc_refs: Option<Vec<&str>> = job.include_paths.as_ref().map(|v| v.iter().map(String::as_str).collect());
    let exc_refs: Option<Vec<&str>> = job.exclude_paths.as_ref().map(|v| v.iter().map(String::as_str).collect());
    let params = CrawlParams {
        url: &job.url,
        limit: job.limit,
        max_depth: job.max_depth,
        include_paths: inc_refs,
        exclude_paths: exc_refs,
        allow_subdomains: job.allow_subdomains,
    };

    let start = match client.start_crawl(&params) {
        Ok(r) => r,
        Err(e) => return ToolOutput::error(format!("Crawl failed to start: {e}")),
    };
    if !start.success {
        let msg = start.error.unwrap_or_else(|| "Unknown error".to_owned());
        return ToolOutput::error(format!("Crawl failed to start: {msg}"));
    }
    let Some(job_id) = start.id else {
        return ToolOutput::error("Crawl started but no job ID returned".to_owned());
    };

    let mut completed = 0u32;
    let timed_out = |n: u32| format!("timed out after {}s ({n} pages scraped)", timing.budget.as_secs());
    loop {
        if Instant::now().checked_add(timing.interval).is_none_or(|t| t >= deadline) {
            return abandon(&client, &job_id, &timed_out(completed));
        }
        std::thread::sleep(timing.interval);
        let status = match client.poll_crawl(&job_id) {
            Ok(s) => s,
            // A poll cut short by the deadline is a timeout, not a poll failure.
            Err(_) if Instant::now() >= deadline => return abandon(&client, &job_id, &timed_out(completed)),
            Err(e) => return abandon(&client, &job_id, &format!("poll failed: {e}")),
        };
        completed = status.completed.unwrap_or(completed);
        match status.status.as_str() {
            "completed" => {
                let collected = collect_pages(&client, status);
                return write_output(&job.url, &job.output, &collected);
            }
            "failed" | "cancelled" => {
                let msg = status.error.unwrap_or_else(|| status.status.clone());
                return ToolOutput::error(format!("Crawl {job_id} ended without results: {msg}"));
            }
            _ => {} // still scraping
        }
    }
}

/// Cancel an abandoned job (best effort) and report why it was abandoned.
fn abandon(client: &FirecrawlClient, job_id: &str, why: &str) -> ToolOutput {
    let cancel = match client.cancel_crawl(job_id) {
        Ok(()) => "The job was cancelled on Firecrawl's side.".to_owned(),
        Err(e) => format!("Cancelling the job failed ({e}): it may still be running and consuming credits."),
    };
    ToolOutput::error(format!("Crawl {job_id} {why}. {cancel}"))
}

/// Result pages of a finished crawl.
#[derive(Debug)]
struct Collected {
    /// Every page retrieved.
    pages: Vec<CrawlPageData>,
    /// Credits Firecrawl reports for the job.
    credits: u32,
    /// What is missing, when pagination stopped early.
    note: Option<String>,
}

/// Gather the first status page plus every `next` page. On a pagination error
/// the pages gathered so far are kept and `note` says what is missing.
fn collect_pages(client: &FirecrawlClient, first: CrawlStatusResponse) -> Collected {
    let credits = first.credits_used.unwrap_or(0);
    let total = first.total;
    let mut pages = first.data.unwrap_or_default();
    let mut next = first.next;
    let mut followed = 0usize;
    while let Some(url) = next.take() {
        if followed >= MAX_RESULT_PAGES {
            return Collected { pages, credits, note: Some(format!("stopped after {MAX_RESULT_PAGES} result pages")) };
        }
        followed = followed.saturating_add(1);
        match client.crawl_page(&url) {
            Ok(page) => {
                pages.extend(page.data.unwrap_or_default());
                next = page.next;
            }
            Err(e) => {
                let of = total.map_or_else(String::new, |t| format!(" of {t}"));
                let note = format!("INCOMPLETE: {} pages{of} retrieved, then: {e}", pages.len());
                return Collected { pages, credits, note: Some(note) };
            }
        }
    }
    Collected { pages, credits, note: None }
}

/// Combined markdown for a crawl (`note` flags partial results).
pub(crate) fn build_markdown(url: &str, pages: &[CrawlPageData], credits: u32, note: Option<&str>) -> String {
    let mut md = String::new();
    writeln!(md, "# Crawl: {url}\n").unwrap_or(());
    writeln!(md, "> {} pages crawled, {credits} credits used\n", pages.len()).unwrap_or(());
    if let Some(n) = note {
        writeln!(md, "> WARNING: {n}\n").unwrap_or(());
    }
    md.push_str("---\n\n");
    for (i, page) in pages.iter().enumerate() {
        let title = page.metadata.as_ref().and_then(|m| m.title.as_deref()).unwrap_or("untitled");
        let page_url = page.metadata.as_ref().and_then(|m| m.source_url.as_deref()).unwrap_or("unknown");
        writeln!(md, "## Page {} — {} ({})\n", i.saturating_add(1), title, page_url).unwrap_or(());
        if let Some(content) = page.markdown.as_ref() {
            md.push_str(content);
            md.push_str("\n\n");
        }
        md.push_str("---\n\n");
    }
    md
}

/// Write crawl results to `output` and summarize.
fn write_output(url: &str, output: &Path, collected: &Collected) -> ToolOutput {
    let (pages, credits, note) = (&collected.pages, collected.credits, collected.note.as_deref());
    let count = pages.len();
    if count == 0 {
        return ToolOutput::ok(format!("Crawl of '{url}' completed but returned 0 pages ({credits} credits)."));
    }
    let md = build_markdown(url, pages, credits, note);
    if let Some(parent) = output.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return ToolOutput::error(format!(
            "Crawl completed ({count} pages) but creating '{}' failed: {e}",
            parent.display()
        ));
    }
    if let Err(e) = std::fs::write(output, &md) {
        return ToolOutput::error(format!("Crawl completed ({count} pages) but failed to write output: {e}"));
    }
    let warn = note.map_or_else(String::new, |n| format!(" WARNING: {n}."));
    ToolOutput::ok(format!(
        "Crawl of '{url}' completed: {count} pages, {credits} credits. Results written to '{}'.{warn}",
        output.display(),
    ))
}

/// Validate a resolved output path: no `..` component, and inside one of `roots`.
///
/// # Errors
/// Model-facing reason the path is refused.
pub(crate) fn check_output_path(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("Output path '{}' must not contain '..'", path.display()));
    }
    if roots.iter().any(|r| path.starts_with(r)) {
        Ok(())
    } else {
        Err(format!("Output path '{}' must be inside the project directory", path.display()))
    }
}

/// String array input field, if present.
fn string_list(tool: &ToolUse, key: &str) -> Option<Vec<String>> {
    tool.input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

/// Execute the `firecrawl_crawl` tool: recursively crawl a site.
///
/// Starts an async crawl job, polls until complete, then writes combined
/// markdown output to the specified path. No panel is created — the file
/// is the deliverable.
pub(crate) fn exec_crawl(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("firecrawl_crawl");
    let Some(url_ref) = tool.input.get("url").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'url'".to_owned());
    };
    let Some(output_ref) = tool.input.get("output").and_then(|v| v.as_str()) else {
        return err_result(tool, "Missing required parameter 'output'".to_owned());
    };
    let output = match state.try_resolve_path(output_ref) {
        Ok(p) => p,
        Err(e) => return err_result(tool, e),
    };
    let roots = [state.workdir(), cp_base::config::constants::project_root()];
    if let Err(e) = check_output_path(&output, &roots) {
        return err_result(tool, e);
    }
    let client = match get_client() {
        Ok(c) => c,
        Err(e) => return err_result(tool, e),
    };

    let job = CrawlJob {
        url: url_ref.to_owned(),
        output,
        limit: tool.input.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(10).clamp(1, 100).to_u32(),
        max_depth: tool.input.get("max_depth").and_then(serde_json::Value::as_u64).map(Safe::to_u32),
        include_paths: string_list(tool, "include_paths"),
        exclude_paths: string_list(tool, "exclude_paths"),
        allow_subdomains: tool.input.get("allow_subdomains").and_then(serde_json::Value::as_bool).unwrap_or(false),
    };
    spawn_async_tool(state, tool, ASYNC_TIMEOUT_CRAWL_SECS, move || run_crawl(client, &job, Timing::PRODUCTION))
}
