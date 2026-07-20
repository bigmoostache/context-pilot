/// Action and result types for the event-driven dispatch loop.
pub mod actions;
/// File-path autocomplete state for @-triggered popup.
pub mod autocomplete;
/// Context types, elements, and token estimation.
pub mod context;
/// Serializable data structures: config, messages, persistence types.
pub mod data;
/// Stream-phase state machine, boolean flag structs, and streaming-tool advisory state.
pub mod flags;

/// Runtime state: the in-memory `State` struct with all live fields.
pub mod runtime;
/// Watcher trait and registry for async condition monitoring.
pub mod watchers;
