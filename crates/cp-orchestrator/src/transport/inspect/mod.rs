//! Inspection endpoints — the backend's read-only JSON views of an agent.
//!
//! These handlers shape the agent's projected state and on-disk tier-② files
//! into the maquette JSON the cockpit consumes. They are grouped under one
//! module because they share a role (read → reshape → respond, never mutate)
//! and a structural budget (the parent `transport` directory's 8-entry limit).
//!
//! * [`meta`] — enriched `Agent` objects (registry + view + git + threads).
//! * [`panels`] — cockpit inspection panels (memory, todos, tree, …).
//! * [`finder`] — the per-agent file manager (`/fs`, preview, download).
//! * [`metrics`] — the §19 observability snapshot (breaker, stream, rev lag).
//! * [`vitals`] — on-demand service-connectivity probes (`/vitals`).
//!
//! Each submodule reaches the shared [`Backend`](crate::transport::Backend) and
//! [`HttpReply`](crate::transport::rest::HttpReply) via absolute `crate::`
//! paths, so nesting them here required no change to their handler bodies.

pub mod finder;
mod helpers;
pub mod meta;
pub mod metrics;
pub mod panels;
pub mod providers;
pub mod vitals;
