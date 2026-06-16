//! Server-Sent Events plumbing — the server→client streaming half of the
//! transport (design doc §9, roadmap P7-P8).
//!
//! SSE rides a single long-lived `GET` over plain chunked HTTP, so it fits
//! `tiny_http`'s blocking, thread-per-connection model exactly: the response
//! body is a [`SseBody`] reader whose `read` **blocks** until the next event is
//! ready, and `tiny_http` pumps it to the socket until the client disconnects.
//!
//! # Reconnect-replay by `rev` is native
//!
//! Each event carries an `id:` equal to the oplog `rev` it reflects. On a
//! dropped connection the browser's `EventSource` reconnects automatically,
//! sending `Last-Event-ID: <rev>`; the server resumes the oplog tail from that
//! `rev`. A gap the oplog can no longer cover is signalled with a `resync`
//! event so the client refetches a fresh snapshot over REST. This is the
//! design's hardest transport requirement met by the protocol itself, with no
//! hand-rolled replay layer.
//!
//! The producer that fills a stream is spawned by the caller; this module
//! supplies the wire encoding ([`SseMessage::encode`]), the blocking reader
//! ([`SseBody`]), and the channel that joins them ([`channel`]).

use std::io::{self, Read};
use std::sync::mpsc::{self, Receiver, Sender};

/// One Server-Sent Event: an optional `rev` id, an event name, and a data
/// payload (typically a single line of JSON).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseMessage {
    /// The `rev` this event reflects, emitted as the SSE `id:` field so a
    /// reconnecting client can resume from it via `Last-Event-ID`.
    pub id: Option<u64>,
    /// The SSE `event:` name (e.g. `"delta"`, `"stream"`, `"resync"`).
    pub event: String,
    /// The `data:` payload. Embedded newlines are split into multiple
    /// `data:` lines per the SSE grammar.
    pub data: String,
}

impl SseMessage {
    /// Build a `delta` event carrying an oplog entry at `rev`.
    #[must_use]
    pub fn delta(rev: u64, data: String) -> Self {
        Self { id: Some(rev), event: "delta".to_owned(), data }
    }

    /// Build a `stream` event carrying an ephemeral stream frame.
    #[must_use]
    pub fn stream(data: String) -> Self {
        Self { id: None, event: "stream".to_owned(), data }
    }

    /// Build a `resync` event telling the client to refetch a snapshot over
    /// REST because the requested `rev` is no longer replayable.
    #[must_use]
    pub fn resync() -> Self {
        Self { id: None, event: "resync".to_owned(), data: "{}".to_owned() }
    }

    /// Encode to the SSE wire format: an optional `id:` line, an `event:`
    /// line, one `data:` line per line of payload, terminated by a blank line.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        if let Some(id) = self.id {
            out.push_str("id: ");
            out.push_str(&id.to_string());
            out.push('\n');
        }
        out.push_str("event: ");
        out.push_str(&self.event);
        out.push('\n');
        for line in self.data.split('\n') {
            out.push_str("data: ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
        out.into_bytes()
    }
}

/// Encode an SSE comment line (`: text`), used as a keep-alive that costs the
/// client nothing but lets the server detect a dead socket on write.
#[must_use]
pub fn encode_comment(text: &str) -> Vec<u8> {
    let mut out = String::with_capacity(text.len().saturating_add(4));
    out.push_str(": ");
    out.push_str(text);
    out.push_str("\n\n");
    out.into_bytes()
}

/// The streaming response body: a blocking [`Read`] fed by a channel.
///
/// `tiny_http` calls [`read`](Read::read) repeatedly; each call drains the
/// current event's bytes, blocking on the channel when none are buffered. When
/// every [`Sender`] is dropped (the producer finished or the connection was
/// torn down) `read` returns `Ok(0)` — EOF — and the response ends cleanly.
#[derive(Debug)]
pub struct SseBody {
    /// Source of fully-encoded event byte blocks.
    rx: Receiver<Vec<u8>>,
    /// Bytes of the current block not yet copied to a caller.
    leftover: Vec<u8>,
    /// Read offset into `leftover`.
    pos: usize,
}

impl SseBody {
    /// Create a body draining `rx`.
    const fn new(rx: Receiver<Vec<u8>>) -> Self {
        Self { rx, leftover: Vec::new(), pos: 0 }
    }
}

