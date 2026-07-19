//! Hourly reconcile + embedding-backup tick.
//!
//! One background thread per agent. Every [`TICK_INTERVAL`] it runs, in order:
//!
//! 1. [`reconcile`](crate::index::reconcile) — repair the index against the
//!    CURRENT disk (catches drift the live watcher missed: dropped FS events, a
//!    delete that raced startup).
//! 2. [`export_backup`](crate::index::backup::export_backup) — overwrite the
//!    in-folder embedding backup so it always reflects POST-reconcile truth.
//!
//! The two are deliberately paired (not two independent timers) so the backup
//! never captures a half-drifted snapshot. Both route through the single indexer
//! command channel / one `MeiliClient`, so there is no parallel-writer race.
//! A missed or slow tick is harmless — the next hour heals it, and the boot
//! reconcile + reimport recover anything in between.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::index::{backup, reconcile};
use crate::meili::api::MeiliClient;
use crate::types::IndexerCmd;

/// How often the tick runs (1 hour).
const TICK_INTERVAL: Duration = Duration::from_hours(1);

/// Everything the tick thread needs, bundled so `spawn` stays single-param.
pub(crate) struct TickParams {
    /// Meilisearch server port.
    pub port: u16,
    /// Meilisearch master key.
    pub master_key: String,
    /// Per-project index-name hash.
    pub project_hash: String,
    /// Project root (for the reconcile disk-walk + relative-path rebuild).
    pub project_root: PathBuf,
    /// Channel into the running indexer, used to queue the reconcile delta.
    pub indexer_tx: mpsc::Sender<IndexerCmd>,
}

/// Handle to the running tick thread. Dropping it stops the thread (and joins
/// it), so a TUI reload that replaces `SearchState` tears the old tick down
/// cleanly instead of stacking a second one.
pub(crate) struct BackupTickHandle {
    /// Guarded inner so the handle is `Sync` (stored in `State`'s `TypeMap`).
    inner: std::sync::Mutex<TickInner>,
}

/// Stop-signal + join handle, mutated only under the [`BackupTickHandle`] mutex.
struct TickInner {
    /// Send `()` (or drop) to ask the loop to stop at the next wake.
    stop: mpsc::Sender<()>,
    /// The tick thread, joined on drop.
    join: Option<JoinHandle<()>>,
}

impl BackupTickHandle {
    /// Spawn the hourly reconcile+export tick.
    pub(crate) fn spawn(params: TickParams) -> Self {
        let (stop, rx) = mpsc::channel::<()>();
        let join =
            std::thread::Builder::new().name("meili-backup-tick".to_owned()).spawn(move || run(&rx, &params)).ok();
        Self { inner: std::sync::Mutex::new(TickInner { stop, join }) }
    }
}

impl Drop for BackupTickHandle {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            let _s = inner.stop.send(());
            if let Some(handle) = inner.join.take() {
                let _j = handle.join();
            }
        }
    }
}

impl std::fmt::Debug for BackupTickHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BackupTickHandle(..)")
    }
}

/// The tick loop: sleep one interval (waking early on a stop signal), then run
/// one reconcile+export pass. Exits when the stop channel is signalled or its
/// sender is dropped.
fn run(rx: &mpsc::Receiver<()>, params: &TickParams) {
    loop {
        match rx.recv_timeout(TICK_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => tick(params),
        }
    }
}

/// One reconcile-then-export pass. Best-effort: any failure is logged and the
/// next tick retries.
fn tick(params: &TickParams) {
    let Ok(client) = MeiliClient::new(params.port, &params.master_key) else {
        return;
    };
    let files_uid = format!("cp_{}_files", params.project_hash);
    let logs_uid = format!("cp_{}_logs", params.project_hash);

    // 1. Reconcile against current disk, queueing the delta through the indexer.
    match reconcile::compute_plan(&client, &files_uid, &params.project_root) {
        Ok(plan) => {
            if !plan.is_empty() {
                log::info!(
                    "Hourly reconcile: {} to (re)index, {} to delete",
                    plan.to_index.len(),
                    plan.to_delete.len()
                );
            }
            reconcile::send_plan(&plan, &params.project_root, &params.indexer_tx);
        }
        Err(e) => log::warn!("Hourly reconcile failed: {e}"),
    }

    // 2. Export the post-reconcile backup (atomic overwrite).
    if let Err(e) = backup::export_backup(&client, &files_uid, &logs_uid) {
        log::warn!("Hourly embedding backup failed: {e}");
    }
}
