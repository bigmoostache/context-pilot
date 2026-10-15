//! One command connection's socket I/O, bounded by a **whole-connection**
//! deadline.
//!
//! The previous intake set a 500 ms timeout per `read`, so a peer that sent one
//! byte every 400 ms could hold the reader for as long as it liked (only the
//! 32 MiB buffer cap eventually ended it). [`Conn`] instead fixes a deadline
//! when the connection is accepted, and every read and write gets the time
//! *remaining* until that deadline as its timeout. Once the deadline passes,
//! the connection fails with [`ErrorKind::TimedOut`] whatever the peer is doing.

use std::io::{self, ErrorKind, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// Largest accumulated read (per connection) before a frame is abandoned as
/// junk — bounds memory against a peer that sends an endless un-decodable
/// stream. 32 MiB comfortably fits any realistic command frame (a `SendMessage`
/// carries the full message text); the old 1 MiB cap rejected a large paste,
/// contributing to the "big messages don't go through" symptom (T274). Kept in
/// lockstep with the backend transport's `MAX_BODY` (the other cap on the same
/// path).
pub(super) const MAX_CONNECTION_BUFFER: usize = 32 * 1024 * 1024;

/// Read-chunk size for one `read` call.
const READ_CHUNK: usize = 4096;

/// A connected commander plus its unconsumed input and its deadline.
pub(super) struct Conn<'stream> {
    /// The peer socket, put in blocking mode by [`Conn::new`].
    stream: &'stream mut UnixStream,
    /// Bytes read but not yet consumed as a whole frame.
    buf: Vec<u8>,
    /// Scratch buffer for a single `read`.
    chunk: Box<[u8]>,
    /// Instant after which every further read or write fails.
    deadline: Instant,
}

impl<'stream> Conn<'stream> {
    /// Wrap `stream`, giving the whole connection `budget` to complete.
    ///
    /// The stream is forced to blocking mode: on macOS (and Linux without
    /// `accept4`) an accepted stream inherits the listener's non-blocking flag,
    /// and a non-blocking read would fail with `WouldBlock` the moment the
    /// socket buffer drains mid-frame, cutting any command larger than the
    /// ~8 KiB `AF_UNIX` buffer (T274). With blocking reads, the deadline is the
    /// only thing that ends a slow peer.
    pub(super) fn new(stream: &'stream mut UnixStream, budget: Duration) -> io::Result<Self> {
        stream.set_nonblocking(false)?;
        Ok(Self {
            stream,
            buf: Vec::new(),
            chunk: vec![0u8; READ_CHUNK].into_boxed_slice(),
            deadline: Instant::now().checked_add(budget).unwrap_or_else(Instant::now),
        })
    }

    /// Unconsumed input, for the caller's frame decoder.
    pub(super) fn buffered(&self) -> &[u8] {
        &self.buf
    }

    /// Drop the first `consumed` bytes (one decoded frame).
    pub(super) fn consume(&mut self, consumed: usize) {
        let _drained: Vec<u8> = self.buf.drain(..consumed.min(self.buf.len())).collect();
    }

    /// Time left before the deadline, or a `TimedOut` error once it has passed.
    fn remaining(&self) -> io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(ErrorKind::TimedOut, "command connection deadline exceeded"));
        }
        Ok(left)
    }

    /// Read one more chunk. `Ok(false)` means the peer closed its write side.
    ///
    /// # Errors
    ///
    /// `TimedOut` once the deadline passes (including a read that blocks past
    /// it), `InvalidData` once the buffer exceeds [`MAX_CONNECTION_BUFFER`], or
    /// any socket fault.
    pub(super) fn read_more(&mut self) -> io::Result<bool> {
        if self.buf.len() > MAX_CONNECTION_BUFFER {
            return Err(io::Error::new(ErrorKind::InvalidData, "frame buffer overflow"));
        }
        tolerate_closed_peer(self.stream.set_read_timeout(Some(self.remaining()?)))?;
        let read = match self.stream.read(&mut self.chunk) {
            Ok(n) => n,
            // A blocking read that times out reports WouldBlock on Unix.
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                return Err(io::Error::new(ErrorKind::TimedOut, "command connection deadline exceeded"));
            }
            Err(e) => return Err(e),
        };
        if let Some(got) = self.chunk.get(..read) {
            self.buf.extend_from_slice(got);
        }
        Ok(read > 0)
    }

    /// Write one reply, failing rather than blocking past the deadline.
    ///
    /// # Errors
    ///
    /// `TimedOut` once the deadline passes, or any socket fault (a peer that
    /// stops reading its acks fills the socket buffer and hits the deadline).
    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        tolerate_closed_peer(self.stream.set_write_timeout(Some(self.remaining()?)))?;
        self.stream.write_all(bytes)
    }
}

/// Accept the `EINVAL` macOS returns from `SO_RCVTIMEO`/`SO_SNDTIMEO` on a
/// socket whose peer has fully closed. Such a peer can no longer stall us: a
/// read returns EOF at once and a write fails with `EPIPE`.
fn tolerate_closed_peer(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == ErrorKind::InvalidInput => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trickling_peer_is_cut_off_at_the_deadline() {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        let writer = std::thread::spawn(move || {
            // One byte every 30 ms: each read finishes well inside a per-read
            // timeout, so only a whole-connection deadline can stop this.
            for _ in 0..40u8 {
                if client.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });

        let started = Instant::now();
        let mut conn = Conn::new(&mut server, Duration::from_millis(200)).expect("conn");
        let err = loop {
            match conn.read_more() {
                Ok(true) => {}
                Ok(false) => panic!("peer must not reach EOF before the deadline"),
                Err(e) => break e,
            }
        };
        assert_eq!(err.kind(), ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(600), "cut off near the 200 ms budget");
        drop(conn);
        drop(server);
        writer.join().expect("writer");
    }

    #[test]
    fn a_silent_peer_times_out_instead_of_blocking_forever() {
        let (_client, mut server) = UnixStream::pair().expect("pair");
        let mut conn = Conn::new(&mut server, Duration::from_millis(50)).expect("conn");
        let err = conn.read_more().expect_err("silent peer must time out");
        assert_eq!(err.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn eof_and_consume_behave() {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        client.write_all(b"abc").expect("write");
        drop(client);
        let mut conn = Conn::new(&mut server, Duration::from_secs(1)).expect("conn");
        assert!(conn.read_more().expect("read"));
        assert_eq!(conn.buffered(), b"abc");
        conn.consume(2);
        assert_eq!(conn.buffered(), b"c");
        assert!(!conn.read_more().expect("eof"), "closed peer reads as EOF");
    }
}
