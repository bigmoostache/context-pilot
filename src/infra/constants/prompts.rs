//! Prompt template accessors loaded from YAML configuration.

use crate::infra::config::PROMPTS;

/// Prompt template for panel headers.
pub(crate) fn panel_header() -> &'static str {
    &PROMPTS.panel.header
}
/// Prompt template for panel timestamps.
pub(crate) fn panel_timestamp() -> &'static str {
    &PROMPTS.panel.timestamp
}
/// Prompt template for unknown panel timestamps.
pub(crate) fn panel_timestamp_unknown() -> &'static str {
    &PROMPTS.panel.timestamp_unknown
}
/// Prompt template for panel footers.
pub(crate) fn panel_footer() -> &'static str {
    &PROMPTS.panel.footer
}
/// Prompt template for panel footer acknowledgment.
pub(crate) fn panel_footer_ack() -> &'static str {
    &PROMPTS.panel.footer_ack
}
