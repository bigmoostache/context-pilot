use std::sync::{Arc, Mutex};

/// Maximum ring buffer size in bytes (256 KB).
pub const RING_BUFFER_CAPACITY: usize = 256 * 1024;

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
    pub fn write(&self, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut pos = inner.write_pos;
        for &byte in data {
            if let Some(slot) = inner.buf.get_mut(pos) {
                *slot = byte;
            }
            pos = pos.saturating_add(1);
            if pos >= RING_BUFFER_CAPACITY {
                pos = 0;
                inner.wrapped = true;
            }
        }
        inner.write_pos = pos;
        inner.total_written = inner.total_written.saturating_add(data.len() as u64);
    }

    /// Read the entire buffer contents as a string.
    /// Returns (content, `total_bytes_written`).
    #[must_use]
    pub fn read_all(&self) -> (String, u64) {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = inner.total_written;
        let bytes = if inner.wrapped {
            // Data from write_pos..end, then 0..write_pos
            let mut v = Vec::with_capacity(RING_BUFFER_CAPACITY);
            v.extend_from_slice(inner.buf.get(inner.write_pos..).unwrap_or_default());
            v.extend_from_slice(inner.buf.get(..inner.write_pos).unwrap_or_default());
            v
        } else {
            inner.buf.get(..inner.write_pos).unwrap_or_default().to_vec()
        };
        (String::from_utf8_lossy(&bytes).into_owned(), total)
    }

    /// Return the last N lines of buffer content.
    #[must_use]
    pub fn last_n_lines(&self, n: usize) -> String {
        let (content, _) = self.read_all();
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(n);
        lines.get(start..).unwrap_or_default().join("\n")
    }

    /// Check if the buffer content matches a regex pattern.
    /// Falls back to literal substring match if the regex is invalid.
    #[must_use]
    pub fn contains_pattern(&self, pattern: &str) -> bool {
        let (content, _) = self.read_all();
        regex::Regex::new(pattern).map_or_else(|_| content.contains(pattern), |re| re.is_match(&content))
    }

    /// Monotonic counter of total bytes written (for change detection).
    #[must_use]
    pub fn total_written(&self) -> u64 {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.total_written
    }
}
