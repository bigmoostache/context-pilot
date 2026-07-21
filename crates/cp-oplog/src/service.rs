//! [`OplogService`] — the off-loop group-commit thread and its asymmetric
//! backpressure policy (design doc GAP 2 / I2).
//!
//! # The problem GAP 2 names
//!
//! The agent's main loop must never `fsync` (design doc I2): durability latency
//! cannot ride the hot path. So oplog appends are *enqueued* and a dedicated
//! thread performs the `fdatasync`. But that raises the symmetric question the
//! stream tee never has to answer — **what happens when the oplog thread can't
//! keep up** (a slow disk, an `fsync` storm)?
//!
//! The stream tee may *drop*: tokens are disposable. The oplog cannot drop a
//! command effect — it is durable truth. An unbounded queue would grow memory
//! without limit under a slow disk; a bounded-and-blocking queue would stall
//! whoever submits. There is no single right answer, because the two record
//! classes have **opposite** needs:
//!
//! * **Durability-gated** records — command effects, seen-marks, message heads,
//!   lifecycle, checkpoints — *cannot* be lost. Under pressure the submitter
//!   **blocks by design**: correctness beats latency. The block lands on the
//!   *submitting* thread (the command-intake path, which is already off the
//!   render loop for journal-then-ack, design doc I11), never on the loop.
//! * **Best-effort** records — phase transitions, cost aggregates — *may* be
//!   dropped or coalesced under pressure. A lost intermediate phase self-heals:
//!   replay reconstructs the latest phase, and the live stream already carried
//!   the hint (design doc I10/K6). A dropped cost sample is re-aggregated later.
//!
//! [`Service`] implements exactly this asymmetry: [`append_durable`] blocks
//! on a bounded channel and returns the durable `rev`; [`append_best_effort`]
//! is fire-and-forget and reports [`BestEffortOutcome::Dropped`] when the queue
//! is full. [`Durability::of`] is the pure, tested predicate that classifies a
//! record so callers route it through the right door.
//!
//! [`append_durable`]: Service::append_durable
//! [`append_best_effort`]: Service::append_best_effort
//!
//! # Group commit
//!
//! The thread drains as many queued jobs as are immediately available (up to
//! [`MAX_BATCH`]), [`append_buffered`](crate::append::OplogWriter::append_buffered)s
//! each, then calls [`sync`](crate::append::OplogWriter::sync) **once** — a
//! single `fdatasync` amortised across the whole batch. Only after the sync are
//! the batch's durable submitters released with their `rev`s, preserving
//! announce-after-durable (design doc K9). Because the channel is FIFO, when an
//! `append_durable` returns, every record the thread received before it is also
//! durable — so a durable append doubles as a flush barrier.

use std::io;
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};

use cp_wire::types::oplog::OpEntryKind;

use crate::append::{DEFAULT_SEGMENT_LIMIT, OplogWriter};
use crate::error::{Error, OplogResult};

/// The post-sync result handed back to a durable submitter: its assigned `rev`,
/// or an error message (the channel carries text because [`Error`] is not
/// `Clone`).
type DurableAck = Result<u64, String>;

/// A queued durable record's reply channel paired with its buffered append
/// result, awaiting the group `fdatasync` before release.
type PendingAck = (SyncSender<DurableAck>, DurableAck);

/// Default bound on the job queue. A full queue blocks durable submitters and
/// drops best-effort ones — the GAP 2 asymmetry.
pub const DEFAULT_QUEUE_CAPACITY: usize = 1024;

/// Maximum records a single group commit drains before forcing its `fdatasync`,
/// bounding worst-case commit latency under a sustained burst.
pub const MAX_BATCH: usize = 1024;

/// How a record must be treated under queue pressure (design doc GAP 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// Must never be lost — the submitter blocks rather than drop it.
    Durable,

    /// May be dropped or coalesced under pressure; it self-heals on replay.
    BestEffort,
}

