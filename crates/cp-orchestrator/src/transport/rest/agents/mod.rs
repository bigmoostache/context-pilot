//! Thread-facing REST helpers, grouped so `rest/` stays within the 8-entry
//! directory cap.
//!
//! - [`agent_shape`] — reshapes a raw agent agent record into the web
//!   frontend's expected shape, overlaying the live oplog roster.
//! - [`conversations`] — the read-path conversation-search handler (T671),
//!   which forwards a query to the agent over the bridge and returns its hits.

pub(super) mod agent_shape;
pub(super) mod conversations;

pub(crate) use agent_shape::{attach_disk_notes, attach_disk_tasks, overlay_roster, reshape_agent};
