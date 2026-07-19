//! Boot-time server + indexer wiring, split from `lib.rs` for the line budget.
//!
//! Connects to (or starts) the global Meilisearch server, computes the offline
//! reconcile plan against a quiesced index, then spawns the background indexer +
//! file watcher and injects the delta. Called only from `load_module_data`.

use crate::index;
use crate::meili;
use crate::meili::api::MeiliClient;
use crate::types::{self, SearchPersistData};

/// Connect to (or start) the global Meilisearch server and stamp the resolved
/// port/master-key onto `persist`. Registers the project for orphan cleanup and
/// prunes stale indexes. On failure, zeroes the port (keyword-less fallback).
pub(crate) fn bootstrap_server(persist: &mut SearchPersistData, project_path: &str) {
    match meili::server::ensure_server_running() {
        Ok(info) => {
            persist.port = info.port;
            persist.master_key = info.master_key;
            let _r = meili::server::register_project(project_path, &persist.project_hash);
            meili::server::cleanup_orphan_indexes(persist.port, &persist.master_key);
        }
        Err(e) => {
            log::warn!("Meilisearch server not available: {e}");
            persist.port = 0;
            persist.master_key = String::new();
        }
    }

    // Ensure indexes + embedders exist (idempotent)
    if persist.port > 0
        && let Err(e) = meili::bootstrap::ensure_indexes(persist.port, &persist.master_key, &persist.project_hash)
    {
        log::warn!("Failed to ensure Meilisearch indexes: {e}");
    }
}

/// Compute the boot-time reconcile plan against a quiesced index.
///
/// Reimport warms an EMPTY index from the in-folder backup (zero Voyage) first,
/// so the returned plan only re-embeds files that genuinely drifted. Returns
/// `None` when the server is down or no plan could be computed.
pub(crate) fn compute_boot_plan(
    persist: &SearchPersistData,
    project_path: &str,
) -> Option<index::reconcile::ReconcilePlan> {
    if persist.port == 0 {
        return None;
    }
    let files_uid = format!("cp_{}_files", persist.project_hash);
    let logs_uid = format!("cp_{}_logs", persist.project_hash);

    if let Ok(client) = MeiliClient::new(persist.port, &persist.master_key) {
        index::backup::maybe_reimport(&client, &files_uid, &logs_uid);
    }

    let client = MeiliClient::new(persist.port, &persist.master_key).ok()?;
    index::reconcile::compute_plan(&client, &files_uid, std::path::Path::new(project_path)).ok()
}

/// Queue the offline reconcile delta onto the indexer channel, then mark the
/// initial scan complete. Logs a one-line summary when the plan is non-empty.
fn inject_reconcile_delta(
    tx: &std::sync::mpsc::Sender<types::IndexerCmd>,
    project_path: &str,
    reconcile_plan: Option<&index::reconcile::ReconcilePlan>,
) {
    if let Some(plan) = reconcile_plan {
        if !plan.is_empty() {
            log::info!("Reconcile: {} to (re)index, {} to delete", plan.to_index.len(), plan.to_delete.len());
        }
        index::reconcile::send_plan(plan, std::path::Path::new(project_path), tx);
    }
    let _r = tx.send(types::IndexerCmd::ScanComplete);
}

/// Start the background indexer + file watcher, then inject the offline
/// reconcile delta and mark the scan complete. Returns the channel + watcher
/// handle (both `None` when the server is down or the indexer failed to spawn).
pub(crate) fn spawn_indexer_pipeline(
    persist: &SearchPersistData,
    project_path: &str,
    metrics: &std::sync::Arc<std::sync::Mutex<types::SearchMetrics>>,
    reconcile_plan: Option<&index::reconcile::ReconcilePlan>,
) -> (Option<std::sync::mpsc::Sender<types::IndexerCmd>>, Option<types::WatcherHandle>) {
    if persist.port == 0 {
        return (None, None);
    }

    // `skip_initial_scan` is always true — the reconcile plan replaces the scan.
    let (indexer_tx, watcher) = match index::indexer::start(index::indexer::IndexerParams {
        port: persist.port,
        master_key: persist.master_key.clone(),
        project_hash: persist.project_hash.clone(),
        project_root: std::path::PathBuf::from(project_path),
        metrics: std::sync::Arc::clone(metrics),
        skip_initial_scan: true,
    }) {
        Ok((tx, w)) => (Some(tx), Some(types::WatcherHandle::new(w))),
        Err(e) => {
            log::warn!("Failed to start search indexer: {e}");
            (None, None)
        }
    };

    if let Some(tx) = &indexer_tx {
        inject_reconcile_delta(tx, project_path, reconcile_plan);
    }

    (indexer_tx, watcher)
}