impl Durability {
    /// Classify a record by whether losing it would break correctness.
    ///
    /// Only phase transitions, cost aggregates, and focus changes are
    /// best-effort — a dropped phase is re-derived on replay and the stream
    /// carried its hint, cost samples re-aggregate, and a dropped focus change
    /// self-heals from the agent's tier-② `FocusState` (it is disposable UI
    /// state, not durable truth). Everything else is durability-gated, and an
    /// `Unknown` record (from a newer schema this build does not understand) is
    /// treated as `Durable` conservatively: it might be effect-bearing, so it is
    /// never silently dropped.
    #[must_use]
    pub const fn of(kind: &OpEntryKind) -> Self {
        match *kind {
            OpEntryKind::PhaseTransition { .. }
            | OpEntryKind::CostAggregate { .. }
            | OpEntryKind::ContextUsage { .. }
            | OpEntryKind::ThreadFocusChanged { .. } => Self::BestEffort,
            OpEntryKind::CommandEffect { .. }
            | OpEntryKind::SeenMark { .. }
            | OpEntryKind::MessageCreated { .. }
            | OpEntryKind::MessageDeleted { .. }
            | OpEntryKind::ThreadCreated { .. }
            | OpEntryKind::ThreadArchived { .. }
            | OpEntryKind::ThreadRestored { .. }
            | OpEntryKind::ThreadPaused { .. }
            | OpEntryKind::ThreadResumed { .. }
            | OpEntryKind::ThreadDeleted { .. }
            | OpEntryKind::ThreadStatusChanged { .. }
            | OpEntryKind::Lifecycle { .. }
            | OpEntryKind::Checkpoint { .. }
            | OpEntryKind::Unknown => Self::Durable,
        }
    }
}

/// The fate of an [`append_best_effort`](OplogService::append_best_effort) call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BestEffortOutcome {
    /// Accepted into the queue (it will be written on the next group commit).
    /// Acceptance is *not* durability — a best-effort record is never awaited.
    Submitted,

    /// The queue was full (or the service stopped); the record was dropped, as
    /// the GAP 2 policy permits for this class.
    Dropped,
}

/// A unit of work for the commit thread.
enum Job {
    /// A durable record: its `rev` (or error) is returned via `ack` after sync.
    Durable {
        /// The record to append.
        kind: OpEntryKind,
        /// One-shot reply channel carrying the post-sync result.
        ack: SyncSender<DurableAck>,
    },

    /// A best-effort record: written if a batch picks it up, never awaited.
    BestEffort {
        /// The record to append.
        kind: OpEntryKind,
    },

    /// Drain the current batch, then stop the thread.
    Shutdown,
}

/// A handle to the off-loop oplog group-commit thread.
///
/// Submitters never `fsync`; they enqueue. The owned [`OplogWriter`] lives on
/// the commit thread, which group-commits batches. Drop or [`shutdown`] to stop
/// the thread cleanly (the final batch is still synced).
///
/// [`shutdown`]: Service::shutdown
#[derive(Debug)]
pub struct Service {
    /// Bounded job queue — full means backpressure (block durable, drop best-effort).
    tx: SyncSender<Job>,

    /// The commit thread handle, joined on [`shutdown`](Self::shutdown).
    handle: Option<JoinHandle<()>>,
}

impl Service {
    /// Spawn a service over the oplog in `dir` with default segment and queue
    /// sizes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the oplog cannot be opened.
    pub fn spawn<P>(dir: P) -> OplogResult<Self>
    where
        P: AsRef<Path>,
    {
        Self::spawn_inner(dir.as_ref(), DEFAULT_SEGMENT_LIMIT, DEFAULT_QUEUE_CAPACITY)
    }

    /// Spawn with an explicit segment limit (used by tests to force rolls
    /// through the group-commit path without writing 64 MiB).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the oplog cannot be opened.
    pub fn spawn_with_segment_limit<P>(dir: P, segment_limit: u64) -> OplogResult<Self>
    where
        P: AsRef<Path>,
    {
        Self::spawn_inner(dir.as_ref(), segment_limit, DEFAULT_QUEUE_CAPACITY)
    }

    /// Open the writer and start the commit thread.
    fn spawn_inner(dir: &Path, segment_limit: u64, capacity: usize) -> OplogResult<Self> {
        let writer = OplogWriter::open_with_segment_limit(dir, segment_limit)?;
        let (tx, rx) = sync_channel::<Job>(capacity);
        let handle = thread::spawn(move || commit_loop(writer, &rx));
        Ok(Self { tx, handle: Some(handle) })
    }

    /// Append a durability-gated record, blocking until it is durable, and
    /// return its `rev`.
    ///
    /// If the queue is full the call blocks (the GAP 2 durability backpressure);
    /// it returns only after the commit thread has `fdatasync`'d the batch
    /// carrying this record, so the returned `rev` is always durable.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the service has stopped or the underlying
    /// append/sync failed.
    pub fn append_durable(&self, kind: OpEntryKind) -> OplogResult<u64> {
        let (ack_tx, ack_rx) = sync_channel::<DurableAck>(1);
        self.tx.send(Job::Durable { kind, ack: ack_tx }).map_err(|_ignored| stopped("oplog service stopped"))?;
        match ack_rx.recv() {
            Ok(Ok(rev)) => Ok(rev),
            Ok(Err(message)) => Err(stopped(message)),
            Err(_ignored) => Err(stopped("oplog service dropped the ack")),
        }
    }

