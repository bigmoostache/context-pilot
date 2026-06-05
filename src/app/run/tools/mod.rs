/// Post-tool-execution checks: panel readiness, deferred sleeps, and question forms.
pub(crate) mod checks;
/// Watcher-sentinel replacement, blocking-result accumulation, and queue-flush execution.
pub(crate) mod cleanup;
/// Global tool metadata middleware — validates `intent`/`verb` on every tool call.
pub(crate) mod metadata;
/// Tool execution pipeline: tool-call messages, pre-flight checks, queue intercept, callbacks.
pub(crate) mod pipeline;
/// Queue flush execution — dequeues and runs all queued tool calls.
pub(crate) mod queue_flush;
