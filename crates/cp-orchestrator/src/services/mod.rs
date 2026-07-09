//! Backend **services** — the runtime layer built on the discovery, channel,
//! and supervisor primitives.
//!
//! Each service is a pure, single-owner data structure driven by the
//! orchestrator loop; none owns I/O threads of its own (the socket reads and
//! the WebSocket writer that feed them arrive with the transport phase):
//!
//! * [`materialized_view`] — [`MaterializedView`], the in-memory fleet-state
//!   projection folded from each agent's oplog (count-bounded restart, I5).
//! * [`stream_hub`] — [`StreamHub`], the per-agent stream fan-out to N bounded
//!   subscribers with overflow-drop, degraded marking, and snapshot reconcile
//!   (R2-17).

pub mod agent_meta;
pub mod auth;
pub mod materialized_view;
pub mod releases;
pub mod retire;
pub mod stream_hub;

pub use agent_meta::{AvatarStore, NameOverrides};
pub use materialized_view::{AgentView, CostSnapshot, MaterializedView};
pub use releases::ReleaseStore;
pub use retire::{RetiredRecord, RetiredStore};
pub use stream_hub::{StreamHub, Subscriber};
