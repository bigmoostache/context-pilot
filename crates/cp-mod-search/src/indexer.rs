//! Background file indexer thread.
//!
//! Receives file-system events via an [`mpsc`] channel, reads and
//! chunks the files using the [`SplitterChain`], and indexes the
//! resulting documents into Meilisearch.
//!
//! Also performs the initial full-project scan on first boot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, PollWatcher, RecursiveMode, Watcher as _};

use crate::meili::api::MeiliClient;
use crate::splitter::SplitterChain;
use crate::types;
use crate::types::IndexerCmd;

/// Duration to wait after the first event before processing a batch.
const DEBOUNCE_MS: u64 = 200;

/// Poll interval for the file watcher.
///
/// Uses [`PollWatcher`] which periodically walks the directory tree and
/// diffs against its last known state — **zero kernel-level FDs** needed.
/// 3 seconds strikes a good balance between responsiveness and CPU overhead.
/// (The previous `RecommendedWatcher` with kqueue used one FD per file,
/// exhausting the 256-FD macOS default on any non-trivial project.)
const POLL_INTERVAL_SECS: u64 = 3;

/// Parameters for starting the background indexer.
pub(crate) struct IndexerParams {
    /// Meilisearch server port.
    pub port: u16,
    /// Meilisearch master key.
    pub master_key: String,
    /// Project hash for index naming.
    pub project_hash: String,
    /// Root directory of the project.
    pub project_root: PathBuf,
    /// Shared metrics updated by the indexer thread.
    pub metrics: std::sync::Arc<std::sync::Mutex<types::SearchMetrics>>,
    /// Skip the initial full-project scan.
    ///
    /// Set to `true` on TUI reload — Meilisearch already has data from
    /// the previous session and the `PollWatcher` picks up incremental
    /// changes.  Set to `false` on first boot (fresh indexes).
    pub skip_initial_scan: bool,
}

/// Internal context for the running indexer thread.
struct IndexerCtx {
    /// Meilisearch HTTP client.
    client: MeiliClient,
    /// Index UID for project files.
    files_uid: String,
    /// Root directory of the project.
    project_root: PathBuf,
    /// File splitter chain (tree-sitter → fixed-size fallback).
    splitter: SplitterChain,
    /// Shared metrics, updated as files are indexed.
    metrics: std::sync::Arc<std::sync::Mutex<types::SearchMetrics>>,
    /// Per-file last-indexed mtime (ms since epoch).
    ///
    /// Used to skip re-indexing files reported by [`PollWatcher`] whose
    /// content hasn't actually changed.  Without this, phantom watcher
    /// events trigger a full delete→split→upload→embed cycle on every
    /// poll interval, keeping Meilisearch at 200 %+ CPU via embedding
    /// regeneration even when the project is idle.
    last_indexed_mtime: HashMap<String, u64>,
}

/// Start the background indexer and file watcher.
///
/// Returns the command sender and watcher handle.  The indexer thread
/// runs until the sender is dropped.
///
/// # Errors
///
/// Returns an error if the file watcher cannot be created.
pub(crate) fn start(params: IndexerParams) -> Result<(mpsc::Sender<IndexerCmd>, PollWatcher), String> {
    let (tx, rx) = mpsc::channel::<IndexerCmd>();

    // Clone sender for the watcher callback
    let watcher_tx = tx.clone();

    // Set up polling file watcher — walks the tree every POLL_INTERVAL_SECS.
    // Unlike RecommendedWatcher (kqueue), PollWatcher uses ZERO kernel FDs
    // for watching.  The slight latency (≤3s) is fine for a search index.
    let mut watcher = PollWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                for path in &event.paths {
                    let cmd = match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => IndexerCmd::IndexFile(path.clone()),
                        EventKind::Remove(_) => IndexerCmd::DeleteFile(path.clone()),
                        EventKind::Access(_) | EventKind::Other | EventKind::Any => continue,
                    };
                    let _r = watcher_tx.send(cmd);
                }
            }
        },
        notify::Config::default().with_poll_interval(Duration::from_secs(POLL_INTERVAL_SECS)),
    )
    .map_err(|e| format!("Cannot create file watcher: {e}"))?;

    watcher
        .watch(&params.project_root, RecursiveMode::Recursive)
        .map_err(|e| format!("Cannot watch project root: {e}"))?;

    // Spawn initial scan on a helper thread (queues IndexFile commands)
    if params.skip_initial_scan {
        // Reload path: Meilisearch already has data from the previous session.
        // Mark scan as complete immediately — the PollWatcher handles incremental changes.
        if let Ok(mut m) = params.metrics.lock() {
            m.scan_complete = true;
        }
        log::info!("Skipping initial scan (reload with existing indexes)");
    } else {
        let scan_tx = tx.clone();
        let scan_root = params.project_root.clone();
        let _scan_handle = std::thread::Builder::new()
            .name("search-scan".into())
            .spawn(move || {
                scan_directory(&scan_tx, &scan_root);
                let _r = scan_tx.send(IndexerCmd::ScanComplete);
            })
            .map_err(|e| format!("Cannot spawn scan thread: {e}"))?;
    }

    // Spawn the indexer thread
    let _indexer_handle = std::thread::Builder::new()
        .name("search-indexer".into())
        .spawn(move || {
            indexer_loop(&rx, &params);
        })
        .map_err(|e| format!("Cannot spawn indexer thread: {e}"))?;

    Ok((tx, watcher))
}

