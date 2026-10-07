//! Per-thread *stream* runtime — the mutable per-stream bookkeeping a single
//! thread owns while its LLM turn is in flight.
//!
//! [`ThreadRuntime`](cp_base::state::runtime::bundle::ThreadRuntime) swaps a
//! thread's **`State`** (conversation, panels, token/cost telemetry) in and out
//! of the resident slot. But a streaming turn also carries mutable state that
//! lives on [`App`] *outside* `State` — the typewriter buffer, the pending
//! tool-call queue, the deferred `StreamDone` payload, the console-wait / blocking
//! watcher accumulators, and the deferred-sleep flags. Those were single
//! top-level `App` fields, so when [`advance_background_threads`] swapped a
//! background thread's `State` in and ran the shared advancement core, the
//! focused thread's *residual* typewriter characters / pending tools /
//! `pending_done` bled into the background thread's context (and vice-versa):
//! the N>1 "completely buggy" corruption.
//!
//! [`StreamRuntime`] is the carrier that fixes it — the per-stream analogue of
//! `ThreadRuntime`. One stash exists per non-resident thread (parked in
//! [`App::parked_stream_runtimes`]); [`StreamRuntime::swap_with_app`] exchanges
//! all nine fields with `App` in O(1) via [`std::mem::swap`], exactly mirroring
//! the `ThreadRuntime` swap-model. The focused thread's stream runtime always
//! lives in the flat `App` fields (the resident slot); every background thread's
//! parks here and is swapped in only for the duration of its advancement step.
//!
//! At N=1 the map is empty and nothing is ever swapped — behaviour is identical
//! to single-thread.

use crate::app::{App, PendingDone};
use crate::infra::tools::{ToolResult, ToolUse};
use crate::ui::TypewriterBuffer;

/// The complete per-stream mutable runtime owned by one thread.
///
/// Every field mirrors a per-stream field of [`App`] (same name, same type);
/// [`swap_with_app`](Self::swap_with_app) exchanges them all. The resident
/// thread's values live on `App`; every non-resident thread parks its values
/// here in [`App::parked_stream_runtimes`].
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

    /// Exchange every per-stream field with `app` in O(1).
    ///
    /// Symmetric, like [`ThreadRuntime::swap_with`](cp_base::state::runtime::bundle::ThreadRuntime::swap_with):
    /// calling it once *loads* `self` into `app` (parking `app`'s previous
    /// resident per-stream state back into `self`); calling it again restores the
    /// original arrangement. The advancement loop uses it to make a background
    /// thread's stream runtime resident for one step, then swaps the focused
    /// thread's back.
    pub(crate) const fn swap_with_app(&mut self, app: &mut App) {
        use std::mem::swap;
        swap(&mut self.typewriter, &mut app.typewriter);
        swap(&mut self.pending_done, &mut app.pending_done);
        swap(&mut self.pending_tools, &mut app.pending_tools);
        swap(&mut self.pending_retry_error, &mut app.pending_retry_error);
        swap(&mut self.wait_started_ms, &mut app.wait_started_ms);
        swap(&mut self.deferred_tool_sleep_until_ms, &mut app.deferred_tool_sleep_until_ms);
        swap(&mut self.deferred_tool_sleeping, &mut app.deferred_tool_sleeping);
        swap(&mut self.pending_console_wait_tool_results, &mut app.pending_console_wait_tool_results);
        swap(&mut self.accumulated_blocking_results, &mut app.accumulated_blocking_results);
    }
}
