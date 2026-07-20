//! Unicode box-drawing and indicator characters for TUI rendering.
//!
//! Values are written as `\u{XXXX}` escapes to keep the Rust source ASCII-only
//! (clippy::non_ascii_literal); the rendered glyph is shown in each doc comment.

/// Horizontal line segment (─, U+2500).
pub const HORIZONTAL: &str = "\u{2500}";
/// Full-width block (█, U+2588).
pub const BLOCK_FULL: &str = "\u{2588}";
/// Light shade block (░, U+2591).
pub const BLOCK_LIGHT: &str = "\u{2591}";
/// Filled circle (●, U+25CF).
pub const DOT: &str = "\u{25cf}";
/// Right-pointing triangle (▸, U+25B8).
pub const ARROW_RIGHT: &str = "\u{25b8}";
/// Up arrow (↑, U+2191).
pub const ARROW_UP: &str = "\u{2191}";
/// Down arrow (↓, U+2193).
pub const ARROW_DOWN: &str = "\u{2193}";
/// Cross mark (✗, U+2717).
pub const CROSS: &str = "\u{2717}";
