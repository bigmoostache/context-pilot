//! IR-native markdown parser — delegates to the shared `cp_render::markdown` module.
//!
//! This module exists for backward compatibility with call sites in the
//! conversation renderer. New code should use `cp_render::markdown` directly.

/// Parse a markdown line and return IR spans.
///
/// Handles headers (`#`), bullet points (`- `, `* `), and inline formatting.
pub(super) fn parse_markdown_line_ir(line: &str) -> Vec<cp_render::Span> {
    cp_render::markdown::parse_line(line)
}
