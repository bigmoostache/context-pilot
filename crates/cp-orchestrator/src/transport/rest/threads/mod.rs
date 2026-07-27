//! Thread-facing REST helpers, grouped so `rest/` stays within the 8-entry
//! directory cap.
//!
//! - [`thread_shape`] — reshapes a raw agent thread record into the web
//!   frontend's expected shape, overlaying the live oplog roster.
//! - [`conversations`] — the read-path conversation-search handler (T671),
//!   which forwards a query to the agent over the bridge and returns its hits.

pub(super) mod conversations;
pub(super) mod thread_shape;

pub(crate) use thread_shape::{overlay_roster, reshape_thread};
