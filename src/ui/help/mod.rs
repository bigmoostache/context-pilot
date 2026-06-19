/// Palette command definitions and fuzzy matching.
mod commands;
/// Configuration overlay (Ctrl+H) rendering.
pub(crate) mod config_overlay;
/// Question form and autocomplete popup overlay rendering.
pub(crate) mod input;
/// MCP server setup overlay (list, add, remove, auth config).
pub(crate) mod mcp_overlay;
/// Command palette (Ctrl+P) state and rendering.
mod palette;

pub(crate) use palette::CommandPalette;
