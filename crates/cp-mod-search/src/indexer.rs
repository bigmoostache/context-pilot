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

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::meili::client::MeiliClient;
use crate::ocr;
use crate::splitter::SplitterChain;
use crate::types;
use crate::types::IndexerCmd;

/// Duration to wait after the first event before processing a batch.
const DEBOUNCE_MS: u64 = 200;

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
    /// Optional Datalab OCR client for converting PDFs/images to text.
    /// `None` when `DATALAB_API_KEY` is not set.
    ocr_client: Option<ocr::DatalabClient>,
}

/// Start the background indexer and file watcher.
///
/// Returns the command sender and watcher handle.  The indexer thread
/// runs until the sender is dropped.
///
/// # Errors
///
/// Returns an error if the file watcher cannot be created.
pub(crate) fn start(params: IndexerParams) -> Result<(mpsc::Sender<IndexerCmd>, RecommendedWatcher), String> {
    let (tx, rx) = mpsc::channel::<IndexerCmd>();

    // Clone sender for the watcher callback
    let watcher_tx = tx.clone();

    // Set up file watcher
    let mut watcher = RecommendedWatcher::new(
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
        notify::Config::default(),
    )
    .map_err(|e| format!("Cannot create file watcher: {e}"))?;

    watcher
        .watch(&params.project_root, RecursiveMode::Recursive)
        .map_err(|e| format!("Cannot watch project root: {e}"))?;

    // Spawn initial scan on a helper thread (queues IndexFile commands)
    let scan_tx = tx.clone();
    let scan_root = params.project_root.clone();
    let _scan_handle = std::thread::Builder::new()
        .name("search-scan".into())
        .spawn(move || {
            scan_directory(&scan_tx, &scan_root);
        })
        .map_err(|e| format!("Cannot spawn scan thread: {e}"))?;

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

    let ctx = IndexerCtx {
        client,
        files_uid: format!("cp_{}_files", params.project_hash),
        project_root: params.project_root.clone(),
        splitter: SplitterChain::new(),
        ocr_client: ocr::api_key_from_env().and_then(|key| match ocr::DatalabClient::new(&key) {
            Ok(c) => {
                log::info!("Indexer: Datalab OCR enabled");
                Some(c)
            }
            Err(e) => {
                log::warn!("Indexer: cannot create Datalab client: {e}");
                None
            }
        }),
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
                    index_one_file(&ctx, path);
                }
                IndexerCmd::DeleteFile(ref path) => {
                    delete_one_file(&ctx, path);
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
fn deduplicate(batch: Vec<IndexerCmd>) -> Vec<IndexerCmd> {
    let mut latest: HashMap<PathBuf, IndexerCmd> = HashMap::new();

    for cmd in batch {
        match &cmd {
            IndexerCmd::IndexFile(p) | IndexerCmd::DeleteFile(p) => {
                let _prev = latest.insert(p.clone(), cmd);
            }
        }
    }

    latest.into_values().collect()
}

// -- File indexing -----------------------------------------------------------

/// Index a single file: read → filter → split → upload.
fn index_one_file(ctx: &IndexerCtx, abs_path: &Path) {
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

    // Check extension allowlist (text files) or OCR eligibility (binary files)
    let ext = rel_path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    let is_text = types::is_allowed_extension(ext);
    let is_ocr = ocr::is_ocr_extension(ext);
    if !is_text && !is_ocr {
        return;
    }

    // Check excluded file patterns
    let filename = rel_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if types::is_excluded_file(filename) {
        return;
    }

    // Check file size (different limits for text vs OCR)
    let Ok(meta) = std::fs::metadata(abs_path) else {
        return;
    };
    let size_limit = if is_ocr { ocr::MAX_OCR_FILE_SIZE } else { types::MAX_FILE_SIZE };
    if meta.len() > size_limit {
        return;
    }

    // Get text content — either read directly or convert via OCR
    let content = if is_ocr {
        // OCR path: convert binary file to markdown via Datalab API
        let Some(ref ocr_client) = ctx.ocr_client else {
            // No API key — skip silently
            return;
        };
        match ocr_client.convert_to_text(abs_path) {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) => return, // Empty OCR result — nothing to index
            Err(e) => {
                log::warn!("OCR failed for {rel_str}: {e}");
                return;
            }
        }
    } else {
        // Text path: read file directly (skip binary files that fail UTF-8)
        let Ok(text) = std::fs::read_to_string(abs_path) else {
            return;
        };
        text
    };

    // Delete existing chunks for this path (delete → re-insert strategy)
    let escaped = rel_str.replace('\'', "\\'");
    let filter = format!("file_path = '{escaped}'");
    if let Ok(task) = ctx.client.delete_documents_by_filter(&ctx.files_uid, &filter) {
        let _r = ctx.client.wait_for_task(task);
    }

    // Split into chunks
    let chunks = ctx.splitter.split(&content, rel_path);
    if chunks.is_empty() {
        return;
    }

    // Build Meilisearch documents
    let last_modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0_u64, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));

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
        let _r = ctx.client.wait_for_task(task);
    }
}

/// Delete all indexed chunks for a single file.
fn delete_one_file(ctx: &IndexerCtx, abs_path: &Path) {
    let rel_path = abs_path.strip_prefix(&ctx.project_root).unwrap_or(abs_path);
    let rel_str = rel_path.to_string_lossy();
    let escaped = rel_str.replace('\'', "\\'");
    let filter = format!("file_path = '{escaped}'");

    if let Ok(task) = ctx.client.delete_documents_by_filter(&ctx.files_uid, &filter) {
        let _r = ctx.client.wait_for_task(task);
    }
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
