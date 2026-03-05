/// Constructor, state persistence helpers, autocomplete / question-form / palette input handlers.
mod input;
/// Main event loop (`App::run`) and spine check / auto-continuation.
pub(crate) mod lifecycle;
/// Reverie (context-optimizer sub-agent) stream lifecycle and tool dispatch.
mod reverie;
/// Stream-event processing, retry logic, typewriter buffer, and stream finalization.
mod streaming;
/// Watcher-sentinel replacement, blocking-result accumulation, and queue-flush execution.
mod tool_cleanup;
/// Tool execution pipeline: tool-call messages, pre-flight checks, queue intercept, callbacks.
mod tool_pipeline;
/// File/GH watcher setup, cache updates, timer-based deprecation, and watcher-event dispatch.
mod watchers;
