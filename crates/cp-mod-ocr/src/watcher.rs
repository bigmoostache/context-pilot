//! Non-blocking watcher delivering a background OCR job's outcome as a
//! spine notification to the agent that started it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::Watcher;
use cp_base::state::watchers::carriers::WatcherResult;

/// Monotonic counter for unique OCR watcher IDs.
static OCR_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Polls the worker's channel; fires once with the worker's message, a
/// "worker died" error, or a timeout.
pub(crate) struct OcrWatcher {
    /// Unique watcher ID.
    watcher_id: String,
    /// Human-readable description for the Spine panel.
    desc: String,
    /// Receiver end of the channel from the worker thread.
    rx: Mutex<Receiver<String>>,
    /// Agent that called `ocr`: the notification goes to it, not to focus.
    owner: Option<String>,
    /// Timestamp when this watcher was registered.
    registered_at_ms: u64,
    /// Absolute deadline in ms since epoch.
    deadline_ms: u64,
}

impl OcrWatcher {
    /// Watch `rx` for at most `timeout_ms`, reporting to `owner`.
    pub(crate) fn new(path_display: &str, rx: Receiver<String>, owner: Option<String>, timeout_ms: u64) -> Self {
        let now = now_ms();
        let counter = OCR_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            watcher_id: format!("ocr_{counter}"),
            desc: format!("\u{23f3} OCR: {path_display}"),
            rx: Mutex::new(rx),
            owner,
            registered_at_ms: now,
            deadline_ms: now.saturating_add(timeout_ms),
        }
    }

    /// Wrap a message, bound to the owning agent with a per-watcher source
    /// (distinct sources keep concurrent jobs from being deduplicated).
    fn result(&self, message: String) -> WatcherResult {
        let r = WatcherResult::new(message).source(format!("ocr:{}", self.watcher_id));
        if let Some(id) = self.owner.as_deref() { r.agent_id(id) } else { r }
    }
}

impl std::fmt::Debug for OcrWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrWatcher")
            .field("watcher_id", &self.watcher_id)
            .field("desc", &self.desc)
            .field("owner", &self.owner)
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
        false
    }

    fn tool_use_id(&self) -> Option<&str> {
        None // No sentinel to replace
    }

    fn check(&self, _state: &State) -> Option<WatcherResult> {
        let Ok(rx) = self.rx.lock() else {
            return Some(self.result("\u{274c} OCR watcher failed (lock poisoned)".to_owned()));
        };
        match rx.try_recv() {
            Ok(message) => Some(self.result(message)),
            Err(TryRecvError::Disconnected) => {
                Some(self.result("\u{274c} OCR worker thread died unexpectedly".to_owned()))
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        (now_ms() >= self.deadline_ms).then(|| {
            let secs = cp_base::panels::time_arith::ms_to_secs(self.deadline_ms.saturating_sub(self.registered_at_ms));
            self.result(format!(
                "\u{274c} OCR worker unresponsive after {secs}s; the output file may still appear later"
            ))
        })
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "ocr"
    }

    fn agent_id(&self) -> Option<&str> {
        self.owner.as_deref()
    }
}
