//! Background persistence writer
//!
//! Receives serialized write operations from the main thread and executes
//! them on a dedicated I/O thread. Debounces rapid saves (coalesces writes
//! within 50ms) to reduce disk I/O during high-frequency save_state calls.
//!
//! The main thread does the CPU work (serialization), the writer thread
//! does the I/O work (file writes). This keeps the event loop responsive.
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A single file write operation
#[derive(Debug, Clone)]
pub struct WriteOp {
    pub path: PathBuf,
    pub content: Vec<u8>,
}

/// A single file delete operation
#[derive(Debug, Clone)]
pub struct DeleteOp {
    pub path: PathBuf,
}

/// A batch of persistence operations to execute atomically
#[derive(Debug, Clone)]
pub struct WriteBatch {
    pub writes: Vec<WriteOp>,
    pub deletes: Vec<DeleteOp>,
    /// Directories to ensure exist before writing
    pub ensure_dirs: Vec<PathBuf>,
}

/// Messages sent to the writer thread
enum WriterMsg {
    /// A new batch of writes (replaces any pending batch for debounce)
    Batch(WriteBatch),
    /// Save a single message file (not debounced — written immediately)
    Message(WriteOp),
    /// Flush all pending writes and signal completion
    Flush,
    /// Shutdown the writer thread
    Shutdown,
}

/// Handle to the background persistence writer
pub struct PersistenceWriter {
    tx: Sender<WriterMsg>,
    /// Shared state for flush synchronization
    flush_sync: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

/// Debounce window in milliseconds
const DEBOUNCE_MS: u64 = 50;

impl PersistenceWriter {
    /// Create a new persistence writer with a background thread
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let flush_sync = Arc::new((Mutex::new(false), Condvar::new()));
        let flush_sync_clone = flush_sync.clone();

        let handle = thread::Builder::new()
            .name("persistence-writer".to_string())
            .spawn(move || {
                writer_loop(rx, flush_sync_clone);
            })
            .expect("failed to spawn persistence writer thread");

        Self { tx, flush_sync, handle: Some(handle) }
    }

    /// Queue a batch of writes (debounced — may be coalesced with subsequent batches)
    pub fn send_batch(&self, batch: WriteBatch) {
        let _ = self.tx.send(WriterMsg::Batch(batch));
    }

    /// Queue a single message write (not debounced — written on next iteration)
    pub fn send_message(&self, op: WriteOp) {
        let _ = self.tx.send(WriterMsg::Message(op));
    }

    /// Flush all pending writes synchronously. Blocks until complete.
    /// Used on app exit to ensure all state is persisted.
    pub fn flush(&self) {
        // Reset the flush flag
        {
            let (lock, _) = &*self.flush_sync;
            let mut flushed = lock.lock().unwrap_or_else(|e| e.into_inner());
            *flushed = false;
        }

        // Send flush request
        let _ = self.tx.send(WriterMsg::Flush);

        // Wait for the writer to signal completion
        let (lock, cvar) = &*self.flush_sync;
        let mut flushed = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*flushed {
            // Timeout after 5 seconds to prevent infinite hang on shutdown
            let result = cvar.wait_timeout(flushed, Duration::from_secs(5)).unwrap_or_else(|e| e.into_inner());
            flushed = result.0;
            if result.1.timed_out() {
                break;
            }
        }
    }

    /// Shutdown the writer thread gracefully
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(WriterMsg::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PersistenceWriter {
    fn drop(&mut self) {
        self.flush();
        self.shutdown();
    }
}

/// The writer thread's main loop
fn writer_loop(rx: Receiver<WriterMsg>, flush_sync: Arc<(Mutex<bool>, Condvar)>) {
    let mut pending_batch: Option<WriteBatch> = None;
    let mut pending_messages: Vec<WriteOp> = Vec::new();

    loop {
        // If we have a pending batch, wait with timeout (debounce)
        // If no pending batch, wait indefinitely for the next message
        let msg = if pending_batch.is_some() {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(msg) => Some(msg),
                Err(mpsc::RecvTimeoutError::Timeout) => None, // Debounce expired — flush
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break, // Channel disconnected
            }
        };

        match msg {
            Some(WriterMsg::Batch(batch)) => {
                // Replace the pending batch (coalesce — only the latest state matters)
                pending_batch = Some(batch);
                // Don't write yet — wait for debounce timeout
                continue;
            }
            Some(WriterMsg::Message(op)) => {
                // Messages are not debounced — queue for immediate write
                pending_messages.push(op);
                // But don't interrupt the debounce loop — write when we flush
                continue;
            }
            Some(WriterMsg::Flush) => {
                // Write everything immediately
                execute_pending_messages(&mut pending_messages);
                execute_batch(pending_batch.take());
                // Signal flush completion
                let (lock, cvar) = &*flush_sync;
                let mut flushed = lock.lock().unwrap_or_else(|e| e.into_inner());
                *flushed = true;
                cvar.notify_all();
                continue;
            }
            Some(WriterMsg::Shutdown) => {
                // Final write + exit
                execute_pending_messages(&mut pending_messages);
                execute_batch(pending_batch.take());
                break;
            }
            None => {
                // Debounce timeout expired — write pending batch
                execute_pending_messages(&mut pending_messages);
                execute_batch(pending_batch.take());
            }
        }
    }
}

/// Execute all pending message writes
fn execute_pending_messages(messages: &mut Vec<WriteOp>) {
    for op in messages.drain(..) {
        write_file(&op.path, &op.content);
    }
}

/// Execute a batch of write/delete operations
fn execute_batch(batch: Option<WriteBatch>) {
    let Some(batch) = batch else { return };

    // Ensure directories exist
    for dir in &batch.ensure_dirs {
        if let Err(e) = fs::create_dir_all(dir) {
            eprintln!("[persistence] failed to create dir {}: {}", dir.display(), e);
        }
    }

    // Execute writes
    for op in &batch.writes {
        write_file(&op.path, &op.content);
    }

    // Execute deletes
    for op in &batch.deletes {
        if let Err(e) = fs::remove_file(&op.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("[persistence] failed to delete {}: {}", op.path.display(), e);
        }
    }
}

/// Write a file, creating parent directories if needed.
/// Logs errors instead of silently swallowing them.
fn write_file(path: &PathBuf, content: &[u8]) {
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        eprintln!("[persistence] failed to create dir {}: {}", parent.display(), e);
        return;
    }
    if let Err(e) = fs::write(path, content) {
        eprintln!("[persistence] failed to write {}: {}", path.display(), e);
    }
}
