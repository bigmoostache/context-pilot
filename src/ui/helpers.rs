use cp_base::cast::Safe as _;
use unicode_width::UnicodeWidthStr as _;

/// Truncate a string to fit within `max_width` display columns, appending '…' if truncated.
pub(crate) fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        s.to_string()
    } else {
        let mut result = String::new();
        let mut width = 0usize;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if width.saturating_add(cw).saturating_add(1) > max_width {
                result.push('…');
                break;
            }
            result.push(c);
            width = width.saturating_add(cw);
        }
        result
    }
}

/// Format a number with K/M suffix for compact display.
pub(crate) fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n.to_f64() / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n.to_f64() / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a millisecond delta as a human-readable "x ago" string.
/// Uses `time_arith` helpers for all truncating integer division.
pub(crate) fn format_time_ago(delta_ms: u64) -> String {
    let seconds = cp_base::panels::time_arith::ms_to_secs(delta_ms);
    let (_, minutes, _) = cp_base::panels::time_arith::secs_to_hms_unwrapped(seconds);
    let (hours, _, _) = cp_base::panels::time_arith::secs_to_hms_unwrapped(seconds);
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{minutes}m ago")
    } else {
        format!("{hours}h ago")
    }
}

/// Word-wrap text to fit within a given width.
pub(crate) fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();

        if current_width == 0 {
            // First word on line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width.saturating_add(1).saturating_add(word_width) <= max_width {
            // Word fits on current line
            current_line.push(' ');
            current_line.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
        } else {
            // Word doesn't fit, start new line
            lines.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Count how many lines a `Line` will take when wrapped to a given width.
/// Uses unicode width for accurate display width calculation.
pub(crate) fn count_wrapped_lines(line: &ratatui::prelude::Line<'_>, max_width: usize) -> usize {
    if max_width == 0 {
        return 1;
    }

    // Concatenate all span content
    let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    if full_text.is_empty() {
        return 1;
    }

    // Simulate word wrapping
    let mut line_count = 1usize;
    let mut current_width = 0usize;

    for word in full_text.split_inclusive(|c: char| c.is_whitespace()) {
        let word_width = word.width();

        if current_width == 0 {
            current_width = word_width;
        } else if current_width.saturating_add(word_width) <= max_width {
            current_width = current_width.saturating_add(word_width);
        } else {
            // Word doesn't fit, start new line
            line_count = line_count.saturating_add(1);
            current_width = word_width;
        }

        // Handle very long words that need to be broken
        while current_width > max_width {
            line_count = line_count.saturating_add(1);
            current_width = current_width.saturating_sub(max_width);
        }
    }

    line_count
}
// Re-export from cp-base
pub(crate) use cp_base::ui::{Cell, render_table};

// ─── Spinner ─────────────────────────────────────────────────────────────────

/// Braille spinner frames (smooth 10-frame animation)
const SPINNER_BRAILLE: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Get a braille spinner frame for the given animation counter.
pub(crate) fn spinner(frame: u64) -> &'static str {
    SPINNER_BRAILLE.iter().copied().cycle().nth(frame.to_usize()).unwrap_or("⠋")
}

// ─── Syntax Highlighting ─────────────────────────────────────────────────────

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use ratatui::style::Color;

/// Lazily-loaded default syntax definitions.
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
/// Lazily-loaded default theme set.
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);
/// Type alias for syntax-highlighted line data.
type HighlightResult = Vec<Vec<(Color, String)>>;
/// Type alias for the highlight cache map.
type HighlightCache = Mutex<HashMap<String, Arc<HighlightResult>>>;
/// LRU-style cache for syntax highlighting results.
static HIGHLIGHT_CACHE: LazyLock<HighlightCache> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Convert syntect color to ratatui color
const fn to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Get syntax-highlighted spans for a file.
/// Returns Vec of lines, where each line is Vec of (color, text) pairs.
pub(crate) fn highlight_file(path: &str, content: &str) -> Arc<HighlightResult> {
    // Check cache first (keyed by path + content hash for simplicity)
    let cache_key = format!("{}:{}", path, content.len());
    {
        let cache = HIGHLIGHT_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.get(&cache_key) {
            return Arc::clone(cached);
        }
    }

    let result = Arc::new(do_highlight(path, content));

    // Store in cache
    {
        let mut cache = HIGHLIGHT_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Limit cache size
        if cache.len() > 50 {
            cache.clear();
        }
        let _r = cache.insert(cache_key, Arc::clone(&result));
    }

    result
}

