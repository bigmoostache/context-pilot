//! Pure monitor mechanics.
//!
//! Line splitting, filtering, 200 ms batching, flood accounting and
//! oversized-payload spilling. No `State`, no I/O except the spill file,
//! so every rule here is unit-testable.

use std::collections::VecDeque;

use super::spec::Mode;

/// A batch is flushed once its first line is this old (ms).
pub const BATCH_WINDOW_MS: u64 = 200;
/// Sliding window for flood accounting (ms).
pub const FLOOD_WINDOW_MS: u64 = 3_600_000;
/// Events allowed per sliding window; one more kills the monitor.
pub const FLOOD_MAX_EVENTS: usize = 20;
/// Payloads above this many bytes are spilled to a log file (same cap as `console_easy_bash`).
pub const INLINE_MAX_BYTES: usize = crate::types::EASY_BASH_INLINE_MAX_BYTES;
/// Bytes of tail shown in the notification when the payload is spilled.
const SPILL_TAIL_BYTES: usize = 300;
/// Directory receiving spilled payloads.
pub const SPILL_DIR: &str = "/tmp/cp-monitor";

/// Which lines a monitor keeps.
#[derive(Debug, Clone, Copy)]
pub struct Filter<'re> {
    /// Firing condition.
    pub mode: Mode,
    /// Compiled pattern (`mode=pattern`).
    pub pattern: Option<&'re regex::Regex>,
}

impl Filter<'_> {
    /// Whether `line` passes this filter.
    fn keeps(self, line: &str) -> bool {
        match self.mode {
            Mode::Line => !line.trim().is_empty(),
            Mode::Pattern => self.pattern.is_some_and(|re| re.is_match(line)),
            Mode::Exit => false,
        }
    }
}

/// Line-level state of one monitor.
#[derive(Debug, Default)]
pub struct Engine {
    /// Bytes of an unterminated trailing line, kept until its newline arrives.
    partial: Vec<u8>,
    /// Lines waiting to be delivered.
    batch: Vec<String>,
    /// When the first line of `batch` arrived.
    batch_since_ms: Option<u64>,
    /// Timestamps of past deliveries inside the flood window.
    fires: VecDeque<u64>,
}

/// Outcome of recording a delivery against the flood budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    /// Within budget.
    Ok,
    /// More than [`FLOOD_MAX_EVENTS`] in the last hour: the monitor must die.
    Flooded,
}

impl Engine {
    /// Split new output bytes into complete lines and queue those the mode accepts.
    pub fn ingest(&mut self, bytes: &[u8], filter: Filter<'_>, now_ms: u64) {
        self.partial.extend_from_slice(bytes);
        let Some(last_nl) = self.partial.iter().rposition(|&b| b == b'\n') else { return };
        let rest = self.partial.split_off(last_nl.saturating_add(1));
        let complete = std::mem::replace(&mut self.partial, rest);
        for raw in String::from_utf8_lossy(&complete).lines() {
            self.offer(raw, filter, now_ms);
        }
    }

    /// Queue the unterminated trailing line (process ended without a final newline).
    pub fn flush_partial(&mut self, filter: Filter<'_>, now_ms: u64) {
        if self.partial.is_empty() {
            return;
        }
        let raw = String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned();
        self.offer(&raw, filter, now_ms);
    }

    /// Queue one line if it passes the mode's filter.
    fn offer(&mut self, raw: &str, filter: Filter<'_>, now_ms: u64) {
        let line = raw.trim_end_matches('\r');
        if filter.keeps(line) {
            let _first = self.batch_since_ms.get_or_insert(now_ms);
            self.batch.push(line.to_owned());
        }
    }

    /// Take the batch when its window has elapsed (or unconditionally with `force`).
    pub fn take_batch(&mut self, now_ms: u64, force: bool) -> Option<Vec<String>> {
        let since = self.batch_since_ms?;
        if !force && now_ms.saturating_sub(since) < BATCH_WINDOW_MS {
            return None;
        }
        self.batch_since_ms = None;
        Some(std::mem::take(&mut self.batch))
    }

    /// Record one delivery and say whether the hourly budget is blown.
    pub fn record_fire(&mut self, now_ms: u64) -> Budget {
        while self.fires.front().is_some_and(|&t| now_ms.saturating_sub(t) >= FLOOD_WINDOW_MS) {
            let _old = self.fires.pop_front();
        }
        self.fires.push_back(now_ms);
        if self.fires.len() > FLOOD_MAX_EVENTS { Budget::Flooded } else { Budget::Ok }
    }
}

/// Last `max` bytes of `s`, cut on a char boundary.
fn tail(s: &str, max: usize) -> &str {
    let mut start = s.len().saturating_sub(max);
    while !s.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    s.get(start..).unwrap_or_default()
}

/// Inline `content` when small; otherwise write it to `SPILL_DIR/<file_stem>.log`
/// and return a pointer plus the last few hundred bytes.
#[must_use]
pub fn render_payload(content: &str, file_stem: &str) -> String {
    if content.len() <= INLINE_MAX_BYTES {
        return content.to_owned();
    }
    let path = std::path::Path::new(SPILL_DIR).join(format!("{file_stem}.log"));
    let written = std::fs::create_dir_all(SPILL_DIR).and_then(|()| std::fs::write(&path, format!("{content}\n")));
    let where_ = match written {
        Ok(()) => format!("full output ({} bytes) in {}", content.len(), path.display()),
        Err(e) => format!("output too large ({} bytes) and spill to {} failed: {e}", content.len(), path.display()),
    };
    format!("[{where_}]\n…{}", tail(content, SPILL_TAIL_BYTES))
}
