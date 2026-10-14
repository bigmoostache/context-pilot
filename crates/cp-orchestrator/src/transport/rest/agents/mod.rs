//! Thread-facing REST helpers, grouped so `rest/` stays within the 8-entry
//! directory cap.
//!
//! - [`agent_shape`] — reshapes a raw agent agent record into the web
//!   frontend's expected shape, overlaying the live oplog roster.
//! - [`conversations`] — the read-path conversation-search handler (T671),
//!   which forwards a query to the agent over the bridge and returns its hits.
//! - [`tree_order`] — orders the list as the sidebar tree (creation order,
//!   sub-agents in preorder under their parent, `depth` per row).

pub(super) mod agent_shape;
pub(super) mod conversations;
mod tree_order;

pub(crate) use agent_shape::{attach_disk_notes, attach_disk_tasks, overlay_roster, reshape_agent, same_agent};
pub(crate) use tree_order::order_tree;
