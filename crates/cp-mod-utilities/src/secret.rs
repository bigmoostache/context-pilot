//! Secret string wrapper that prevents accidental logging.
//!
//! Replaces the `secrecy` crate. Provides a simple newtype around `String`
//! with `Debug` and `Display` implementations that redact the contents.

/// A string value that should not be logged or displayed.
///
/// `Debug` and `Display` both print `[REDACTED]` instead of the inner value.
/// Use [`expose_secret`](Redacted::expose_secret) to access the contents.
#[derive(Clone)]
pub struct Redacted(String);

impl Redacted {
    /// Wrap a string as a secret.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self(value)
    }

    /// Access the secret value.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl std::fmt::Display for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
