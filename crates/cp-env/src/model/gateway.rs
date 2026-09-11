//! The optional LLM gateway.

use std::fmt;

use crate::resolve::Raw;

/// Gateway configuration. Fields are private so the key is redacted in
/// `Debug` and only leaves through [`Gateway::key`].
#[derive(Clone, PartialEq, Eq)]
pub struct Gateway {
    /// Base URL, trailing slashes trimmed.
    url: Option<String>,
    /// Key presented on every call.
    key: Option<String>,
}

impl Gateway {
    /// From the validated values.
    pub(crate) fn from_raw(raw: &Raw) -> Self {
        Self {
            url: raw.text("CP_LLM_GATEWAY").map(str::to_owned),
            key: raw.text("CP_LLM_GATEWAY_KEY").map(str::to_owned),
        }
    }

    /// Base URL when a gateway is configured.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The gateway key, if any.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Whether a gateway is configured at all.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.url.is_some()
    }
}

impl fmt::Debug for Gateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gateway")
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_secret| "[redacted]"))
            .finish()
    }
}
