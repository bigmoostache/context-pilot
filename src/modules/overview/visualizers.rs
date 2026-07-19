//! Tool result visualizers for the overview module.

/// Visualizer for core tool results.
///
/// Colors closed panel names, highlights enabled/disabled tool status,
/// and shows module activation state changes.
pub(super) fn visualize_core_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.starts_with("Closed") || line.contains("enabled") {
                Semantic::Success
            } else if line.contains("disabled") {
                Semantic::Warning
            } else if line.contains("Reloading")
                || line.contains("TUI")
                || (line.starts_with('P') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
            {
                Semantic::Info
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_owned()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
