//! Pre-flight validation result for tool calls.
//!
//! Runs before execution (even when queued) to catch obvious
//! parameter errors early without burning an API round-trip.

// =============================================================================
// Pre-flight validation
// =============================================================================

/// Result of pre-flight validation. Errors block execution; warnings are
/// attached to the result but the tool still runs.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// Blocking errors — tool execution will be refused.
    pub errors: Vec<String>,
    /// Non-blocking warnings — included in the result but tool runs.
    pub warnings: Vec<String>,
    /// When `true`, the pipeline activates the tool queue before the
    /// intercept check, ensuring this call is queued rather than
    /// executed immediately. Used by destructive operations that need
    /// queue protection (e.g. `Close_conversation_history`).
    pub activate_queue: bool,
}

impl Verdict {
    /// Empty result (no errors, no warnings).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` if any blocking errors were recorded.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// `true` if any warnings were recorded.
    #[must_use]
    pub const fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// `true` if both errors and warnings are empty.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }

    /// Append a blocking error (builder pattern).
    #[must_use]
    pub fn error<M: Into<String>>(mut self, msg: M) -> Self {
        self.errors.push(msg.into());
        self
    }

    /// Append a non-blocking warning (builder pattern).
    #[must_use]
    pub fn warning<M: Into<String>>(mut self, msg: M) -> Self {
        self.warnings.push(msg.into());
        self
    }

    /// Merge another `Verdict` into this one.
    pub fn merge(&mut self, other: Self) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.activate_queue = self.activate_queue || other.activate_queue;
    }

    /// Format errors and warnings into a human-readable string.
    #[must_use]
    pub fn format_errors(&self) -> String {
        let mut lines = Vec::new();
        for e in &self.errors {
            lines.push(format!("Error: {e}"));
        }
        for w in &self.warnings {
            lines.push(format!("Warning: {w}"));
        }
        lines.join("\n")
    }
}
