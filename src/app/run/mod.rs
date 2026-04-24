/// Constructor, state persistence helpers, autocomplete / question-form / palette input handlers.
mod input;
/// Main event loop (`App::run`) and spine check / auto-continuation.
pub(crate) mod lifecycle;
/// Reverie (context-optimizer sub-agent) stream lifecycle and tool dispatch.
mod reverie;
/// Stream-event processing, retry logic, typewriter buffer, and stream finalization.
mod streaming;
/// Tool execution: pipeline, post-execution checks, and watcher-sentinel cleanup.
mod tools;
/// File/GH watcher setup, cache updates, timer-based deprecation, and watcher-event dispatch.
mod watchers;