// -- Indexer loop ------------------------------------------------------------

/// Main loop of the background indexer thread.
///
/// Blocks on the receiver, debounces incoming events for 200 ms,
/// deduplicates them, and processes each command.
fn indexer_loop(rx: &mpsc::Receiver<IndexerCmd>, params: &IndexerParams) {
    let client = match MeiliClient::new(params.port, &params.master_key) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Indexer: cannot create Meilisearch client: {e}");
            return;
        }
    };

    let mut ctx = IndexerCtx {
        client,
        files_uid: format!("cp_{}_files", params.project_hash),
        project_root: params.project_root.clone(),
        splitter: SplitterChain::new(),
        metrics: std::sync::Arc::clone(&params.metrics),
        last_indexed_mtime: HashMap::new(),
    };

    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];

        // Debounce: collect more events for DEBOUNCE_MS
        let deadline = Instant::now().checked_add(Duration::from_millis(DEBOUNCE_MS)).unwrap_or_else(Instant::now);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(cmd) => {
                    batch.push(cmd);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        // Deduplicate: keep only the latest operation per path
        let unique = deduplicate(batch);

        for cmd in unique {
            match cmd {
                IndexerCmd::IndexFile(ref path) => {
                    index_one_file(&mut ctx, path);
                }
                IndexerCmd::DeleteFile(ref path) => {
                    delete_one_file(&mut ctx, path);
                }
                IndexerCmd::ScanComplete => {
                    if let Ok(mut m) = ctx.metrics.lock() {
                        m.scan_complete = true;
                    }
                    log::info!("Initial project scan complete");
                }
            }
        }
    }
}

// -- Deduplication -----------------------------------------------------------

/// Keep only the latest command per path.
///
/// If the same path appears multiple times (e.g., rapid saves),
/// only the last command (Index or Delete) is kept.
/// `ScanComplete` is always preserved (appended at the end).
fn deduplicate(batch: Vec<IndexerCmd>) -> Vec<IndexerCmd> {
    let mut latest: HashMap<PathBuf, IndexerCmd> = HashMap::new();
    let mut has_scan_complete = false;

    for cmd in batch {
        match &cmd {
            IndexerCmd::IndexFile(p) | IndexerCmd::DeleteFile(p) => {
                let _prev = latest.insert(p.clone(), cmd);
            }
            IndexerCmd::ScanComplete => {
                has_scan_complete = true;
            }
        }
    }

    let mut result: Vec<IndexerCmd> = latest.into_values().collect();
    if has_scan_complete {
        result.push(IndexerCmd::ScanComplete);
    }
    result
}

// -- File indexing -----------------------------------------------------------

