//! **Authentication & authorization service** — orchestrator-level user
//! management, session handling, and per-agent access control (design doc
//! `docs/design-auth.md`).
//!
//! The auth subsystem is backed by a dedicated SQLite database stored at
//! `~/.context-pilot/orchestrator/auth.db` (configurable via `CP_AUTH_DB`),
//! separate from agent data. Three tables: `users`, `sessions`, `agent_acl`.
//!
//! # Layout
//!
//! * [`types`] — Domain types: [`AuthError`], [`UserRole`], [`AgentRole`],
//!   [`User`], [`Session`].
//! * [`store`] — [`AuthStore`] struct: schema init, password hashing,
//!   token generation, and all CRUD operations.

pub mod types;
pub mod store;
pub(crate) mod backup;
mod helpers;
