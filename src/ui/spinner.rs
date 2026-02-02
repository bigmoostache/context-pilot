//! Spinner and loading indicator utilities.

/// Braille spinner frames (smooth 10-frame animation)
const SPINNER_BRAILLE: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Get a braille spinner frame (default spinner)
pub fn spinner(frame: u64) -> &'static str {
    let index = (frame as usize) % SPINNER_BRAILLE.len();
    SPINNER_BRAILLE[index]
}
