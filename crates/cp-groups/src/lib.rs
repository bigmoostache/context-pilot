//! Groups — conversations shared by agents and the human (T860 phase C).
//!
//! Every agent owns one [`GroupKind::Human`](types::GroupKind::Human) group
//! holding its conversation with the user; coordination and custom groups
//! connect agents with each other. Each group has exactly one owner agent.

/// Group message types (formerly the agents' own message types).
pub mod chat;
/// `GroupsModule`: state registration + metadata persistence.
pub mod wiring;
/// Per-group message files (`groups/<id>.json`), written only when dirty.
pub mod persist;
/// `Group`, `GroupKind`, `GroupsState`.
pub mod types;
