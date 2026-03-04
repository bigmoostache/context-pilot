use std::sync::{Arc, Mutex};

/// Maximum ring buffer size in bytes (256 KB).
pub const RING_BUFFER_CAPACITY: usize = 256 * 1024;

#[derive(Debug)]
struct RingBufferInner {
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
            inner.buf[pos] = byte;
            pos += 1;
            if pos >= RING_BUFFER_CAPACITY {
                pos = 0;
                inner.wrapped = true;
            }
        }
        inner.write_pos = pos;
        inner.total_written += data.len() as u64;
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
            v.extend_from_slice(&inner.buf[inner.write_pos..]);
            v.extend_from_slice(&inner.buf[..inner.write_pos]);
            v
        } else {
            inner.buf[..inner.write_pos].to_vec()
        };
        (String::from_utf8_lossy(&bytes).into_owned(), total)
    }

    /// Return the last N lines of buffer content.
    #[must_use]
    pub fn last_n_lines(&self, n: usize) -> String {
        let (content, _) = self.read_all();
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(n);
        lines[start..].join("\n")
    }

    /// Check if the buffer content matches a regex pattern.
    /// Falls back to literal substring match if the regex is invalid.
    #[must_use]
    pub fn contains_pattern(&self, pattern: &str) -> bool {
        let (content, _) = self.read_all();
        match regex::Regex::new(pattern) {
            Ok(re) => re.is_match(&content),
            Err(_) => content.contains(pattern),
        }
    }

    /// Monotonic counter of total bytes written (for change detection).
    #[must_use]
    pub fn total_written(&self) -> u64 {
        let inner = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.total_written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let rb = RingBuffer::new();
        rb.write(b"hello world\n");
        let (content, total) = rb.read_all();
        assert_eq!(content, "hello world\n");
        assert_eq!(total, 12);
    }

    #[test]
    fn last_n_lines_basic() {
        let rb = RingBuffer::new();
        rb.write(b"line1\nline2\nline3\nline4\n");
        let last2 = rb.last_n_lines(2);
        assert_eq!(last2, "line3\nline4");
    }

    #[test]
    fn contains_pattern_found() {
        let rb = RingBuffer::new();
        rb.write(b"error: something failed\n");
        assert!(rb.contains_pattern("something failed"));
        assert!(!rb.contains_pattern("success"));
    }

    #[test]
    fn contains_pattern_regex() {
        let rb = RingBuffer::new();
        rb.write(b"[INFO] Server started on port 3000\n");
        assert!(rb.contains_pattern(r"port \d+"));
        assert!(rb.contains_pattern(r"Server started"));
        assert!(!rb.contains_pattern(r"^ERROR"));
    }

    #[test]
    fn total_written_monotonic() {
        let rb = RingBuffer::new();
        assert_eq!(rb.total_written(), 0);
        rb.write(b"abc");
        assert_eq!(rb.total_written(), 3);
        rb.write(b"def");
        assert_eq!(rb.total_written(), 6);
    }

    #[test]
    fn wrap_around() {
        // Use a small buffer to test wrapping
        let rb = RingBuffer::new();
        // Write more than capacity
        let data = vec![b'A'; RING_BUFFER_CAPACITY + 10];
        rb.write(&data);
        let (content, total) = rb.read_all();
        assert_eq!(total, RING_BUFFER_CAPACITY as u64 + 10);
        assert_eq!(content.len(), RING_BUFFER_CAPACITY);
        // After wrapping, content should be all A's
        assert!(content.chars().all(|c| c == 'A'));
    }

    #[test]
    fn clone_shares_state() {
        let rb1 = RingBuffer::new();
        let rb2 = rb1.clone();
        rb1.write(b"shared");
        let (content, _) = rb2.read_all();
        assert_eq!(content, "shared");
    }
}
