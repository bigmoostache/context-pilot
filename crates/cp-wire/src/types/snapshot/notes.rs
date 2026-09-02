//! Thread-owned scratchpad-note projection type — the read-only shape the web
//! thread view renders in its Notes aside.
//!
//! Defined here in the I/O-free protocol crate rather than imported from
//! `cp-mod-scratchpad` to keep the layering one-directional (modules depend on
//! the wire, never the reverse), exactly like [`WireTask`](super::todo::WireTask).
//! Carried on the owning thread's
//! [`RosterThread::notes`](super::RosterThread::notes) and replaced wholesale by
//! each [`NotesChanged`](super::super::oplog::OpEntryKind::NotesChanged) delta
//! (whole-list snapshot semantics — the twin of `TaskListChanged`).
//!
//! The module is named `notes` (plural) rather than `note`: the projection type
//! must keep the crate's mandatory `Wire*` prefix (`WireNote`, twin of
//! `WireTask`), and a singular `note` module would make the type name repeat its
//! module name — tripping the workspace-`forbid` `clippy::module_name_repetitions`
//! (which an `expect` attribute cannot override). The plural module name sidesteps
//! it with no suppression.

use serde::{Deserialize, Serialize};

/// One thread-owned scratchpad cell as projected onto the wire — the read-only
/// shape the web thread view renders in its Notes aside.
///
/// A flat list carried on the owning thread's
/// [`RosterThread::notes`](super::RosterThread::notes); the frontend renders it
/// as a list that expands on click to show [`content`](Self::content) (the Files
/// tab interaction pattern).
///
/// A three-field wire value object built by struct literal at its cross-crate
/// call sites (the agent's note projection and the orchestrator's first-paint
/// read), so it is deliberately exhaustive: `#[non_exhaustive]` would forbid the
/// very literal construction those sites need. A new field here is a deliberate
/// wire change every peer recompiles for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "wire value object: all three fields are public, stable, and built by cross-crate consumers via a struct literal, so #[non_exhaustive] would forbid that construction"
)]
pub struct WireNote {
    /// Scratchpad cell identifier (e.g. `"C12"`).
    pub id: String,

    /// Short cell title (the list-row label).
    pub title: String,

    /// The cell's full content (revealed when the row is expanded).
    pub content: String,
}
