//! Accessors for prompt templates (panel header/footer formatting).

use crate::config::PROMPTS;

/// Panel opening header template (`{id}`, `{type}`, `{name}` placeholders).
#[must_use]
pub fn panel_header() -> &'static str {
    &PROMPTS.panel.header
}
/// Panel timestamp template (`{timestamp}` placeholder).
#[must_use]
pub fn panel_timestamp() -> &'static str {
    &PROMPTS.panel.timestamp
}
/// Fallback when panel has no known timestamp.
#[must_use]
pub fn panel_timestamp_unknown() -> &'static str {
    &PROMPTS.panel.timestamp_unknown
}
/// Panel closing footer template.
#[must_use]
pub fn panel_footer() -> &'static str {
    &PROMPTS.panel.footer
}
/// Assistant ack injected after footer.
#[must_use]
pub fn panel_footer_ack() -> &'static str {
    &PROMPTS.panel.footer_ack
}