    /// Submit a durability-gated record **without awaiting** its `fdatasync`.
    ///
    /// This is the I2 main-loop path: the record is enqueued for the commit
    /// thread, which group-commits and `fdatasync`s it **off-loop** — so it is
    /// just as durable as [`append_durable`](Self::append_durable), but the
    /// caller never blocks on the sync. It is *not* dropped under pressure like
    /// [`append_best_effort`](Self::append_best_effort): a full queue applies
    /// correct backpressure (the bounded `send` blocks only until the commit
    /// thread drains space), never silent loss. Use this for user-visible state
    /// mutations the main loop emits (thread roster, message heads) where losing
    /// the record would corrupt the view but blocking on each sync would violate
    /// the never-fsync-on-the-loop rule (design doc I2).
    ///
    /// The post-sync `rev` is discarded (the ack channel is detached); a caller
    /// that needs the durable `rev` must use [`append_durable`](Self::append_durable)
    /// instead. A stopped service is silently ignored — emission is advisory,
    /// never fatal to the agent.
    pub fn submit_durable(&self, kind: OpEntryKind) {
        // Detached ack: the commit thread still group-commits + fsyncs this
        // record durably; we simply drop the reply so the caller never waits.
        // `ack.send` on the commit side already tolerates a dropped receiver,
        // and a stopped service (send error) is benign for advisory emission.
        let (ack_tx, _ack_rx) = sync_channel::<DurableAck>(1);
        let _ignored = self.tx.send(Job::Durable { kind, ack: ack_tx });
    }

    /// Submit a best-effort record. Never blocks and never awaits durability;
    /// returns [`BestEffortOutcome::Dropped`] if the queue is full.
    #[must_use]
    pub fn append_best_effort(&self, kind: OpEntryKind) -> BestEffortOutcome {
        match self.tx.try_send(Job::BestEffort { kind }) {
            Ok(()) => BestEffortOutcome::Submitted,
            Err(TrySendError::Full(_dropped) | TrySendError::Disconnected(_dropped)) => BestEffortOutcome::Dropped,
        }
    }

    /// Stop the service: drain the in-flight batch, flush it, and join the
    /// thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the commit thread panicked.
    pub fn shutdown(mut self) -> OplogResult<()> {
        // Best-effort: if the queue is saturated the Shutdown may not fit, but
        // dropping every sender below still ends the loop.
        let _ignored = self.tx.try_send(Job::Shutdown);
        self.join()
    }

    /// Join the commit thread, if it has not already been joined.
    fn join(&mut self) -> OplogResult<()> {
        self.handle
            .take()
            .map_or(Ok(()), |handle| handle.join().map_err(|_ignored| stopped("oplog commit thread panicked")))
    }
}

impl Drop for Service {
    /// Ensure the commit thread is joined (and its final batch synced) even if
    /// [`shutdown`](Service::shutdown) was not called.
    fn drop(&mut self) {
        let _ignored = self.tx.try_send(Job::Shutdown);
        let _joined = self.join();
    }
}

/// Build an [`Error::Io`] from a message (the channel carries error text
/// because [`Error`] is not `Clone`).
fn stopped<M>(message: M) -> Error
where
    M: Into<String>,
{
    Error::Io(io::Error::other(message.into()))
}

/// The commit thread body: receive a job, drain a batch, group-commit, repeat.
fn commit_loop(mut writer: OplogWriter, rx: &Receiver<Job>) {
    while let Ok(first) = rx.recv() {
        let mut batch = Vec::with_capacity(MAX_BATCH);
        batch.push(first);
        while batch.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(job) => batch.push(job),
                Err(_empty_or_disconnected) => break,
            }
        }
        if commit_batch(&mut writer, batch) {
            break;
        }
    }
}

/// Write every record in `batch` buffered, sync once, then release durable
/// acks. Returns `true` if the batch contained a shutdown request.
fn commit_batch(writer: &mut OplogWriter, batch: Vec<Job>) -> bool {
    let mut acks: Vec<PendingAck> = Vec::new();
    let mut shutdown = false;

    for job in batch {
        match job {
            Job::Durable { kind, ack } => {
                let result = writer.append_buffered(kind).map_err(|e| e.to_string());
                acks.push((ack, result));
            }
            Job::BestEffort { kind } => {
                // A dropped best-effort write is acceptable (GAP 2); ignore the
                // rev and any framing error rather than failing the batch.
                let _ignored = writer.append_buffered(kind);
            }
            Job::Shutdown => shutdown = true,
        }
    }

    let sync_result = writer.sync();
    for (ack, buffered) in acks {
        // The rev is durable only if the group sync succeeded; a sync failure
        // overrides any per-record success.
        let final_result = if let Err(e) = sync_result.as_ref() { Err(e.to_string()) } else { buffered };
        let _ignored = ack.send(final_result);
    }
    shutdown
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
