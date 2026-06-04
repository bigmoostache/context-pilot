//! Platform-specific directory helpers.
//!
//! Replaces the `dirs` crate. Only implements the two functions actually
//! used in the project: [`home_dir`] and [`config_dir`].

use std::path::PathBuf;

/// Return the user's home directory.
///
/// Uses `$HOME` on all Unix platforms.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Return the platform-specific configuration directory.
///
/// - **macOS**: `~/Library/Application Support`
/// - **Linux/other**: `$XDG_CONFIG_HOME` or `~/.config`
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library/Application Support"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home_dir().map(|h| h.join(".config")))
    }
}
