//! **Finder** endpoints — the per-agent file manager confined to an agent's
//! working directory (realm). All paths are jailed to the realm: any attempt to
//! escape via `..`, a symlink, or an absolute path is rejected.
//!
//! Split across cohesive sub-modules (kept under the 500-line file budget, and
//! room for the preview-format endpoints that keep landing here):
//!
//! * [`listing`] — read views: `fs_list`, `fs_preview`, `conversation`.
//! * [`mutate`] — writes: `fs_upload`, `fs_mkdir`, `fs_rename`, `fs_move`,
//!   `fs_trash`.
//! * [`download`] — raw file / zipped-folder download.
//! * [`support`] — shared helpers (path confinement, kind inference, query
//!   parsing) + the unit tests.
//!
//! Every handler is re-exported here so call sites keep the stable
//! `inspect::finder::fs_*` path regardless of which sub-module owns the fn.

mod download;
mod listing;
mod mutate;
pub(super) mod support;

pub use download::fs_download;
pub use listing::{conversation, fs_list, fs_preview};
pub use mutate::{fs_mkdir, fs_move, fs_rename, fs_trash, fs_upload};
