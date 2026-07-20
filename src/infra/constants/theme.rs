//! Theme color accessors loaded from the active YAML theme.

use crate::infra::config::active_theme;
use ratatui::style::Color;

/// Convert an RGB array to a ratatui `Color`.
const fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

// Primary brand colors

/// Accent color from the active theme.
pub(crate) fn accent() -> Color {
    rgb(active_theme().colors.accent)
}
/// Dimmed accent color from the active theme.
pub(crate) fn accent_dim() -> Color {
    rgb(active_theme().colors.accent_dim)
}
/// Success color from the active theme.
pub(crate) fn success() -> Color {
    rgb(active_theme().colors.success)
}
/// Warning color from the active theme.
pub(crate) fn warning() -> Color {
    rgb(active_theme().colors.warning)
}
/// Error color from the active theme.
pub(crate) fn error() -> Color {
    rgb(active_theme().colors.error)
}
/// Orange color from the active theme.
pub(crate) fn orange() -> Color {
    rgb(active_theme().colors.orange)
}

// Text colors

/// Primary text color from the active theme.
pub(crate) fn text() -> Color {
    rgb(active_theme().colors.text)
}
/// Secondary text color from the active theme.
pub(crate) fn text_secondary() -> Color {
    rgb(active_theme().colors.text_secondary)
}
/// Muted text color from the active theme.
pub(crate) fn text_muted() -> Color {
    rgb(active_theme().colors.text_muted)
}

// Background colors

/// Base background color from the active theme.
pub(crate) fn bg_base() -> Color {
    rgb(active_theme().colors.bg_base)
}
/// Surface background color from the active theme.
pub(crate) fn bg_surface() -> Color {
    rgb(active_theme().colors.bg_surface)
}
/// Elevated background color from the active theme.
pub(crate) fn bg_elevated() -> Color {
    rgb(active_theme().colors.bg_elevated)
}

// Border colors

/// Border color from the active theme.
pub(crate) fn border() -> Color {
    rgb(active_theme().colors.border)
}
/// Muted border color from the active theme.
pub(crate) fn border_muted() -> Color {
    rgb(active_theme().colors.border_muted)
}

// Role-specific colors

/// Assistant role color from the active theme.
pub(crate) fn assistant() -> Color {
    rgb(active_theme().colors.assistant)
}

// Status-bar card colors — fixed design tokens (not theme-dependent).
// Named here so they're discoverable and easy to make theme-able later.

/// High-contrast text on colored card backgrounds.
pub(crate) const fn card_text() -> Color {
    Color::Rgb(255, 255, 255)
}
/// Agent card background — purple.
pub(crate) const fn card_agent_bg() -> Color {
    Color::Rgb(130, 80, 200)
}
/// Reverie card background — dark purple.
pub(crate) const fn card_reverie_bg() -> Color {
    Color::Rgb(100, 60, 160)
}
/// Queue card background — amber.
pub(crate) const fn card_queue_bg() -> Color {
    Color::Rgb(180, 120, 40)
}
/// Think balance card background — muted red.
pub(crate) const fn card_think_bg() -> Color {
    Color::Rgb(180, 60, 60)
}
