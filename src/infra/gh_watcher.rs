//! Background polling watcher for `GithubResult` panels.
//!
//! Core logic lives in `cp_mod_github::watcher`. This module re-exports
//! the `GhWatcher` type for use by the binary's app event loop.

pub(crate) use cp_mod_github::watcher::Watcher as GhWatcher;
