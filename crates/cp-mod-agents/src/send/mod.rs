//! Send-time checks run before a message is posted into a group.

/// Validation of agent-authored ` ```form ` blocks.
pub(crate) mod forms;
/// Staleness guard: refuse posting into a group not seen up to date.
pub(crate) mod freshness;
