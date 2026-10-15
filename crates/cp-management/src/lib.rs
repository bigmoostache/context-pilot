//! Management — agents creating and managing their sub-agents (T860 phase E).
//!
//! One tool, `Agent(operation=create|read|update|delete, ...)`. A manager may
//! create and rename its direct children and delete any descendant; each
//! child gets a 1:1 coordination group with its manager. Agent creation and
//! hard deletion need the main loop (bridge announce, runtime teardown), so
//! the tool queues them in [`types::ManagementState`] for the app to drain.

/// Delete confirmation keys bound to one `(caller, target)` (5-minute validity).
pub mod keys;
/// `Agent` tool operations.
pub mod ops;
/// `Agent` tool rule tests.
#[cfg(test)]
mod tests;
/// `ManagementState`: depth cap + outboxes drained by the main loop.
pub mod types;
/// `ManagementModule`: tool registration + config persistence.
pub mod wiring;
