//! OCR tool execution.
//!
//! Dispatches the `ocr` tool call, validates parameters, and spawns
//! a background worker thread that calls the Datalab API. The tool
//! returns immediately — a spine notification fires when the
//! conversion is done (or fails).

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::carriers::WatcherResult;
use cp_base::state::watchers::{Watcher, WatcherRegistry};
use cp_base::tools::{ToolResult, ToolUse};

use crate::client::{DatalabClient, OcrMode, api_key_from_env, is_ocr_extension};

/// Async timeout for OCR API calls (seconds).
///
/// Datalab can take several minutes for large documents. The client
/// has its own 300 s poll timeout; this covers submit + poll + margin.
const ASYNC_TIMEOUT_SECS: u64 = 360;

/// Monotonic counter for generating unique OCR watcher IDs.
static OCR_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Dispatch OCR tool calls.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    (tool.name == "ocr").then(|| execute_ocr(tool, state))
}

/// Execute the `ocr` tool.
///
/// Validates parameters synchronously, spawns a background thread for the
/// Datalab API call, and returns immediately. A spine notification fires
/// when the conversion completes (or fails), waking the AI if idle.
fn execute_ocr(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("ocr_exec");
    // --- Validate DATALAB_API_KEY ---
    let Some(api_key) = api_key_from_env() else {
        return err(tool, "DATALAB_API_KEY not set in environment.".to_owned());
    };

    // --- Validate path ---
    let Some(path_str) = tool.input.get("path").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'path'.".to_owned());
    };
    let path = PathBuf::from(path_str);
    if !path.exists() {
        return err(tool, format!("File not found: '{path_str}'"));
    }

    let ext = path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if !is_ocr_extension(ext) {
        return err(
            tool,
            format!("Unsupported file type '.{ext}'. Supported: pdf, png, jpg, jpeg, tiff, webp, bmp, heic, gif."),
        );
    }

    // --- Validate mode ---
    let Some(mode_str) = tool.input.get("mode").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'mode'. Use 'markdown' or 'text_boxes'.".to_owned());
    };
    let mode = match OcrMode::from_str(mode_str) {
        Ok(m) => m,
        Err(e) => return err(tool, e),
    };

    // --- Validate output ---
    let Some(output_str) = tool.input.get("output").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'output'.".to_owned());
    };
    let output_path = PathBuf::from(output_str);

    // Ensure parent directory exists.
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return err(tool, format!("Cannot create output directory '{}': {e}", parent.display()));
    }

    // --- Spawn background thread + non-blocking watcher ---
    let path_display = path_str.to_owned();
    let output_display = output_str.to_owned();
    let (tx, rx) = mpsc::channel();

    // Clone for the closure — originals are used in the tool result after spawn.
    let path_for_closure = path_display.clone();
    let output_for_closure = output_display.clone();

    let handle = std::thread::Builder::new().name("ocr-worker".into()).spawn(move || {
        let client = match DatalabClient::new(&api_key) {
            Ok(c) => c,
            Err(e) => {
                let _r = tx.send(format!("❌ OCR failed: cannot create client: {e}"));
                return;
            }
        };

        match client.convert(&path, mode) {
            Ok(result) => {
                if let Err(e) = std::fs::write(&output_path, &result.text) {
                    let _r =
                        tx.send(format!("❌ OCR succeeded but failed to write output to '{output_for_closure}': {e}"));
                    return;
                }
                let cached_note = if result.cached { " (from cache)" } else { "" };
                let char_count = result.text.len();
                let _r = tx.send(format!(
                    "✅ OCR complete{cached_note}! {char_count} chars written to '{output_for_closure}'.",
                ));
            }
            Err(e) => {
                let _r = tx.send(format!("❌ OCR failed for '{path_for_closure}': {e}"));
            }
        }
    });

    if let Err(e) = &(handle) {
        return err(tool, format!("Failed to spawn OCR worker thread: {e}"));
    }

    // Register non-blocking watcher — fires as a spine notification
    let counter = OCR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let watcher = OcrWatcher::new(counter, &path_display, rx, ASYNC_TIMEOUT_SECS);
    WatcherRegistry::get_mut(state).register(Box::new(watcher));

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: format!(
            "OCR job submitted for '{path_display}' → '{output_display}'. \
             A notification will arrive when the conversion finishes. \
             If you go idle, the notification will wake you up.",
        ),
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

// ============================================================
// OcrWatcher — non-blocking watcher for background OCR jobs
// ============================================================

/// A non-blocking watcher that polls an [`mpsc::Receiver<String>`] for an
/// OCR completion message from the background worker thread.
///
/// When the message arrives (or the thread dies / times out), the watcher
/// fires a **spine notification** instead of replacing a blocking sentinel.
/// This lets the AI continue working while OCR runs in the background.
struct OcrWatcher {
    /// Unique watcher ID.
    watcher_id: String,
    /// Human-readable description for the Spine panel.
    desc: String,
    /// Receiver end of the channel from the worker thread.
    rx: Mutex<Receiver<String>>,
    /// Timestamp when this watcher was registered.
    registered_at_ms: u64,
    /// Absolute deadline in ms since epoch.
    deadline_ms: u64,
}

impl OcrWatcher {
    /// Create a new OCR watcher.
    fn new(counter: usize, path_display: &str, rx: Receiver<String>, timeout_secs: u64) -> Self {
        let now = now_ms();
        Self {
            watcher_id: format!("ocr_{counter}"),
            desc: format!("⏳ OCR: {path_display}"),
            rx: Mutex::new(rx),
            registered_at_ms: now,
            deadline_ms: now.saturating_add(timeout_secs.saturating_mul(1000)),
        }
    }
}

impl std::fmt::Debug for OcrWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrWatcher")
            .field("watcher_id", &self.watcher_id)
            .field("desc", &self.desc)
            .field("registered_at_ms", &self.registered_at_ms)
            .field("deadline_ms", &self.deadline_ms)
            .finish_non_exhaustive()
    }
}

impl Watcher for OcrWatcher {
    fn id(&self) -> &str {
        &self.watcher_id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        false // Non-blocking — fires as a spine notification
    }

    fn tool_use_id(&self) -> Option<&str> {
        None // No sentinel to replace
    }

    fn check(&self, _state: &State) -> Option<WatcherResult> {
        let Ok(rx) = self.rx.lock() else {
            return Some(WatcherResult::new("❌ OCR watcher failed (lock poisoned)"));
        };
        match rx.try_recv() {
            Ok(message) => Some(WatcherResult::new(message)),
            Err(TryRecvError::Disconnected) => Some(WatcherResult::new("❌ OCR worker thread died unexpectedly")),
            Err(TryRecvError::Empty) => None,
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        (now_ms() >= self.deadline_ms).then(|| {
            let elapsed_secs =
                cp_base::panels::time_arith::ms_to_secs(self.deadline_ms.saturating_sub(self.registered_at_ms));
            WatcherResult::new(format!("❌ OCR timed out after {elapsed_secs}s"))
        })
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "ocr"
    }

    fn suicide(&self, _state: &State) -> bool {
        false
    }

    fn is_easy_bash(&self) -> bool {
        false
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn fire_at_ms(&self) -> Option<u64> {
        None
    }

    fn message(&self) -> Option<&str> {
        None
    }

    fn thread_id(&self) -> Option<&str> {
        None
    }

    fn interval_ms(&self) -> u64 {
        0
    }

    fn recurrence_label(&self) -> Option<&str> {
        None
    }
}
