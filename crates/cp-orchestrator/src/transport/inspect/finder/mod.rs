//! **Finder** endpoints — the per-agent file manager confined to an agent's
//! working directory (realm). All paths are jailed to the realm: any attempt to
//! escape via `..`, a symlink, or an absolute path is rejected.
//!
//! Split across cohesive sub-modules (kept under the 500-line file budget, and
//! room for the preview-format endpoints that keep landing here):
//!
//! * [`listing`] — read views: `fs_list`, `fs_preview`, `fs_descriptions`,
//!   `conversation`.
//! * [`sheet`] — spreadsheet → table JSON (`fs_sheet`, CSV/TSV/xlsx/xls/ods).
//! * [`mutate`] — writes: `fs_upload`, `fs_write`, `fs_mkdir`, `fs_rename`,
//!   `fs_move`, `fs_trash`.
//! * [`download`] — raw file / zipped-folder download (`fs_download`) and the
//!   inline raw-serve (`fs_raw`, image/PDF previews).
//! * [`support`] — shared helpers (path confinement, kind inference, query
//!   parsing) + the unit tests.
//!
//! Every handler is re-exported here so call sites keep the stable
//! `inspect::finder::fs_*` path regardless of which sub-module owns the fn.

mod download;
mod listing;
mod mutate;
mod sheet;
pub(super) mod support;
mod upload;

pub use download::{fs_download, fs_raw};
pub use listing::{conversation, fs_descriptions, fs_list, fs_preview};
pub use mutate::{fs_mkdir, fs_move, fs_rename, fs_trash, fs_write};
pub use sheet::fs_sheet;
pub use upload::{fs_upload, fs_upload_unique};
