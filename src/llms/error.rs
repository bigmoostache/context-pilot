use std::fmt;

/// Typed error for LLM streaming operations.
///
/// Replaces `Result<(), String>` to distinguish error categories
/// (auth, network, API, stream read, parse) without losing context.
#[derive(Debug)]
pub enum LlmError {
    /// Missing or invalid API key / OAuth token
    Auth(String),
    /// Network-level failure (DNS, connection, timeout)
    Network(String),
    /// API returned a non-success HTTP status
    Api { status: u16, body: String },
    /// Error reading from the SSE stream
    StreamRead(String),
    /// Failed to parse response JSON
    Parse(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Auth(msg) => write!(f, "Auth error: {}", msg),
            LlmError::Network(msg) => write!(f, "Network error: {}", msg),
            LlmError::Api { status, body } => write!(f, "API error {}: {}", status, body),
            LlmError::StreamRead(msg) => write!(f, "Stream read error: {}", msg),
            LlmError::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        LlmError::Network(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_auth() {
        let e = LlmError::Auth("key missing".into());
        assert_eq!(e.to_string(), "Auth error: key missing");
    }

    #[test]
    fn display_api() {
        let e = LlmError::Api { status: 429, body: "rate limited".into() };
        assert_eq!(e.to_string(), "API error 429: rate limited");
    }

    #[test]
    fn display_network() {
        let e = LlmError::Network("timeout".into());
        assert_eq!(e.to_string(), "Network error: timeout");
    }

    #[test]
    fn display_stream_read() {
        let e = LlmError::StreamRead("connection reset".into());
        assert_eq!(e.to_string(), "Stream read error: connection reset");
    }

    #[test]
    fn display_parse() {
        let e = LlmError::Parse("invalid json".into());
        assert_eq!(e.to_string(), "Parse error: invalid json");
    }
}
