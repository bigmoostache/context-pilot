//! Background persistence writer
//!
//! Receives serialized write operations from the main thread and executes
//! them on a dedicated I/O thread. Debounces rapid saves (coalesces writes
//! within 50ms) to reduce disk I/O during high-frequency `save_state` calls.
//!
//! The main thread does the CPU work (serialization), the writer thread
//! does the I/O work (file writes). This keeps the event loop responsive.
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A single file write operation
#[derive(Debug, Clone)]
pub(crate) struct WriteOp {
    /// Target file path.
    pub path: PathBuf,
    /// Serialized bytes to write.
    pub content: Vec<u8>,
}

/// A single file delete operation
#[derive(Debug, Clone)]
pub(crate) struct DeleteOp {
    /// Target file path to remove.
    pub path: PathBuf,
}

/// A batch of persistence operations to execute atomically
#[derive(Debug, Clone)]
pub(crate) struct WriteBatch {
    /// File write operations in this batch.
    pub writes: Vec<WriteOp>,
    /// File delete operations in this batch.
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
    /// Flush all pending writes, then signal completion via the one-shot channel
    Flush(Sender<()>),
    /// Shutdown the writer thread
    Shutdown,
}

/// Handle to the background persistence writer
pub(crate) struct PersistenceWriter {
    /// Sender end of the writer message channel.
    tx: Sender<WriterMsg>,
    /// Join handle for the background I/O thread.
    handle: Option<JoinHandle<()>>,
}

/// Debounce window in milliseconds
const DEBOUNCE_MS: u64 = 50;

impl PersistenceWriter {
    /// Create a new persistence writer with a background thread
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            WriterThread { rx }.run();
        });

        Self { tx, handle: Some(handle) }
    }

    /// Queue a batch of writes (debounced — may be coalesced with subsequent batches)
    pub(crate) fn send_batch(&self, batch: WriteBatch) {
        let _r = self.tx.send(WriterMsg::Batch(batch));
    }

    /// Queue a single message write (not debounced — written on next iteration)
    pub(crate) fn send_message(&self, op: WriteOp) {
        let _r = self.tx.send(WriterMsg::Message(op));
    }

    /// Flush all pending writes synchronously. Blocks until complete.
    /// Used on app exit to ensure all state is persisted.
    pub(crate) fn flush(&self) {
        let (done_tx, done_rx) = mpsc::channel();
        let _send = self.tx.send(WriterMsg::Flush(done_tx));
        // Block until the writer signals completion (timeout 5s to prevent infinite hang)
        let _recv = done_rx.recv_timeout(Duration::from_secs(5));
    }

    /// Shutdown the writer thread gracefully
    pub(crate) fn shutdown(&mut self) {
        let _send = self.tx.send(WriterMsg::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _join = handle.join();
        }
    }
}

impl Drop for PersistenceWriter {
    fn drop(&mut self) {
        self.flush();
        self.shutdown();
    }
}

/// Background writer thread state. Owns the channel receiver.
struct WriterThread {
    /// Receiver end of the writer message channel.
    rx: Receiver<WriterMsg>,
}

impl WriterThread {
    /// Consume self and process write messages until disconnected.
    fn run(self) {
        let Self { rx } = self;
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
                }
                Some(WriterMsg::Message(op)) => {
                    // Messages are not debounced — queue for immediate write
                    pending_messages.push(op);
                    // But don't interrupt the debounce loop — write when we flush
                }
                Some(WriterMsg::Flush(done_tx)) => {
                    // Write everything immediately
                    execute_pending_messages(&mut pending_messages);
                    execute_batch(pending_batch.take());
                    // Signal completion — receiver wakes up
                    let _r = done_tx.send(());
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
            drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", dir.display(), e));
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
            drop(writeln!(std::io::stderr(), "[persistence] failed to delete {}: {}", op.path.display(), e));
        }
    }
}

/// Write a file, creating parent directories if needed.
/// Logs errors instead of silently swallowing them.
fn write_file(path: &PathBuf, content: &[u8]) {
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        drop(writeln!(std::io::stderr(), "[persistence] failed to create dir {}: {}", parent.display(), e));
        return;
    }
    if let Err(e) = fs::write(path, content) {
        drop(writeln!(std::io::stderr(), "[persistence] failed to write {}: {}", path.display(), e));
    }
}