/// Perform syntax highlighting on the given file content.
fn do_highlight(path: &str, content: &str) -> Vec<Vec<(Color, String)>> {
    // Find syntax for this file
    let syntax = SYNTAX_SET
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| {
            // Try by extension
            Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| SYNTAX_SET.find_syntax_by_extension(ext))
        })
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    // Use a dark theme
    let Some(theme_ref) = THEME_SET.themes.get("base16-ocean.dark") else {
        return Vec::new();
    };

    let mut highlighter = HighlightLines::new(syntax, theme_ref);
    let mut result = Vec::new();

    for line in LinesWithEndings::from(content) {
        let ranges: Vec<(Style, &str)> = highlighter.highlight_line(line, &SYNTAX_SET).unwrap_or_default();

        let spans: Vec<(Color, String)> = ranges
            .into_iter()
            .map(|(style, text)| {
                let color = to_ratatui_color(style.foreground);
                // Remove trailing newline from text for display
                let text = text.trim_end_matches('\n').to_string();
                (color, text)
            })
            .collect();

        result.push(spans);
    }

    result
}

// ─── IR-aware Syntax Highlighting ────────────────────────────────────────────

/// Type alias for IR-highlighted line data (RGB colour spans).
type IrHighlightResult = Vec<Vec<cp_render::Span>>;
/// LRU-style cache for IR syntax highlighting results.
type IrHighlightCache = Mutex<HashMap<String, Arc<IrHighlightResult>>>;
/// Cached IR highlight results, keyed by `path:content_len`.
static IR_HIGHLIGHT_CACHE: LazyLock<IrHighlightCache> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get IR-aware syntax-highlighted spans for a file.
///
/// Returns `Vec` of lines, where each line is `Vec<cp_render::Span>` with
/// RGB colour overrides from the syntect theme. Used by `FilePanel::blocks()`
/// to emit IR blocks instead of ratatui-coupled `(Color, String)` tuples.
pub(crate) fn highlight_file_ir(path: &str, content: &str) -> Arc<IrHighlightResult> {
    let cache_key = format!("{}:{}", path, content.len());
    {
        let cache = IR_HIGHLIGHT_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cached) = cache.get(&cache_key) {
            return Arc::clone(cached);
        }
    }

    let result = Arc::new(do_highlight_ir(path, content));

    {
        let mut cache = IR_HIGHLIGHT_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.len() > 50 {
            cache.clear();
        }
        let _r = cache.insert(cache_key, Arc::clone(&result));
    }

    result
}

/// Perform syntax highlighting, returning IR spans with RGB colour overrides.
fn do_highlight_ir(path: &str, content: &str) -> IrHighlightResult {
    let syntax = SYNTAX_SET
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| {
            Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| SYNTAX_SET.find_syntax_by_extension(ext))
        })
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let Some(theme_ref) = THEME_SET.themes.get("base16-ocean.dark") else {
        return Vec::new();
    };

    let mut highlighter = HighlightLines::new(syntax, theme_ref);
    let mut result = Vec::new();

    for line in LinesWithEndings::from(content) {
        let ranges: Vec<(Style, &str)> = highlighter.highlight_line(line, &SYNTAX_SET).unwrap_or_default();

        let spans: Vec<cp_render::Span> = ranges
            .into_iter()
            .map(|(style, text)| {
                let text = text.trim_end_matches('\n').to_string();
                cp_render::Span::rgb(text, style.foreground.r, style.foreground.g, style.foreground.b)
            })
            .collect();

        result.push(spans);
    }

    result
}

// ─── Typewriter Buffer ───────────────────────────────────────────────────────

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::infra::constants::{
    TYPEWRITER_DEFAULT_DELAY_MS, TYPEWRITER_MAX_DELAY_MS, TYPEWRITER_MIN_DELAY_MS, TYPEWRITER_MOVING_AVG_SIZE,
};

