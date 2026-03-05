/// Collapsed sidebar rendering (icon + badge mode).
mod collapsed;
/// Full sidebar rendering with panel list and token stats.
mod full;

pub(super) use collapsed::render_sidebar_collapsed;
pub(super) use full::render_sidebar;
