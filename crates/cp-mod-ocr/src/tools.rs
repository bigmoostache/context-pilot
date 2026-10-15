//! OCR tool execution.
//!
//! Dispatches the `ocr` tool call, validates parameters, and spawns
//! a background worker thread that calls the Datalab API. The tool
//! returns immediately — a spine notification fires when the
//! conversion is done (or fails).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;
use cp_base::tools::{ToolResult, ToolUse};

use crate::cache::atomic_write;
use crate::client::{DatalabClient, OcrMode, WATCHER_TIMEOUT, api_key_from_env, is_ocr_extension};
use crate::watcher::OcrWatcher;

/// Maximum OCR jobs running at once; further calls are refused.
pub(crate) const MAX_CONCURRENT_JOBS: usize = 4;

/// Jobs currently running (held by [`JobSlot`]).
static RUNNING_JOBS: AtomicUsize = AtomicUsize::new(0);

/// One reserved slot of [`MAX_CONCURRENT_JOBS`], released on drop (also when
/// the worker panics).
#[derive(Debug)]
pub(crate) struct JobSlot;

impl JobSlot {
    /// Reserve a slot, or `None` when all are taken.
    pub(crate) fn acquire() -> Option<Self> {
        RUNNING_JOBS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_CONCURRENT_JOBS).then_some(n.saturating_add(1))
            })
            .ok()
            .map(|_prev| Self)
    }
}

impl Drop for JobSlot {
    fn drop(&mut self) {
        let _prev = RUNNING_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Dispatch OCR tool calls.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    (tool.name == "ocr").then(|| execute_ocr(tool, state))
}

/// Validated inputs for one OCR job, produced by [`validate_ocr_inputs`].
struct OcrJob {
    /// Datalab API key from the vault.
    api_key: String,
    /// Source document, resolved against the agent's workdir.
    path: PathBuf,
    /// Output format (markdown or text-boxes).
    mode: OcrMode,
    /// Destination, resolved against the agent's workdir.
    output_path: PathBuf,
    /// Source path as given (for messages).
    path_display: String,
    /// Output path as given (for messages).
    output_display: String,
}

/// Validate every `ocr` parameter synchronously (API key, path existence +
/// extension, mode, output + parent dir). Relative paths resolve against the
/// executing agent's workdir (its worktree, else the project root).
fn validate_ocr_inputs(tool: &ToolUse, state: &State) -> Result<OcrJob, String> {
    if let Some(e) = state.workdir_error() {
        return Err(e);
    }
    let api_key = api_key_from_env().ok_or_else(|| {
        "Datalab API key not found: set DATALAB_API_KEY in the environment or ~/.context-pilot/.env, \
         or store 'datalab' in the credential vault."
            .to_owned()
    })?;

    let path_str = tool.input.get("path").and_then(|v| v.as_str()).ok_or("Missing required parameter 'path'.")?;
    let path = state.resolve_path(path_str);
    if !path.is_file() {
        return Err(format!("File not found: '{path_str}'"));
    }
    let ext = path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if !is_ocr_extension(ext) {
        return Err(format!(
            "Unsupported file type '.{ext}'. Supported: pdf, png, jpg, jpeg, tiff, webp, bmp, heic, gif."
        ));
    }

    let mode_str = tool
        .input
        .get("mode")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter 'mode'. Use 'markdown' or 'text_boxes'.")?;
    let mode = OcrMode::from_str(mode_str)?;

    let output_str = tool.input.get("output").and_then(|v| v.as_str()).ok_or("Missing required parameter 'output'.")?;
    let output_path = state.resolve_path(output_str);
    if let Some(parent) = output_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create output directory '{}': {e}", parent.display()))?;
    }

    Ok(OcrJob {
        api_key,
        path,
        mode,
        output_path,
        path_display: path_str.to_owned(),
        output_display: output_str.to_owned(),
    })
}

/// Run one job on the worker thread and phrase its outcome.
fn run_job(client: &DatalabClient, job: &OcrJob) -> String {
    match client.convert(&job.path, job.mode) {
        Ok(result) => {
            let out = &job.output_display;
            if let Err(e) = atomic_write(&job.output_path, result.text.as_bytes()) {
                return format!("\u{274c} OCR succeeded but failed to write output to '{out}': {e}");
            }
            let cached_note = if result.cached { " (from cache)" } else { "" };
            let chars = result.text.chars().count();
            format!("\u{2705} OCR complete{cached_note}! {chars} chars written to '{out}'.")
        }
        Err(e) => format!("\u{274c} OCR failed for '{}': {e}", job.path_display),
    }
}

/// Execute the `ocr` tool: validate synchronously, spawn the worker, register
/// a non-blocking watcher bound to the calling agent, return immediately.
fn execute_ocr(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("ocr_exec");
    let job = match validate_ocr_inputs(tool, state) {
        Ok(j) => j,
        Err(e) => return err(tool, e),
    };
    let Some(slot) = JobSlot::acquire() else {
        return err(
            tool,
            format!("{MAX_CONCURRENT_JOBS} OCR jobs already running. Wait for a completion notification and retry."),
        );
    };
    let content = format!(
        "OCR job submitted for '{}' \u{2192} '{}'. A notification will arrive when the conversion finishes. \
         If you go idle, the notification will wake you up.",
        job.path_display, job.output_display
    );
    let path_display = job.path_display.clone();

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::Builder::new().name("ocr-worker".into()).spawn(move || {
        let _slot = slot;
        let message = match DatalabClient::new(&job.api_key) {
            Ok(client) => run_job(&client, &job),
            Err(e) => format!("\u{274c} OCR failed: cannot create client: {e}"),
        };
        let _r = tx.send(message);
    });
    if let Err(e) = handle {
        return err(tool, format!("Failed to spawn OCR worker thread: {e}"));
    }

    let owner = state.executing_agent_id().map(str::to_owned);
    let timeout_ms = u64::try_from(WATCHER_TIMEOUT.as_millis()).unwrap_or(u64::MAX);
    let watcher = OcrWatcher::new(&path_display, rx, owner, timeout_ms);
    WatcherRegistry::get_mut(state).register(Box::new(watcher));

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
fn err(tool: &ToolUse, content: String) -> ToolResult {
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
