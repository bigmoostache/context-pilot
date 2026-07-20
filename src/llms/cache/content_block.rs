//! [`ContentBlock`] field accessors.
//!
//! Extracted from `mod.rs` for the 500-line cap. Each accessor funnels the
//! `&self` variant match through `cp_base::deref_match!` — the workspace's sole
//! ref-pattern chokepoint — so non-`Copy` field extraction stays
//! `pattern_type_mismatch`-clean without a per-site suppression.

use serde_json::Value;

use super::super::ContentBlock;

impl ContentBlock {
    /// Text payload, or `None` for a non-text block.
    pub(crate) const fn text(&self) -> Option<&str> {
        cp_base::deref_match!(self, {
            Self::Text { ref text } => Some(text.as_str()),
            Self::ToolUse { .. } => None,
            Self::ToolResult { .. } => None,
        })
    }

    /// `(id, name, input)` for a tool-use block, else `None`.
    pub(crate) const fn tool_use(&self) -> Option<(&str, &str, &Value)> {
        cp_base::deref_match!(self, {
            Self::ToolUse { ref id, ref name, ref input } => Some((id.as_str(), name.as_str(), input)),
            Self::Text { .. } => None,
            Self::ToolResult { .. } => None,
        })
    }

    /// `(tool_use_id, content)` for a tool-result block, else `None`.
    pub(crate) const fn tool_result(&self) -> Option<(&str, &str)> {
        cp_base::deref_match!(self, {
            Self::ToolResult { ref tool_use_id, ref content } => Some((tool_use_id.as_str(), content.as_str())),
            Self::Text { .. } => None,
            Self::ToolUse { .. } => None,
        })
    }
}
