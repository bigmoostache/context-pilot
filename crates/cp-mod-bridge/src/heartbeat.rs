//! [`Beacon`] — the agent-side liveness heartbeat thread.
//!
//! A dedicated thread rewrites the agent's heartbeat file
//! (`<folder>/heartbeat`) in place at a fixed cadence so the backend can tell a
//! genuinely-running agent from a registry entry that merely *claims* to be
//! running (design doc §10 / D11). The record format — fixed-size, CRC-checked,
//! in-band timestamp — lives in [`cp_wire::heartbeat`]; this module owns only
//! the file and the thread.
//!
//! # Why in-place, not rename
//!
//! The record is exactly [`HEARTBEAT_LEN`](cp_wire::heartbeat::HEARTBEAT_LEN)
//! bytes, so each beat `seek(0)`s and overwrites the previous one: no
//! `tmp`+rename churn (which would thrash the directory and race a reader) and
//! no append growth. `sync_data` makes each beat durable; a crash mid-overwrite
//! leaves a torn record the reader rejects by CRC, never a falsely-fresh one.
//!
//! # Lifecycle
//!
//! [`Beacon::start`] writes the first beat synchronously (so a valid record
//! exists the instant it returns) and spawns the thread for every subsequent
//! beat. The thread blocks on a stop channel with a `cadence` timeout: a
//! timeout means "write the next beat", a received signal (or a dropped sender)
//! means "stop". Dropping the [`Beacon`] signals the thread and joins it, so the
//! heartbeat stops cleanly when the [`crate::boot::Boot`] it belongs to is
//! dropped.

use std::fs::{File, OpenOptions};
use std::io::{self, Seek as _, SeekFrom, Write as _};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cp_wire::heartbeat::{HEARTBEAT_SCHEMA_VERSION, Heartbeat};

/// The unchanging parameters of every beat: the identity stamped into each
/// record plus the loop's wake cadence. Bundled so the beat loop and the
/// per-beat writer each take one argument instead of a long parameter list.
#[derive(Clone, Debug)]
struct Beat {
    /// The agent's process id, stamped into every record.
    pid: u32,

    /// The 32-hex-char boot id binding each beat to the registry identity.
    boot_id: String,

    /// How long the loop waits between beats.
    cadence: Duration,
}

/// A running heartbeat beacon: the stop channel plus the thread writing beats.
///
/// Dropping it stops the thread (the final partial cadence is not awaited) and
/// joins it, so no orphaned beacon outlives the agent.
#[derive(Debug)]
pub struct Beacon {
    /// Sending (or dropping) this stops the beat loop at its next wake.
    stop: Sender<()>,

    /// The beat thread, joined on drop.
    handle: Option<JoinHandle<()>>,
}

impl Beacon {
    /// Open (creating if absent) the heartbeat file at `path`, write the first
    /// beat, and spawn the thread that writes every beat thereafter at
    /// `cadence`.
    ///
    /// `pid` and `boot_id` are stamped into every record: `pid` makes the
    /// record self-describing, and `boot_id` binds it to the registry entry's
    /// identity so a reused pid cannot masquerade as this agent (design doc
    /// §10 / D11).
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the file cannot be opened or the first beat
    /// cannot be written (`boot_id` not 32 bytes is mapped to
    /// [`io::ErrorKind::InvalidInput`]).
    pub fn start(path: &Path, pid: u32, boot_id: String, cadence: Duration) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;

        let beat = Beat { pid, boot_id, cadence };
        // The first beat is written before returning, so a reader that opens
        // the file the moment boot completes always finds a valid record.
        write_beat(&mut file, &beat, 0)?;

        let (stop, rx) = mpsc::channel::<()>();
        let handle = thread::spawn(move || beat_loop(file, &beat, &rx));
        Ok(Self { stop, handle: Some(handle) })
    }

    /// Stop the beacon and join its thread.
    ///
    /// Idempotent with [`Drop`]: calling this consumes the beacon so the drop
    /// is a no-op.
    pub fn stop(mut self) {
        self.signal_and_join();
    }

    /// Signal the thread to stop and join it (shared by [`stop`](Self::stop)
    /// and [`Drop`]).
    fn signal_and_join(&mut self) {
        // A send error means the thread already exited — nothing to stop.
        let _ignored = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _joined = handle.join();
        }
    }
}

