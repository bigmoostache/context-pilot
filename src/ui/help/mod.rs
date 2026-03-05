/// Palette command definitions and fuzzy matching.
mod commands;
/// Configuration overlay (Ctrl+H) rendering.
pub(crate) mod config_overlay;
/// Command palette (Ctrl+P) state and rendering.
mod palette;

pub(crate) use palette::CommandPalette;
