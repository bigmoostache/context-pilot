//! Agent **registration** — the boot-time identity it mints and the discovery
//! record it publishes.
//!
//! Booting an agent has two registration concerns, grouped here:
//!
//! * [`identity`] mints the agent's stable folder id, its per-boot id, and the
//!   bearer `cap_token` (the secrets and naming keys, design doc §10 / I9); and
//! * [`registry`] writes those — plus the agent's resource paths — atomically
//!   and `0600` to `~/.context-pilot/agents/<id>.json`, the single artifact the
//!   backend watches to discover the agent.
//!
//! [`crate::boot`] orchestrates both: it mints identity, acquires resources,
//! then publishes the registry record last.

pub mod identity;
pub mod registry;
