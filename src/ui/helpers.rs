use cp_base::cast::SafeCast;
use unicode_width::UnicodeWidthStr;

pub(crate) fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        s.to_string()
    } else {
        let mut result = String::new();
        let mut width = 0;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if width + cw + 1 > max_width {
                result.push('…');
                break;
            }
            result.push(c);
            width += cw;
        }
        result
    }
}

pub(crate) fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n.to_f64() / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n.to_f64() / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a millisecond delta as a human-readable "x ago" string
pub(crate) fn format_time_ago(delta_ms: u64) -> String {
    let seconds = delta_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else {
        format!("{}h ago", seconds / 3600)
    }
}

/// Word-wrap text to fit within a given width
pub(crate) fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();

        if current_width == 0 {
            // First word on line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            // Word fits on current line
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
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

/// Count how many lines a Line will take when wrapped to a given width
/// Uses unicode width for accurate display width calculation
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
    let mut line_count = 1;
    let mut current_width = 0;

    for word in full_text.split_inclusive(|c: char| c.is_whitespace()) {
        let word_width = word.width();

        if current_width == 0 {
            current_width = word_width;
        } else if current_width + word_width <= max_width {
            current_width += word_width;
        } else {
            // Word doesn't fit, start new line
            line_count += 1;
            current_width = word_width;
        }

        // Handle very long words that need to be broken
        while current_width > max_width {
            line_count += 1;
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

/// Get a braille spinner frame (default spinner)
pub(crate) fn spinner(frame: u64) -> &'static str {
    let index = (frame.to_usize()) % SPINNER_BRAILLE.len();
    SPINNER_BRAILLE[index]
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

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);
type HighlightResult = Vec<Vec<(Color, String)>>;
static HIGHLIGHT_CACHE: LazyLock<Mutex<HashMap<String, Arc<HighlightResult>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Convert syntect color to ratatui color
const fn to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Get syntax-highlighted spans for a file
/// Returns Vec of lines, where each line is Vec of (color, text) pairs
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
    let theme = &THEME_SET.themes["base16-ocean.dark"];

    let mut highlighter = HighlightLines::new(syntax, theme);
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
