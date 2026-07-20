//! Theme color accessors — RGB colors from the active theme.

use super::active_theme;
use ratatui::style::Color;

/// Convert an `[r, g, b]` triple to a ratatui RGB color.
const fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

/// Primary accent color.
#[must_use]
pub fn accent() -> Color {
    rgb(active_theme().colors.accent)
}
/// Dimmed accent for inactive highlights.
#[must_use]
pub fn accent_dim() -> Color {
    rgb(active_theme().colors.accent_dim)
}
/// Success indicator color.
#[must_use]
pub fn success() -> Color {
    rgb(active_theme().colors.success)
}
/// Warning indicator color.
#[must_use]
pub fn warning() -> Color {
    rgb(active_theme().colors.warning)
}
/// Error indicator color.
#[must_use]
pub fn error() -> Color {
    rgb(active_theme().colors.error)
}
/// Orange indicator color (elevated warnings, heavy usage).
#[must_use]
pub fn orange() -> Color {
    rgb(active_theme().colors.orange)
}
/// Primary text color.
#[must_use]
pub fn text() -> Color {
    rgb(active_theme().colors.text)
}
/// Secondary text color (labels, metadata).
#[must_use]
pub fn text_secondary() -> Color {
    rgb(active_theme().colors.text_secondary)
}
/// Muted text color (hints, disabled).
#[must_use]
pub fn text_muted() -> Color {
    rgb(active_theme().colors.text_muted)
}
/// Base background color.
#[must_use]
pub fn bg_base() -> Color {
    rgb(active_theme().colors.bg_base)
}
/// Elevated surface background (panels).
#[must_use]
pub fn bg_surface() -> Color {
    rgb(active_theme().colors.bg_surface)
}
/// Highest-elevation background (popups, overlays).
#[must_use]
pub fn bg_elevated() -> Color {
    rgb(active_theme().colors.bg_elevated)
}
/// Primary border color.
#[must_use]
pub fn border() -> Color {
    rgb(active_theme().colors.border)
}
/// Subtle border color (dividers).
#[must_use]
pub fn border_muted() -> Color {
    rgb(active_theme().colors.border_muted)
}
/// User message accent color.
#[must_use]
pub fn user() -> Color {
    rgb(active_theme().colors.user)
}
/// Assistant message accent color.
#[must_use]
pub fn assistant() -> Color {
    rgb(active_theme().colors.assistant)
}