impl Drop for Beacon {
    fn drop(&mut self) {
        self.signal_and_join();
    }
}

/// The beat thread body: wait one cadence, write the next beat, repeat, until
/// the stop channel fires or its sender is dropped.
///
/// The loop *waits first, then writes*, because beat `0` was already written
/// synchronously in [`Beacon::start`]; the first thread beat is `1`.
fn beat_loop(mut file: File, beat: &Beat, rx: &mpsc::Receiver<()>) {
    let mut sequence: u64 = 1;
    loop {
        match rx.recv_timeout(beat.cadence) {
            // Stop requested, or the beacon was dropped.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            // Cadence elapsed: emit the next beat. A write error is handled by
            // omission — the beat simply does not advance, and the reader will
            // see the record go stale, which is the correct liveness signal.
            Err(RecvTimeoutError::Timeout) => {
                let _ignored = write_beat(&mut file, beat, sequence);
                sequence = sequence.wrapping_add(1);
            }
        }
    }
}

/// Encode one beat and overwrite the file in place (`seek(0)` + `write_all` +
/// `sync_data`). The record is fixed-size, so the file never grows and no stale
/// tail can survive.
fn write_beat(file: &mut File, beat: &Beat, sequence: u64) -> io::Result<()> {
    let record = Heartbeat {
        schema_version: HEARTBEAT_SCHEMA_VERSION,
        timestamp_ms: now_ms(),
        sequence,
        pid: beat.pid,
        boot_id: beat.boot_id.clone(),
    };
    let bytes = record.encode().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

    let _pos = file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.sync_data()?;
    Ok(())
}

/// Wall-clock milliseconds since the Unix epoch, or `0` if the clock predates
/// it (freshness is relative, so a `0` floor is harmless).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::heartbeat::HEARTBEAT_LEN;
    use std::thread::sleep;
    use tempfile::tempdir;

    const BOOT: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn first_beat_is_written_synchronously() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("heartbeat");
        let beacon = Beacon::start(&path, 4242, BOOT.to_owned(), Duration::from_secs(10)).expect("start");

        // The file exists and holds a valid, fresh beat immediately.
        let bytes = std::fs::read(&path).expect("read");
        assert_eq!(bytes.len(), HEARTBEAT_LEN);
        let beat = Heartbeat::decode(&bytes).expect("decode");
        assert_eq!(beat.pid, 4242);
        assert_eq!(beat.sequence, 0, "the synchronous first beat is sequence 0");
        assert!(beat.matches_boot(BOOT));

        beacon.stop();
    }

    #[test]
    fn beats_advance_at_cadence() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("heartbeat");
        let beacon = Beacon::start(&path, 7, BOOT.to_owned(), Duration::from_millis(20)).expect("start");

        // Wait for a few cadences, then confirm the sequence has advanced past
        // the synchronous beat 0.
        sleep(Duration::from_millis(90));
        let bytes = std::fs::read(&path).expect("read");
        let beat = Heartbeat::decode(&bytes).expect("decode");
        assert!(beat.sequence >= 1, "the thread must have written at least one more beat");

        beacon.stop();
    }

    #[test]
    fn drop_stops_the_thread() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("heartbeat");
        {
            let _beacon = Beacon::start(&path, 1, BOOT.to_owned(), Duration::from_millis(10)).expect("start");
            sleep(Duration::from_millis(30));
        } // dropped here → thread joined.

        // Read the last sequence, wait well past several cadences, and confirm
        // it did not advance (the thread is gone).
        let first = Heartbeat::decode(&std::fs::read(&path).expect("read")).expect("decode").sequence;
        sleep(Duration::from_millis(60));
        let second = Heartbeat::decode(&std::fs::read(&path).expect("read")).expect("decode").sequence;
        assert_eq!(first, second, "no beat may be written after drop");
    }

    #[test]
    fn record_stays_fixed_size_across_beats() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("heartbeat");
        let beacon = Beacon::start(&path, 9, BOOT.to_owned(), Duration::from_millis(15)).expect("start");
        sleep(Duration::from_millis(70));
        // In-place overwrite must never grow the file.
        assert_eq!(std::fs::metadata(&path).expect("meta").len(), HEARTBEAT_LEN as u64);
        beacon.stop();
    }
}
