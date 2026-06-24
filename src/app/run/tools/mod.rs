/// Post-tool-execution checks: panel readiness, deferred sleeps, and question forms.
pub(crate) mod checks;
/// Watcher-sentinel replacement, blocking-result accumulation, and queue-flush execution.
pub(crate) mod cleanup;
/// Tool execution pipeline: tool-call messages, pre-flight checks, queue intercept, callbacks.
pub(crate) mod pipeline;
/// Queue flush execution — dequeues and runs all queued tool calls.
pub(crate) mod queue_flush;
/// Interactive main-loop wedge watchdog (heartbeat + activity marker + diagnostic dump).
pub(crate) mod watchdog;