/// Buffered typewriter animation: accumulates streaming chunks and releases
/// characters at a smoothed rate for a natural typing effect.
pub(crate) struct TypewriterBuffer {
    /// Characters waiting to be released to the display.
    pub pending_chars: VecDeque<char>,
    /// Recent inter-chunk intervals for speed estimation.
    pub chunk_intervals: VecDeque<Duration>,
    /// Recent chunk sizes (in characters) for speed estimation.
    pub chunk_sizes: VecDeque<usize>,
    /// Timestamp of the last received chunk.
    pub last_chunk_time: Option<Instant>,
    /// Timestamp of the last character release.
    pub last_char_time: Instant,
    /// Current estimated characters-per-millisecond release rate.
    pub chars_per_ms: f64,
    /// Whether the upstream stream has finished sending chunks.
    pub stream_done: bool,
}

impl TypewriterBuffer {
    /// Create a new typewriter buffer with default speed.
    pub(crate) fn new() -> Self {
        Self {
            pending_chars: VecDeque::new(),
            chunk_intervals: VecDeque::new(),
            chunk_sizes: VecDeque::new(),
            last_chunk_time: None,
            last_char_time: Instant::now(),
            chars_per_ms: 1.0 / TYPEWRITER_DEFAULT_DELAY_MS,
            stream_done: false,
        }
    }

    /// Reset the buffer to its initial state, clearing all pending data.
    pub(crate) fn reset(&mut self) {
        self.pending_chars.clear();
        self.chunk_intervals.clear();
        self.chunk_sizes.clear();
        self.last_chunk_time = None;
        self.last_char_time = Instant::now();
        self.chars_per_ms = 1.0 / TYPEWRITER_DEFAULT_DELAY_MS;
        self.stream_done = false;
    }

    /// Add a new text chunk from the stream, updating speed estimates.
    pub(crate) fn add_chunk(&mut self, text: &str) {
        let now = Instant::now();

        if let Some(last_time) = self.last_chunk_time {
            let interval = now.duration_since(last_time);
            if self.chunk_intervals.len() >= TYPEWRITER_MOVING_AVG_SIZE {
                let _r = self.chunk_intervals.pop_front();
            }
            self.chunk_intervals.push_back(interval);
        }
        self.last_chunk_time = Some(now);

        let char_count = text.chars().count();
        if self.chunk_sizes.len() >= TYPEWRITER_MOVING_AVG_SIZE {
            let _r = self.chunk_sizes.pop_front();
        }
        self.chunk_sizes.push_back(char_count);

        for c in text.chars() {
            self.pending_chars.push_back(c);
        }

        self.recalculate_speed();
    }

    /// Recalculate the release speed based on recent chunk intervals and sizes.
    fn recalculate_speed(&mut self) {
        if self.chunk_intervals.is_empty() || self.chunk_sizes.is_empty() {
            return;
        }

        let total_interval_ms: f64 = self.chunk_intervals.iter().map(|d| d.as_secs_f64() * 1000.0).sum();
        let avg_interval_ms = total_interval_ms / self.chunk_intervals.len().to_f64();

        let total_chars: usize = self.chunk_sizes.iter().sum();
        let avg_chunk_size = total_chars.to_f64() / self.chunk_sizes.len().to_f64();

        if avg_interval_ms > 0.0 && avg_chunk_size > 0.0 {
            let calculated_delay = avg_interval_ms / avg_chunk_size;
            let clamped_delay = calculated_delay.clamp(TYPEWRITER_MIN_DELAY_MS, TYPEWRITER_MAX_DELAY_MS);
            self.chars_per_ms = 1.0 / clamped_delay;
        }
    }

    /// Mark the stream as finished; remaining chars will be flushed faster.
    pub(crate) const fn mark_done(&mut self) {
        self.stream_done = true;
    }

    /// Release characters that are due based on elapsed time.
    /// Returns `None` if no characters are ready yet.
    pub(crate) fn take_chars(&mut self) -> Option<String> {
        if self.pending_chars.is_empty() {
            return None;
        }

        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.last_char_time).as_secs_f64() * 1000.0;
        let chars_to_release = (elapsed_ms * self.chars_per_ms).floor().to_usize();

        if chars_to_release == 0 {
            return None;
        }

        let chars_to_take = if self.stream_done {
            chars_to_release.max(2).min(self.pending_chars.len())
        } else {
            chars_to_release.min(self.pending_chars.len())
        };

        if chars_to_take == 0 {
            return None;
        }

        self.last_char_time = now;

        let mut result = String::with_capacity(chars_to_take);
        for _ in 0..chars_to_take {
            if let Some(c) = self.pending_chars.pop_front() {
                result.push(c);
            }
        }

        Some(result)
    }
}
