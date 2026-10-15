use std::sync::{Arc, Mutex};

/// Maximum ring buffer size in bytes (256 KB).
pub const RING_BUFFER_CAPACITY: usize = 256 * 1024;

/// Window scanned by [`RingBuffer::last_n_lines`] — enough for a handful of
/// lines without copying the whole 256 KiB buffer on every wait result.
const TAIL_WINDOW: u64 = 16 * 1024;

/// Internal mutable state of the ring buffer, protected by a mutex.
#[derive(Debug)]
struct RingBufferInner {
    /// Backing byte storage (fixed-size circular buffer).
    buf: Vec<u8>,
    /// Current write position in the circular buffer
    write_pos: usize,
    /// Total bytes written (monotonic, never wraps)
    total_written: u64,
    /// Whether the buffer has wrapped at least once
    wrapped: bool,
}

/// Thread-safe ring buffer for capturing process output.
/// Clone is cheap — it shares the inner buffer via Arc.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    /// Shared inner state behind an `Arc<Mutex>`.
    inner: Arc<Mutex<RingBufferInner>>,
}

/// A compiled `console_wait` pattern: regex, or literal bytes when the
/// pattern is not a valid regex (the documented fallback).
#[derive(Debug, Clone)]
pub struct Matcher {
    /// Compiled regex; `None` when the pattern is not a valid regex.
    regex: Option<regex::bytes::Regex>,
    /// Raw pattern bytes, matched literally when `regex` is `None`.
    literal: Vec<u8>,
}

impl Matcher {
    /// Compile `pattern`, falling back to a literal match if it is not a valid regex.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self { regex: regex::bytes::Regex::new(pattern).ok(), literal: pattern.as_bytes().to_vec() }
    }

    /// End offset (exclusive) of the first match in `hay`.
    #[must_use]
    pub fn find_end(&self, hay: &[u8]) -> Option<usize> {
        if let Some(re) = self.regex.as_ref() {
            return re.find(hay).map(|m| m.end());
        }
        let lit = self.literal.as_slice();
        if lit.is_empty() {
            return Some(0);
        }
        hay.windows(lit.len()).position(|w| w == lit).map(|p| p.saturating_add(lit.len()))
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl RingBuffer {
    /// Create a new ring buffer with default capacity (256KB).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RingBufferInner {
                buf: vec![0u8; RING_BUFFER_CAPACITY],
                write_pos: 0,
                total_written: 0,
                wrapped: false,
            })),
        }
    }

    /// Append bytes to the ring buffer, wrapping around as needed.
    ///
    /// Copies in at most two slices; input longer than the capacity keeps only
    /// its last `RING_BUFFER_CAPACITY` bytes (the rest would be overwritten anyway).
    pub fn write(&self, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.total_written = inner.total_written.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        let kept = data.get(data.len().saturating_sub(RING_BUFFER_CAPACITY)..).unwrap_or_default();
        if kept.len() < data.len() {
            inner.wrapped = true;
        }
        let pos = inner.write_pos;
        let first = kept.len().min(RING_BUFFER_CAPACITY.saturating_sub(pos));
        let (head, rest) = kept.split_at(first);
        if let Some(dst) = inner.buf.get_mut(pos..pos.saturating_add(first)) {
            dst.copy_from_slice(head);
        }
        if let Some(dst) = inner.buf.get_mut(..rest.len()) {
            dst.copy_from_slice(rest);
        }
        let end = pos.saturating_add(kept.len());
        if end >= RING_BUFFER_CAPACITY {
            inner.wrapped = true;
            inner.write_pos = end.saturating_sub(RING_BUFFER_CAPACITY);
        } else {
            inner.write_pos = end;
        }
    }

    /// Read the entire buffer contents as a string.
    /// Returns (content, `total_bytes_written`).
    #[must_use]
    pub fn read_all(&self) -> (String, u64) {
        let (bytes, total) = self.read_since(0);
        (String::from_utf8_lossy(&bytes).into_owned(), total)
    }

    /// Return the last N lines of buffer content (scans only the last 16 KiB).
    #[must_use]
    pub fn last_n_lines(&self, n: usize) -> String {
        let (bytes, _) = self.read_since(self.total_written().saturating_sub(TAIL_WINDOW));
        let content = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(n);
        lines.get(start..).unwrap_or_default().join("\n")
    }

    /// First match of `matcher` in output written after `cursor`.
    ///
    /// Returns the absolute (monotonic) offset just past the match, so callers
    /// can advance their cursor and never match the same output twice.
    #[must_use]
    pub fn find_since(&self, cursor: u64, matcher: &Matcher) -> Option<u64> {
        let (bytes, total) = self.read_since(cursor);
        let start = total.saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let end = matcher.find_end(&bytes)?;
        Some(start.saturating_add(u64::try_from(end).unwrap_or(u64::MAX)))
    }

    /// Bytes written after the monotonic offset `cursor`, plus the new cursor.
    ///
    /// When `cursor` points before the oldest retained byte (output evicted by
    /// wrap-around), reading resumes at the oldest byte still held.
    #[must_use]
    pub fn read_since(&self, cursor: u64) -> (Vec<u8>, u64) {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = inner.total_written;
        let held = if inner.wrapped { RING_BUFFER_CAPACITY } else { inner.write_pos };
        let wanted = usize::try_from(total.saturating_sub(cursor)).unwrap_or(usize::MAX).min(held);
        // `wanted` bytes end at write_pos (exclusive), possibly wrapping backwards.
        let mut out = Vec::with_capacity(wanted);
        if wanted <= inner.write_pos {
            out.extend_from_slice(
                inner.buf.get(inner.write_pos.saturating_sub(wanted)..inner.write_pos).unwrap_or_default(),
            );
        } else {
            let tail = wanted.saturating_sub(inner.write_pos);
            out.extend_from_slice(inner.buf.get(RING_BUFFER_CAPACITY.saturating_sub(tail)..).unwrap_or_default());
            out.extend_from_slice(inner.buf.get(..inner.write_pos).unwrap_or_default());
        }
        drop(inner);
        (out, total)
    }

    /// Monotonic counter of total bytes written (for change detection).
    #[must_use]
    pub fn total_written(&self) -> u64 {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.total_written
    }
}
