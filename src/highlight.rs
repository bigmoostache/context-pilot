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
fn to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Get syntax-highlighted spans for a file
/// Returns Vec of lines, where each line is Vec of (color, text) pairs
pub fn highlight_file(path: &str, content: &str) -> Arc<HighlightResult> {
    // Check cache first (keyed by path + content hash for simplicity)
    let cache_key = format!("{}:{}", path, content.len());
    {
        let cache = HIGHLIGHT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.get(&cache_key) {
            return Arc::clone(cached);
        }
    }

    let result = Arc::new(do_highlight(path, content));

    // Store in cache
    {
        let mut cache = HIGHLIGHT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        // Limit cache size
        if cache.len() > 50 {
            cache.clear();
        }
        cache.insert(cache_key, Arc::clone(&result));
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
