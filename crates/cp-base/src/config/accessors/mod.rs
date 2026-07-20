//! Thin accessor modules for compile-time embedded configuration.
//!
//! Each sub-module provides zero-cost getters backed by static `LazyLock` singletons.
//! Grouped here to keep `config/mod.rs` focused on type definitions and loading.

use std::sync::atomic::{AtomicU8, Ordering};

use super::{DEFAULT_THEME, THEME_ORDER, THEMES, Theme, invariant_panic};

// =============================================================================
// ACTIVE THEME (Global State — index-based lookup, fully safe)
// =============================================================================

/// Index into [`THEME_ORDER`] for the active theme.
/// `u8::MAX` = not yet set (uses [`DEFAULT_THEME`] on first access).
static CACHED_THEME_IDX: AtomicU8 = AtomicU8::new(u8::MAX);

/// Resolve a theme-order index to its theme reference.
fn theme_by_index(idx: u8) -> Option<&'static Theme> {
    let id = THEME_ORDER.get(usize::from(idx))?;
    THEMES.themes.get(*id)
}

/// Find the index of `theme_id` in [`THEME_ORDER`], or `None` if absent.
fn theme_index(theme_id: &str) -> Option<u8> {
    let i = THEME_ORDER.iter().position(|&id| id == theme_id)?;
    u8::try_from(i).ok()
}

/// Set the active theme ID (call when state is loaded or theme changes).
pub fn set_active_theme(theme_id: &str) {
    if let Some(idx) = theme_index(theme_id) {
        CACHED_THEME_IDX.store(idx, Ordering::Release);
    }
}

/// Get the currently active theme (atomic load + `HashMap` lookup — no unsafe).
///
/// Falls back to the default theme, then to any available theme.
///
/// # Panics
///
/// Panics if the themes map contains zero entries.
pub fn active_theme() -> &'static Theme {
    let idx = CACHED_THEME_IDX.load(Ordering::Acquire);
    let resolve = |theme: Option<&'static Theme>| -> &'static Theme {
        theme.unwrap_or_else(|| invariant_panic("themes.yaml has no themes"))
    };
    if idx == u8::MAX {
        // First call before set_active_theme — initialize from default
        let default_idx = theme_index(DEFAULT_THEME);
        if let Some(di) = default_idx {
            CACHED_THEME_IDX.store(di, Ordering::Release);
        }
        resolve(default_idx.and_then(theme_by_index).or_else(|| THEMES.themes.values().next()))
    } else {
        resolve(theme_by_index(idx).or_else(|| THEMES.themes.values().next()))
    }
}

// =============================================================================
// THEME COLORS (loaded from active theme in yamls/themes.yaml)
// =============================================================================

/// Theme color accessors from the active theme.
///
/// Each returns a `ratatui::style::Color::Rgb`. Zero-cost after first call
/// (atomic pointer).
pub mod theme;

// =============================================================================
// UI CHARACTERS
// =============================================================================

/// Unicode box-drawing and indicator characters for TUI rendering.
pub mod chars;

// =============================================================================
// ICONS / EMOJIS (loaded from active theme in yamls/themes.yaml)
// All icons are normalized to 2 display cells width for consistent alignment
// =============================================================================

/// Message and context icons from the active theme.
pub mod icons;

// =============================================================================
// PROMPTS (loaded from yamls/prompts.yaml via config module)
// =============================================================================

/// Accessors for the seed prompt library (agents, skills, commands).
pub mod library;

/// Accessors for prompt templates (panel header/footer formatting).
pub mod prompts;
