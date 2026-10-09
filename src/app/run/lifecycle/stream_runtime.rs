//! Per-thread *stream* runtime: the mutable bookkeeping a thread owns while
//! its LLM turn is in flight (typewriter, pending tool calls, the deferred
//! `StreamDone` payload, console-wait / blocking-watcher accumulators, the
//! deferred-sleep flags).
//!
//! It lives in the thread's own per-thread module data
//! ([`ThreadRuntime::thread_module_data`](cp_base::state::runtime::bundle::ThreadRuntime)),
//! so it follows the executing thread like the rest of its context: switching
//! threads moves nothing, and a thread's in-flight stream can never bleed into
//! another's. Reach it through [`App::stream_rt`] / [`App::stream_rt_mut`].

use crate::app::{App, PendingDone};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::ui::TypewriterBuffer;

/// The complete per-stream mutable runtime owned by one thread.
///
/// Stored per thread; see the module docs.
pub(crate) struct StreamRuntime {
    /// Streaming typewriter buffer (pending chars + speed estimation).
    pub typewriter: TypewriterBuffer,
    /// Deferred `StreamDone` payload awaiting typewriter drain.
    pub pending_done: Option<PendingDone>,
    /// Tool calls accumulated during streaming, awaiting execution.
    pub pending_tools: Vec<ToolUse>,
    /// Retryable error pending a stream re-launch on the next step.
    pub pending_retry_error: Option<String>,
    /// Timestamp (ms) when `wait_for_panels` started (for timeout).
    pub wait_started_ms: u64,
    /// Deferred tool-sleep deadline (ms): the tool pipeline resumes after this.
    pub deferred_tool_sleep_until_ms: u64,
    /// Whether this thread is in a deferred tool-sleep wait.
    pub deferred_tool_sleeping: bool,
    /// Tool results held while a console blocking wait is active.
    pub pending_console_wait_tool_results: Option<Vec<ToolResult>>,
    /// Partial blocking-watcher results, collected until all complete.
    pub accumulated_blocking_results: Vec<cp_base::state::watchers::carriers::WatcherResult>,
}

impl Default for StreamRuntime {
    fn default() -> Self {
        Self {
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            pending_retry_error: None,
            wait_started_ms: 0,
            deferred_tool_sleep_until_ms: 0,
            deferred_tool_sleeping: false,
            pending_console_wait_tool_results: None,
            accumulated_blocking_results: Vec::new(),
        }
    }
}

impl StreamRuntime {
    /// A fresh, empty per-stream runtime (every field at its idle default).
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// The executing thread's stream runtime, if it has been touched yet.
    pub(crate) fn stream_rt(&self) -> Option<&StreamRuntime> {
        self.state.get_ext::<StreamRuntime>()
    }

    /// The executing thread's stream runtime, created on first use.
    pub(crate) fn stream_rt_mut(&mut self) -> &mut StreamRuntime {
        if self.state.get_ext::<StreamRuntime>().is_none() {
            self.state.set_ext_thread(StreamRuntime::new());
        }
        self.state.ext_mut::<StreamRuntime>()
    }
}
