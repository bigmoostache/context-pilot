//! File-indexing pipeline: indexability gates, the background indexer thread,
//! and boot/hourly filesystem⇄index reconciliation.
//!
//! Grouped into one sub-module so `src/` stays within the 8-entry directory cap.

/// Incarnation-agnostic embedding backup: export + reimport-on-empty.
pub mod backup;
/// Boot-time server + indexer wiring (split from lib.rs for the line budget).
pub mod boot;
/// File indexability gates (extension allowlist, exclusions, size cap, `is_indexable`).
pub(crate) mod filters;
/// Background file indexer thread and file watcher.
pub mod indexer;
/// Log → Meilisearch sync.
pub mod logsync;
/// Boot + hourly filesystem⇄index reconciliation sweep.
pub mod reconcile;
/// Hourly reconcile + embedding-backup tick.
pub mod tick;