impl Read for SseBody {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        // Refill from the channel when the current block is exhausted.
        if self.pos >= self.leftover.len() {
            match self.rx.recv() {
                Ok(block) => {
                    self.leftover = block;
                    self.pos = 0;
                }
                // All senders dropped — clean end of stream.
                Err(_) => return Ok(0),
            }
        }
        let remaining = self.leftover.get(self.pos..).unwrap_or(&[]);
        let n = remaining.len().min(out.len());
        if let (Some(src), Some(dst)) = (remaining.get(..n), out.get_mut(..n)) {
            dst.copy_from_slice(src);
        }
        self.pos = self.pos.saturating_add(n);
        Ok(n)
    }
}

/// A handle for pushing encoded events into a stream.
///
/// Each [`send`](SseSink::send) encodes one [`SseMessage`]; a send error means
/// the client disconnected (the [`SseBody`] was dropped), which the producer
/// uses as its stop signal.
#[derive(Clone, Debug)]
pub struct SseSink {
    /// Channel to the body reader.
    tx: Sender<Vec<u8>>,
}

impl SseSink {
    /// Encode and enqueue one event. Returns `Err` if the client is gone.
    ///
    /// # Errors
    ///
    /// Returns the unsent bytes if the receiving [`SseBody`] has been dropped.
    pub fn send(&self, msg: &SseMessage) -> Result<(), Vec<u8>> {
        self.tx.send(msg.encode()).map_err(|e| e.0)
    }

    /// Enqueue a keep-alive comment. Returns `Err` if the client is gone.
    ///
    /// # Errors
    ///
    /// Returns the unsent bytes if the receiving [`SseBody`] has been dropped.
    pub fn keep_alive(&self) -> Result<(), Vec<u8>> {
        self.tx.send(encode_comment("keep-alive")).map_err(|e| e.0)
    }
}

/// Create a connected ([`SseSink`], [`SseBody`]) pair.
///
/// The sink is moved into the producer thread; the body is handed to
/// `tiny_http` as the response reader.
#[must_use]
pub fn channel() -> (SseSink, SseBody) {
    let (tx, rx) = mpsc::channel();
    (SseSink { tx }, SseBody::new(rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_includes_id_event_and_data() {
        let msg = SseMessage::delta(42, "{\"a\":1}".to_owned());
        let wire = String::from_utf8(msg.encode()).expect("utf8");
        assert_eq!(wire, "id: 42\nevent: delta\ndata: {\"a\":1}\n\n");
    }

    #[test]
    fn encode_without_id_omits_id_line() {
        let msg = SseMessage::stream("hi".to_owned());
        let wire = String::from_utf8(msg.encode()).expect("utf8");
        assert_eq!(wire, "event: stream\ndata: hi\n\n");
    }

    #[test]
    fn encode_multiline_data_splits_per_line() {
        let msg = SseMessage { id: None, event: "x".to_owned(), data: "a\nb".to_owned() };
        let wire = String::from_utf8(msg.encode()).expect("utf8");
        assert_eq!(wire, "event: x\ndata: a\ndata: b\n\n");
    }

    #[test]
    fn comment_is_well_formed() {
        let wire = String::from_utf8(encode_comment("keep-alive")).expect("utf8");
        assert_eq!(wire, ": keep-alive\n\n");
    }

    #[test]
    fn body_yields_sent_bytes_then_eof() {
        let (sink, mut body) = channel();
        sink.send(&SseMessage::delta(1, "x".to_owned())).expect("send");
        drop(sink); // signal end of stream

        let mut all = Vec::new();
        let _n = body.read_to_end(&mut all).expect("read");
        let wire = String::from_utf8(all).expect("utf8");
        assert_eq!(wire, "id: 1\nevent: delta\ndata: x\n\n");
    }

    #[test]
    fn body_read_returns_zero_when_sink_dropped() {
        let (sink, mut body) = channel();
        drop(sink);
        let mut buf = [0u8; 16];
        assert_eq!(body.read(&mut buf).expect("read"), 0, "dropped sink ⇒ EOF");
    }

    #[test]
    fn body_handles_partial_reads_across_small_buffers() {
        let (sink, mut body) = channel();
        sink.send(&SseMessage::stream("abc".to_owned())).expect("send");
        drop(sink);

        // event = "event: stream\ndata: abc\n\n" — read it 3 bytes at a time.
        let mut assembled = Vec::new();
        let mut buf = [0u8; 3];
        loop {
            let n = body.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            assembled.extend_from_slice(buf.get(..n).expect("slice"));
        }
        let wire = String::from_utf8(assembled).expect("utf8");
        assert_eq!(wire, "event: stream\ndata: abc\n\n");
    }

    #[test]
    fn send_after_body_dropped_errors() {
        let (sink, body) = channel();
        drop(body);
        assert!(sink.send(&SseMessage::resync()).is_err(), "client gone ⇒ send errors");
    }
}
