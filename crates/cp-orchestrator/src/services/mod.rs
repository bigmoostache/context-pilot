//! Backend **services** — the runtime layer built on the discovery, channel,
//! and supervisor primitives.
//!
//! Each service is a pure, single-owner data structure driven by the
//! orchestrator loop; none owns I/O threads of its own (the socket reads and
//! the WebSocket writer that feed them arrive with the transport phase):
//!
//! * [`materialized_view`] — [`MaterializedView`], the in-memory fleet-state
//!   projection folded from each agent's oplog (count-bounded restart, I5).
//! * [`cost_breaker`] — [`CostBreaker`], the durable per-agent spend breaker
//!   whose trip survives a crash-loop and which fails closed (R2-8 / V9).
//! * [`stream_hub`] — [`StreamHub`], the per-agent stream fan-out to N bounded
//!   subscribers with overflow-drop, degraded marking, and snapshot reconcile
//!   (R2-17).

pub mod avatars;
pub mod auth;
pub mod cost_breaker;
pub mod materialized_view;
pub mod names;
pub mod retire;
pub mod stream_hub;

pub use avatars::AvatarStore;
pub use cost_breaker::{CostBreaker, Verdict};
pub use materialized_view::{AgentView, CostSnapshot, MaterializedView};
pub use names::NameOverrides;
pub use retire::{RetiredRecord, RetiredStore};
pub use stream_hub::{StreamHub, Subscriber};
