//! Buffered, byte-counting terminal writer.
//!
//! `io::Stdout` is a 1 KiB `LineWriter`: ratatui queues escape sequences per
//! cell, so a large diff turned into hundreds of blocking `write(2)` calls to
//! the pty. [`TtyWriter`] buffers a whole frame and hands it to the terminal in
//! one write on flush, and counts the bytes so the frame cost is measurable.

use std::io::{self, BufWriter, Stdout, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use cp_base::cast::Safe as _;
use ratatui::prelude::{CrosstermBackend, Terminal};

/// Buffer size: holds a full truecolor repaint of a large screen in one piece;
/// bigger frames just spill into a second write.
const FRAME_BUF_BYTES: usize = 256 * 1024;

/// Bytes accepted by the (single) app terminal since the last [`take_written`].
/// A static because ratatui gates `CrosstermBackend::writer_mut` behind an
/// unstable feature, so the writer itself is unreachable once wrapped.
static WRITTEN: AtomicU64 = AtomicU64::new(0);

/// The app's terminal type.
pub(crate) type Tty = Terminal<CrosstermBackend<TtyWriter>>;

/// Stdout behind a frame-sized buffer; every accepted byte is counted.
pub(crate) struct TtyWriter {
    /// Buffered stdout; flushed once per frame.
    inner: BufWriter<Stdout>,
}

impl Write for TtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        let _prev = WRITTEN.fetch_add(n.to_u64(), Ordering::Relaxed);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Bytes written to the terminal since the previous call, resetting the count.
pub(crate) fn take_written() -> u64 {
    WRITTEN.swap(0, Ordering::Relaxed)
}

/// Create the app terminal over a [`TtyWriter`].
pub(crate) fn new_terminal() -> io::Result<Tty> {
    Terminal::new(CrosstermBackend::new(TtyWriter { inner: BufWriter::with_capacity(FRAME_BUF_BYTES, io::stdout()) }))
}
