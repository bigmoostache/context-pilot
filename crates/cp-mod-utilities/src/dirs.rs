//! Platform-specific directory helpers.
//!
//! Replaces the `dirs` crate. Only implements the two functions actually
//! used in the project: [`home_dir`] and [`config_dir`]. Both are anchored on
//! the environment validated at boot, so neither can fail.

use std::path::PathBuf;

/// Return the user's home directory (`$HOME`, validated at boot).
#[must_use]
pub fn home_dir() -> PathBuf {
    cp_env::env().core.home.clone()
}

/// Return the platform-specific configuration directory.
///
/// - **macOS**: `~/Library/Application Support`
/// - **Linux/other**: `$XDG_CONFIG_HOME` or `~/.config`
#[must_use]
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library/Application Support")
    }
    #[cfg(not(target_os = "macos"))]
    {
        cp_env::env().core.xdg_config_home.clone()
    }
}
