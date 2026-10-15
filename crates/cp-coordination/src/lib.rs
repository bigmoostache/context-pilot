//! Coordination — agents creating and managing groups (T860 phase F).
//!
//! Four tools on custom groups: `Group_create`, `Read`,
//! `Group_update` (owner only, membership guarded by the exact current
//! list) and `Group_delete` (owner only, never a human group).

/// Side effects on other agents: notices, panel cleanup, scoped clears.
pub mod effects;
/// Group tool operations.
pub mod ops;
/// State-level tests of the group tool rules.
#[cfg(test)]
mod tests;
/// `CoordinationModule`: tool registration.
pub mod wiring;
