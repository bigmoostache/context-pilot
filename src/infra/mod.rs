/// HTTP API helpers for LLM providers.
pub(crate) mod api;
/// Application configuration loaded from YAML files.
pub(crate) mod config;
/// Application-wide constants (colors, icons, prompts, layout values).
pub(crate) mod constants;
/// GitHub webhook / event watcher.
pub(crate) mod gh_watcher;
/// Simple profiler for identifying slow operations.
pub(crate) mod profiler;
/// Tool definition helpers.
pub(crate) mod tools;
/// File-system watcher for detecting changes to open files.
pub(crate) mod watcher;
