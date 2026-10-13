//! Coordination — agents creating and managing groups (T860 phase F).
//!
//! Four tools on custom groups: `Group_create`, `Group_open`,
//! `Group_update` (owner only, membership guarded by the exact current
//! list) and `Group_delete` (owner only, never a human group).

/// Group tool operations.
pub mod ops;
/// `CoordinationModule`: tool registration.
pub mod wiring;