/// Index a single file: read → filter → split → upload.
fn index_one_file(ctx: &mut IndexerCtx, abs_path: &Path) {
    let _fg = cp_base::flame!("index_file");
    // Skip symlinks
    if abs_path.is_symlink() {
        return;
    }

    // Relative path for storage
    let rel_path = abs_path.strip_prefix(&ctx.project_root).unwrap_or(abs_path);
    let rel_str = rel_path.to_string_lossy();

    // Check path exclusions (directory components)
    for component in rel_path.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_str().unwrap_or("");
            if types::is_excluded_dir(name_str) {
                return;
            }
        }
    }

    // Check extension allowlist (text files only)
    let ext = rel_path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if !types::is_allowed_extension(ext) {
        return;
    }

    // Check excluded file patterns
    let filename = rel_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if types::is_excluded_file(filename) {
        return;
    }

    // Check file size
    let Ok(meta) = std::fs::metadata(abs_path) else {
        return;
    };
    if meta.len() > types::MAX_FILE_SIZE {
        return;
    }

    // Compute mtime for deduplication — skip re-indexing unchanged files.
    // PollWatcher can fire phantom events for files whose content hasn't
    // changed (metadata updates, macOS quirks). Without this check, each
    // phantom event triggers delete→split→upload→embed, keeping Meilisearch
    // pegged at 200%+ CPU from constant embedding regeneration.
    let last_modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0_u64, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));

    if ctx.last_indexed_mtime.get(rel_str.as_ref()).is_some_and(|&t| t == last_modified_ms) {
        return; // File unchanged since last index — skip
    }

    // Read file content (skip binary files that fail UTF-8)
    let Ok(content) = std::fs::read_to_string(abs_path) else {
        return;
    };

    // Delete existing chunks for this path (delete → re-insert strategy)
    let escaped = rel_str.replace('\'', "\\'");
    let filter = format!("file_path = '{escaped}'");
    if let Ok(task) = ctx.client.delete_documents_by_filter(&ctx.files_uid, &filter) {
        let _r = crate::meili::tasks::wait_for_task(&ctx.client, task);
    }

    // Split into chunks
    let chunks = ctx.splitter.split(&content, rel_path);
    if chunks.is_empty() {
        return;
    }

    // Build Meilisearch documents
    let docs: Vec<serde_json::Value> = chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            // Meilisearch IDs: only [a-zA-Z0-9_-] allowed.
            let safe_id: String = format!("{rel_str}-{i}")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect();
            serde_json::json!({
                "id": safe_id,
                "file_path": rel_str,
                "content": chunk.content,
                "extension": ext,
                "chunk_type": chunk.kind,
                "chunk_name": chunk.name,
                "line_start": chunk.line_start,
                "line_end": chunk.line_end,
                "char_start": chunk.char_start,
                "char_end": chunk.char_end,
                "last_modified_ms": last_modified_ms,
            })
        })
        .collect();

    // Send to Meilisearch
    if let Ok(task) = ctx.client.add_documents(&ctx.files_uid, &serde_json::Value::Array(docs)) {
        let _r = crate::meili::tasks::wait_for_task(&ctx.client, task);
    }

    // Update metrics
    if let Ok(mut m) = ctx.metrics.lock() {
        m.files_indexed = m.files_indexed.saturating_add(1);
        let chunk_count = u64::try_from(chunks.len()).unwrap_or(0);
        m.chunks_indexed = m.chunks_indexed.saturating_add(chunk_count);
        let count = m.extension_counts.entry(ext.to_string()).or_insert(0);
        *count = count.saturating_add(1);
        for chunk in &chunks {
            if chunk.kind == "raw" {
                m.fallback_chunks = m.fallback_chunks.saturating_add(1);
            } else {
                m.tree_sitter_chunks = m.tree_sitter_chunks.saturating_add(1);
            }
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        m.last_activity_ms = now_ms;

        // Track per-file recompute counts and last-sent timestamps
        let rc = m.recompute_counts.entry(rel_str.to_string()).or_insert(0);
        *rc = rc.saturating_add(1);
        let _prev = m.last_sent_ms.insert(rel_str.to_string(), now_ms);
    }

    // Record mtime so subsequent PollWatcher events for this unchanged
    // file are skipped (the key optimisation that prevents phantom re-indexing).
    let _prev = ctx.last_indexed_mtime.insert(rel_str.to_string(), last_modified_ms);
}

/// Delete all indexed chunks for a single file.
fn delete_one_file(ctx: &mut IndexerCtx, abs_path: &Path) {
    let rel_path = abs_path.strip_prefix(&ctx.project_root).unwrap_or(abs_path);
    let rel_str = rel_path.to_string_lossy();
    let escaped = rel_str.replace('\'', "\\'");
    let filter = format!("file_path = '{escaped}'");

    if let Ok(task) = ctx.client.delete_documents_by_filter(&ctx.files_uid, &filter) {
        let _r = crate::meili::tasks::wait_for_task(&ctx.client, task);
    }

    // Clear cached mtime so the file gets re-indexed if recreated
    let _prev = ctx.last_indexed_mtime.remove(rel_str.as_ref());
}

// -- Directory scan ----------------------------------------------------------

/// Recursively scan a directory and queue eligible files for indexing.
///
/// Skips symlinks, excluded directories, and sends `IndexFile` for
/// every regular file encountered.  Filtering (extension, size) is
/// done by the indexer thread when it processes each command.
fn scan_directory(tx: &mpsc::Sender<IndexerCmd>, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip symlinks
        if path.is_symlink() {
            continue;
        }

        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_str().unwrap_or("");
            if !types::is_excluded_dir(name_str) {
                scan_directory(tx, &path);
            }
        } else if path.is_file() {
            let _r = tx.send(IndexerCmd::IndexFile(path));
        }
    }
}
